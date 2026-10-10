//! Sink expansion geometry, in final Overview logical pixels, independent of rendering.
//!
//! The same rectangles must feed rendering and passive input. Converting the resulting poses
//! back to workspace coordinates is the only place that divides by the Overview zoom.
use smithay::utils::{Logical, Point, Rectangle, Size};
use std::rc::Rc;

use super::sink_presentation::SinkPose;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Overhang {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
}

impl Overhang {
    pub fn include(&mut self, preview: Rectangle<f64, Logical>, rect: Rectangle<f64, Logical>) {
        self.top = self.top.max(preview.loc.y - rect.loc.y);
        self.bottom = self
            .bottom
            .max(rect.loc.y + rect.size.h - preview.loc.y - preview.size.h);
        self.left = self.left.max(preview.loc.x - rect.loc.x);
        self.right = self
            .right
            .max(rect.loc.x + rect.size.w - preview.loc.x - preview.size.w);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SinkOverviewBudget {
    pub visible_depth: usize,
    pub entry_height: f64,
    pub tail_height: f64,
    tail_decay: f64,
}

/// A capacity snapshot shared by all workspaces on one output, including empty workspaces.
/// It stays independent of stack counts. Output/config/curve changes replace this object.
#[derive(Debug, Clone, PartialEq)]
pub struct SinkOverviewOutputSnapshot {
    pub view_size: Size<f64, Logical>,
    pub zoom: f64,
    pub scale: f64,
    pub budget: SinkOverviewBudget,
    /// In final Overview pixels. Rendering and input use the same allocation.
    pub reserved: Overhang,
}

impl SinkOverviewOutputSnapshot {
    /// Chrome/curve reservations can grow without changing the native coordinate basis.
    pub fn same_geometry_basis(&self, other: &Self) -> bool {
        self.view_size == other.view_size
            && self.zoom == other.zoom
            && self.scale == other.scale
            && self.budget == other.budget
    }
}

#[derive(Debug, Clone)]
pub struct SinkOverviewFrame {
    pub output: Rc<SinkOverviewOutputSnapshot>,
    /// Existing Overview progress; no additional opening clock.
    pub progress: f64,
}

impl SinkOverviewBudget {
    /// Limit the whole expansion before assigning entries. This preserves their ordering even
    /// on a tiny output with the largest valid configuration. Capacity does not depend on N.
    pub fn new(config: niri_config::OverviewSink, visible_depth: usize, available: f64) -> Self {
        let visible_depth = visible_depth.clamp(1, 4);
        let entry_height = config.entry_height.clamp(1., 256.);
        let tail_height = config.tail_height.clamp(0., 256.);
        let requested = visible_depth as f64 * entry_height + tail_height;
        let fraction = (available.max(0.) / requested).min(1.);
        Self {
            visible_depth,
            entry_height: entry_height * fraction,
            tail_height: tail_height * fraction,
            tail_decay: config.tail_decay.clamp(0.01, 0.99),
        }
    }

    pub fn tail(&self, count: usize) -> f64 {
        // exp_m1 preserves the small first step when q is near one. No depth+1 overflow.
        -self.tail_height * (count as f64 * self.tail_decay.ln()).exp_m1()
    }

    pub fn capacity(&self) -> f64 {
        self.visible_depth as f64 * self.entry_height + self.tail_height
    }

    /// Reserve even for an empty stack, so its first Sink cannot change workspace spacing.
    /// Source and destination edges may be anywhere in the preview. Beyond [0, 1], their
    /// affine interpolation can escape it by a fraction of its full extent, not only B/2.
    pub fn reserve(
        &self,
        preview: Size<f64, Logical>,
        progress: (f64, f64),
        chrome: f64,
    ) -> Overhang {
        let extra = |p: f64, extent: f64, expansion: f64| {
            (-p).max(p - 1.).max(0.) * extent + p.abs() * expansion + chrome
        };
        let vertical = extra(progress.0, preview.h, self.capacity() / 2.).max(extra(
            progress.1,
            preview.h,
            self.capacity() / 2.,
        ));
        let horizontal = extra(progress.0, preview.w, 0.).max(extra(progress.1, preview.w, 0.));
        Overhang {
            top: vertical,
            bottom: vertical,
            left: horizontal,
            right: horizontal,
        }
    }

    pub fn extent(&self, count: usize) -> f64 {
        count.min(self.visible_depth) as f64 * self.entry_height
            + self.tail(count.saturating_sub(self.visible_depth))
    }

    pub fn offset(&self, depth: usize) -> f64 {
        if depth < self.visible_depth {
            (depth + 1) as f64 * self.entry_height
        } else {
            self.visible_depth as f64 * self.entry_height
                + self.tail(depth - self.visible_depth + 1)
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SinkOverviewEntry {
    pub rect: Rectangle<f64, Logical>,
    /// Deep tail members are visual hints, never passive selection targets.
    pub entrance: Option<Rectangle<f64, Logical>>,
    /// Scale in workspace coordinates; does not resize the client.
    pub scale: f64,
}

#[derive(Debug)]
pub struct SinkOverviewGeometry {
    pub extent: f64,
    pub foreground_offset: f64,
    pub entries: Vec<SinkOverviewEntry>,
}

impl SinkOverviewGeometry {
    /// `preview` and budget are final Overview pixels; source sizes are native workspace pixels,
    /// in LIFO order. Fit preserves each source's aspect ratio and never enlarges a client.
    pub fn new(
        preview: Rectangle<f64, Logical>,
        zoom: f64,
        mut budget: SinkOverviewBudget,
        sources: &[Size<f64, Logical>],
    ) -> Self {
        assert!(zoom.is_finite() && zoom > 0.);
        assert!(preview.size.w.is_finite() && preview.size.h.is_finite());
        let projected: Vec<_> = sources
            .iter()
            .map(|source| {
                assert!(source.w.is_finite() && source.h.is_finite());
                let fit = (preview.size.w.max(0.) / (source.w.max(1.) * zoom))
                    .min(preview.size.h.max(0.) / (source.h.max(1.) * zoom))
                    .clamp(0., 1.);
                (source.upscale(fit * zoom), fit)
            })
            .collect();
        // A very short window cannot expose a taller strip. Reduce all full strips together so
        // they remain ordered and have no gaps. Reserved capacity still uses the original h.
        for (size, _) in projected.iter().take(budget.visible_depth) {
            budget.entry_height = budget.entry_height.min(size.h.max(0.));
        }
        let extent = budget.extent(sources.len());
        let foreground_offset = extent / 2.;
        let entries = projected
            .into_iter()
            .enumerate()
            .map(|(depth, (size, scale))| {
                let pos = preview.loc
                    + Point::from((
                        (preview.size.w - size.w) / 2.,
                        foreground_offset - budget.offset(depth),
                    ));
                let rect = Rectangle::new(pos, size);
                let entrance = (depth < budget.visible_depth && budget.entry_height > 0.)
                    .then(|| Rectangle::new(pos, Size::from((size.w, budget.entry_height))));
                SinkOverviewEntry {
                    rect,
                    entrance,
                    scale,
                }
            })
            .collect();
        Self {
            extent,
            foreground_offset,
            entries,
        }
    }

    pub fn pose(&self, depth: usize, zoom: f64) -> SinkPose {
        let entry = &self.entries[depth];
        SinkPose {
            position: entry.rect.loc.downscale(zoom),
            scale: entry.scale,
            alpha: 1.,
        }
    }
}

/// Bound all four affine rectangle edges, including native chrome and overshoot. Independent
/// edge extrema matter when source sizes differ; checking just endpoint rectangles is unsafe.
pub fn tween_bounds(
    from: Rectangle<f64, Logical>,
    to: Rectangle<f64, Logical>,
    progress: (f64, f64),
    chrome: f64,
    minimum_size: Size<f64, Logical>,
) -> Rectangle<f64, Logical> {
    let range = |a: f64, b: f64| {
        let first = a + (b - a) * progress.0;
        let last = a + (b - a) * progress.1;
        (first.min(last), first.max(last))
    };
    let x = range(from.loc.x, to.loc.x);
    let y = range(from.loc.y, to.loc.y);
    let left = x.0 - chrome;
    let top = y.0 - chrome;
    // SinkPose clamps a negative interpolated scale to 1e-6. Include the largest origin too,
    // otherwise a reversing spring could put the tiny clamped rectangle outside these bounds.
    let right = range(from.loc.x + from.size.w, to.loc.x + to.size.w)
        .1
        .max(x.1 + minimum_size.w)
        + chrome;
    let bottom = range(from.loc.y + from.size.h, to.loc.y + to.size.h)
        .1
        .max(y.1 + minimum_size.h)
        + chrome;
    Rectangle::new((left, top).into(), (right - left, bottom - top).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::{Animation, Clock};
    use approx::assert_abs_diff_eq;
    use proptest::prelude::*;
    use std::time::Duration;

    fn budget() -> SinkOverviewBudget {
        SinkOverviewBudget::new(Default::default(), 2, 1000.)
    }

    #[test]
    fn extent_tail_capacity_and_empty_stack() {
        let budget = budget();
        for (count, expected) in [(0, 0.), (1, 24.), (2, 48.), (3, 54.), (4, 57.)] {
            assert_eq!(budget.extent(count), expected);
        }
        assert_eq!(budget.capacity(), 60.);
        assert_eq!(budget.extent(usize::MAX), 60.);
        assert_eq!(budget.offset(usize::MAX), 60.);
        let geometry =
            SinkOverviewGeometry::new(Rectangle::from_size((640., 360.).into()), 0.5, budget, &[]);
        assert_eq!(geometry.extent, 0.);
        assert_eq!(geometry.foreground_offset, 0.);
        assert!(geometry.entries.is_empty());
    }

    #[test]
    fn configuration_extremes_on_small_outputs() {
        for visible_depth in 1..=4 {
            for h in [1., 24., 256.] {
                for tail in [0., 12., 256.] {
                    for q in [0.01, 0.5, 0.99] {
                        for available in [0., 0.1, 1., 8., 100., 4096.] {
                            let config = niri_config::OverviewSink {
                                entry_height: h,
                                tail_height: tail,
                                tail_decay: q,
                                ..Default::default()
                            };
                            let budget = SinkOverviewBudget::new(config, visible_depth, available);
                            assert!(budget.capacity() <= available + 1e-10);
                            let mut previous = 0.;
                            for count in [0, 1, visible_depth, visible_depth + 1, 100, usize::MAX] {
                                let extent = budget.extent(count);
                                assert!(extent.is_finite() && extent >= previous);
                                assert!(extent <= budget.capacity() + 1e-10);
                                previous = extent;
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn unequal_sources_share_center_and_have_disjoint_full_entries() {
        let sources = [(800., 600.), (1800., 200.), (100., 2000.), (3000., 2400.)].map(Size::from);
        for zoom in [0.0001, 0.25, 0.5, 0.75] {
            for size in [(1., 1.), (40., 30.), (1280., 720.)] {
                let preview = Rectangle::new((13., 19.).into(), Size::from(size).upscale(zoom));
                let budget = SinkOverviewBudget::new(Default::default(), 2, preview.size.h / 2.);
                let geometry = SinkOverviewGeometry::new(preview, zoom, budget, &sources);
                assert!(geometry.extent <= budget.capacity() + 1e-10);
                for (depth, entry) in geometry.entries.iter().enumerate() {
                    assert_abs_diff_eq!(
                        entry.rect.loc.x + entry.rect.size.w / 2.,
                        preview.loc.x + preview.size.w / 2.,
                        epsilon = 1e-9
                    );
                    assert!(entry.rect.size.w <= preview.size.w + 1e-9);
                    assert!(entry.rect.size.h <= preview.size.h + 1e-9);
                    let pose = geometry.pose(depth, zoom);
                    let roundtrip = pose.rectangle(sources[depth]);
                    assert_abs_diff_eq!(roundtrip.loc.x * zoom, entry.rect.loc.x, epsilon = 1e-9);
                    assert_abs_diff_eq!(roundtrip.size.h * zoom, entry.rect.size.h, epsilon = 1e-9);
                    assert_eq!(entry.entrance.is_some(), depth < 2);
                }
                let top = geometry.entries[0].entrance.unwrap();
                let second = geometry.entries[1].entrance.unwrap();
                assert_abs_diff_eq!(second.loc.y + second.size.h, top.loc.y, epsilon = 1e-9);
                assert_abs_diff_eq!(
                    top.loc.y + top.size.h,
                    preview.loc.y + geometry.foreground_offset,
                    epsilon = 1e-9
                );
            }
        }
    }

    #[test]
    fn very_short_source_limits_full_strips_without_affecting_capacity() {
        let preview = Rectangle::from_size((640., 360.).into());
        let geometry = SinkOverviewGeometry::new(
            preview,
            0.5,
            budget(),
            &[(800., 600.).into(), (800., 2.).into()],
        );
        assert_eq!(geometry.extent, 2.);
        assert_eq!(geometry.foreground_offset, 1.);
        assert_eq!(budget().capacity(), 60.);
        for entry in geometry.entries {
            assert_eq!(entry.entrance.unwrap().size.h, 1.);
        }
    }

    #[test]
    fn logical_entry_height_is_not_scaled_twice_at_fractional_dpi() {
        for zoom in [0.25, 0.5, 0.75] {
            let preview = Rectangle::from_size(Size::from((1280., 720.)).upscale(zoom));
            let geometry =
                SinkOverviewGeometry::new(preview, zoom, budget(), &[(800., 600.).into(); 2]);
            for dpi in [1., 1.25, 1.5, 2.] {
                let entry = geometry.entries[0].entrance.unwrap();
                assert_eq!(entry.size.h * dpi, 24. * dpi);
                assert_abs_diff_eq!(
                    (geometry.pose(1, zoom).position.y - geometry.pose(0, zoom).position.y) * zoom,
                    -24.,
                    epsilon = 1e-9
                );
            }
        }
    }

    #[test]
    fn actual_spring_reversal_and_gesture_velocity_stay_inside_reserved_bounds() {
        for damping in [0.01, 0.6, 1., 3.] {
            for velocity in [-40., 0., 40.] {
                for (from, to) in [(0., 1.), (1., 0.), (0.4, 1.), (1.2, 0.)] {
                    let config = niri_config::Animation {
                        off: false,
                        kind: niri_config::animations::Kind::Spring(
                            niri_config::animations::SpringParams {
                                damping_ratio: damping,
                                stiffness: 800,
                                epsilon: 0.001,
                            },
                        ),
                    };
                    let anim = Animation::new(Clock::default(), from, to, velocity, config);
                    let range = anim.value_range();
                    let desktop = Rectangle::new((100., 30.).into(), (300., 180.).into());
                    let expanded = Rectangle::new((50., -24.).into(), (400., 240.).into());
                    let bounds = tween_bounds(desktop, expanded, range, 18., Size::default());
                    for millis in (0..2000).step_by(3) {
                        let p = anim.value_at(anim.start_time() + Duration::from_millis(millis));
                        assert!(range.0 <= p && p <= range.1, "{range:?}, p={p}");
                        let loc = desktop.loc + (expanded.loc - desktop.loc).upscale(p);
                        let size = desktop.size + (expanded.size - desktop.size).upscale(p);
                        assert!(bounds.loc.x <= loc.x - 18. + 1e-8);
                        assert!(bounds.loc.y <= loc.y - 18. + 1e-8);
                        assert!(loc.x + size.w + 18. <= bounds.loc.x + bounds.size.w + 1e-8);
                        assert!(loc.y + size.h + 18. <= bounds.loc.y + bounds.size.h + 1e-8);
                    }
                }
            }
        }
    }

    #[test]
    fn custom_easing_overshoot_and_asymmetric_chrome_overhang() {
        for (from, to) in [(0., 1.), (1., 0.)] {
            let config = niri_config::Animation {
                off: false,
                kind: niri_config::animations::Kind::Easing(
                    niri_config::animations::EasingParams {
                        duration_ms: 1000,
                        curve: niri_config::animations::Curve::CubicBezier(0.2, -4., 0.8, 5.),
                    },
                ),
            };
            let anim = Animation::new(Clock::default(), from, to, 0., config);
            let range = anim.value_range();
            assert_eq!(range, (-4., 5.));
            for millis in 0..=1000 {
                let p = anim.value_at(anim.start_time() + Duration::from_millis(millis));
                assert!(range.0 <= p && p <= range.1);
            }
        }
        let preview = Rectangle::from_size((100., 60.).into());
        let rect = Rectangle::new((-10., -20.).into(), (150., 100.).into());
        let mut overhang = Overhang::default();
        overhang.include(preview, rect);
        assert_eq!(
            overhang,
            Overhang {
                top: 20.,
                bottom: 20.,
                left: 10.,
                right: 40.
            }
        );
    }

    #[test]
    fn negative_scale_overshoot_reserves_the_actual_clamped_pose() {
        let source = Size::from((800., 600.));
        let from = SinkPose {
            position: (100., 40.).into(),
            scale: 0.2,
            alpha: 0.5,
        };
        let to = SinkPose {
            position: (20., -30.).into(),
            scale: 1.,
            alpha: 1.,
        };
        let bounds = tween_bounds(
            from.rectangle(source),
            to.rectangle(source),
            (-3., 2.),
            0.,
            source.upscale(1e-6),
        );
        for i in 0..=1000 {
            let pose = from.interpolate(to, -3. + i as f64 * 0.005);
            let rect = pose.rectangle(source);
            assert!(rect.loc.x >= bounds.loc.x - 1e-9);
            assert!(rect.loc.y >= bounds.loc.y - 1e-9);
            assert!(rect.loc.x + rect.size.w <= bounds.loc.x + bounds.size.w + 1e-9);
            assert!(rect.loc.y + rect.size.h <= bounds.loc.y + bounds.size.h + 1e-9);
        }
    }

    proptest! {
        #[test]
        fn varied_sources_fit_reserved_capacity_and_entries(
            output in (1f64..4096., 1f64..2160.),
            zoom in 0.0001f64..0.75,
            sources in proptest::collection::vec((1f64..8192., 1f64..8192.), 0..128),
            depth in 1usize..=4,
            h in 1f64..=256.,
            tail in 0f64..=256.,
            decay in 0.01f64..=0.99,
            available_fraction in 0f64..1.,
            low in -2f64..0.,
            high in 1f64..3.,
        ) {
            let config = niri_config::OverviewSink { entry_height: h, tail_height: tail, tail_decay: decay, ..Default::default() };
            let preview = Rectangle::from_size(Size::from(output).upscale(zoom));
            let budget = SinkOverviewBudget::new(config, depth, preview.size.h * available_fraction);
            let sources: Vec<_> = sources.into_iter().map(Size::from).collect();
            let geometry = SinkOverviewGeometry::new(preview, zoom, budget, &sources);
            prop_assert!(geometry.extent <= budget.capacity() + 1e-8);
            let reserved = budget.reserve(preview.size, (low, high), 4.);
            for (index, entry) in geometry.entries.iter().enumerate() {
                // Exercise a range of desktop scales and positions inside the native preview.
                let source = sources[index];
                let from = super::super::sink_presentation::desktop_pose(source, Rectangle::from_size(Size::from(output)), index, Default::default()).rectangle(source);
                let from = Rectangle::new(from.loc.upscale(zoom), from.size.upscale(zoom));
                let bounds = tween_bounds(from, entry.rect, (low, high), 4., source.upscale(zoom * 1e-6));
                prop_assert!(bounds.loc.x >= -reserved.left - 1e-8);
                prop_assert!(bounds.loc.y >= -reserved.top - 1e-8);
                prop_assert!(bounds.loc.x + bounds.size.w <= preview.size.w + reserved.right + 1e-8);
                prop_assert!(bounds.loc.y + bounds.size.h <= preview.size.h + reserved.bottom + 1e-8);
                if let Some(entrance) = entry.entrance {
                    prop_assert!(entrance.size.h <= entry.rect.size.h + 1e-8);
                    prop_assert!(entrance.loc.y + entrance.size.h <= geometry.foreground_offset + 1e-8);
                    if index != 0 {
                        prop_assert!(entrance.loc.y + entrance.size.h <= geometry.entries[index - 1].rect.loc.y + 1e-8);
                    }
                }
            }
        }
    }
}
