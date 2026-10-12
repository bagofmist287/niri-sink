use core::f64;
use std::rc::Rc;

use niri_config::utils::MergeWith as _;
use niri_config::{Color, CornerRadius, GradientInterpolation};
use niri_ipc::WindowLayout;
use smithay::backend::renderer::element::{Element, Kind};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Logical, Point, Rectangle, Scale, Size};

use super::focus_ring::{FocusRing, FocusRingRenderElement};
use super::opening_window::{OpenAnimation, OpeningWindowRenderElement};
use super::shadow::Shadow;
use super::shrink_expand::{NormalSizePlan, ShrinkAxis, SpineGeometry, WindowForm};
use super::sink_presentation::{SinkPose, SinkPresentation};
use super::{
    HitType, LayoutElement, LayoutElementRenderElement, LayoutElementRenderSnapshot, Options,
    SizeFrac, RESIZE_ANIMATION_THRESHOLD,
};
use crate::animation::{Animation, Clock};
use crate::layout::SizingMode;
use crate::niri_render_elements;
use crate::render_helpers::background_effect::BackgroundEffectElement;
use crate::render_helpers::border::BorderRenderElement;
use crate::render_helpers::clipped_surface::{ClippedSurfaceRenderElement, RoundedCornerDamage};
use crate::render_helpers::damage::ExtraDamage;
use crate::render_helpers::offscreen::{OffscreenBuffer, OffscreenRenderElement};
use crate::render_helpers::projected_group::{ProjectedGroup, ProjectionBuffer};
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::resize::ResizeRenderElement;
use crate::render_helpers::shaders::{ProgramType, Shaders};
use crate::render_helpers::shadow::ShadowRenderElement;
use crate::render_helpers::snapshot::RenderSnapshot;
use crate::render_helpers::solid_color::{SolidColorBuffer, SolidColorRenderElement};
use crate::render_helpers::xray::{Xray, XrayPos};
use crate::render_helpers::{RenderCtx, RenderTarget};
use crate::ui::shrink_preview::ShrinkPreview;
use crate::ui::spine::{Spine, SpineRenderElement};
use crate::utils::transaction::Transaction;
use crate::utils::{
    baba_is_float_offset, round_logical_in_physical, round_logical_in_physical_max1, ResizeEdge,
};

/// Toplevel window with decorations.
#[derive(Debug)]
pub struct Tile<W: LayoutElement> {
    pub(super) sink_generation: u64,
    pub(super) sink_focus_cancelled: bool,
    pub(super) sink_presentation: Option<SinkPresentation>,
    /// Local expanded pose, relative to the workspace origin before its common F displacement.
    /// It owns no source and does not drive the Overview opening/closing clock.
    pub(super) sink_overview_presentation: Option<SinkPresentation>,
    projection_buffer: std::cell::RefCell<Option<Rc<ProjectionBuffer>>>,

    /// The toplevel window itself.
    window: W,

    /// Independent outer geometry; the original client remains owned by this Tile.
    spine: Option<SpineGeometry>,
    spine_ui: Spine,
    spine_visual_active: bool,
    shrink_preview: Option<ShrinkPreview>,
    /// Current-slot allocation while the original client catches up, never a restore history.
    normal_restore: Option<NormalSizePlan>,
    /// Container origin at batch insertion, retained through a subsequent pending mode change.
    normal_restore_origin_mode: SizingMode,
    normal_restore_deadline: Option<std::time::Instant>,
    pub(super) normal_restore_focus_cancelled: bool,

    /// The border around the window.
    border: FocusRing,

    /// The focus ring around the window.
    focus_ring: FocusRing,

    /// The shadow around the window.
    shadow: Shadow,

    /// This tile's current sizing mode.
    ///
    /// This will update only when the `window` actually goes maximized or fullscreen, rather than
    /// right away, to avoid black backdrop flicker before the window has had a chance to resize.
    sizing_mode: SizingMode,

    /// The black backdrop for fullscreen windows.
    fullscreen_backdrop: SolidColorBuffer,

    /// Whether the tile should float upon unfullscreening.
    pub(super) restore_to_floating: bool,

    /// The size that the window should assume when going floating.
    ///
    /// This is generally the last size the window had when it was floating. It can be unknown if
    /// the window starts out in the tiling layout or fullscreen.
    pub(super) floating_window_size: Option<Size<i32, Logical>>,

    /// The position that the tile should assume when going floating, relative to the floating
    /// space working area.
    ///
    /// This is generally the last position the tile had when it was floating. It can be unknown if
    /// the window starts out in the tiling layout.
    pub(super) floating_pos: Option<Point<f64, SizeFrac>>,

    /// Currently selected preset width index when this tile is floating.
    pub(super) floating_preset_width_idx: Option<usize>,

    /// Currently selected preset height index when this tile is floating.
    pub(super) floating_preset_height_idx: Option<usize>,

    /// The animation upon opening a window.
    open_animation: Option<OpenAnimation>,

    /// The animation of the window resizing.
    resize_animation: Option<ResizeAnimation>,

    /// The animation of a tile visually moving horizontally.
    move_x_animation: Option<MoveAnimation>,

    /// The animation of a tile visually moving vertically.
    move_y_animation: Option<MoveAnimation>,

    /// The animation of the tile's opacity.
    pub(super) alpha_animation: Option<AlphaAnimation>,

    /// Offset during the initial interactive move rubberband.
    pub(super) interactive_move_offset: Point<f64, Logical>,

    /// Snapshot of the last render for use in the close animation.
    unmap_snapshot: Option<TileRenderSnapshot>,

    /// Extra damage for clipped surface corner radius changes.
    rounded_corner_damage: RoundedCornerDamage,

    /// The view size for the tile's workspace.
    ///
    /// Used as the fullscreen target size.
    view_size: Size<f64, Logical>,

    /// Scale of the output the tile is on (and rounds its sizes to).
    scale: f64,

    /// Clock for driving animations.
    pub(super) clock: Clock,

    /// Configurable properties of the layout.
    pub(super) options: Rc<Options>,
}

niri_render_elements! {
    TileRenderElement<R> => {
        LayoutElement = LayoutElementRenderElement<R>,
        FocusRing = FocusRingRenderElement,
        SolidColor = SolidColorRenderElement,
        Opening = OpeningWindowRenderElement,
        Resize = ResizeRenderElement,
        Border = BorderRenderElement,
        Shadow = ShadowRenderElement,
        ClippedSurface = ClippedSurfaceRenderElement<R>,
        Offscreen = OffscreenRenderElement,
        ExtraDamage = ExtraDamage,
        BackgroundEffect = BackgroundEffectElement,
        Spine = SpineRenderElement,
    }
}

pub type TileRenderSnapshot =
    RenderSnapshot<TileRenderElement<GlesRenderer>, TileRenderElement<GlesRenderer>>;

#[derive(Debug)]
struct ResizeAnimation {
    anim: Animation,
    size_from: Size<f64, Logical>,
    snapshot: LayoutElementRenderSnapshot,
    offscreen: OffscreenBuffer,
    tile_size_from: Size<f64, Logical>,
    // If the resize involved the fullscreen state at some point, this is the progress toward the
    // fullscreen state. Used for things like fullscreen backdrop alpha.
    //
    // Note that this can be set even if this specific resize is between two non-fullscreen states,
    // for example when issuing a new resize during an unfullscreen resize.
    fullscreen_progress: Option<Animation>,
    // Similar to above but for fullscreen-or-maximized.
    expanded_progress: Option<Animation>,
}

#[derive(Debug)]
struct MoveAnimation {
    anim: Animation,
    from: f64,
    /// Whether this animation is for moving the tile between workspaces.
    ///
    /// Controls whether the tile is rendered uncropped and above others.
    is_between_workspaces: bool,
}

#[derive(Debug)]
pub(super) struct AlphaAnimation {
    pub(super) anim: Animation,
    /// Whether the animation should persist after it's done.
    ///
    /// This is used by things like interactive move which need to animate alpha to
    /// semitransparent, then hold it at semitransparent for a while, until the operation
    /// completes.
    pub(super) hold_after_done: bool,
    offscreen: OffscreenBuffer,
}

impl<W: LayoutElement> Tile<W> {
    pub fn new(
        window: W,
        view_size: Size<f64, Logical>,
        scale: f64,
        clock: Clock,
        options: Rc<Options>,
    ) -> Self {
        let rules = window.rules();
        let border_config = options.layout.border.merged_with(&rules.border);
        let focus_ring_config = options.layout.focus_ring.merged_with(&rules.focus_ring);
        let shadow_config = options.layout.shadow.merged_with(&rules.shadow);
        let sizing_mode = window.sizing_mode();

        Self {
            sink_generation: 0,
            sink_focus_cancelled: false,
            sink_presentation: None,
            sink_overview_presentation: None,
            projection_buffer: Default::default(),
            window,
            spine: None,
            spine_ui: Spine::default(),
            spine_visual_active: false,
            shrink_preview: None,
            normal_restore: None,
            normal_restore_origin_mode: SizingMode::Normal,
            normal_restore_deadline: None,
            normal_restore_focus_cancelled: false,
            border: FocusRing::new(border_config.into()),
            focus_ring: FocusRing::new(focus_ring_config),
            shadow: Shadow::new(shadow_config),
            sizing_mode,
            fullscreen_backdrop: SolidColorBuffer::new((0., 0.), [0., 0., 0., 1.]),
            restore_to_floating: false,
            floating_window_size: None,
            floating_pos: None,
            floating_preset_width_idx: None,
            floating_preset_height_idx: None,
            open_animation: None,
            resize_animation: None,
            move_x_animation: None,
            move_y_animation: None,
            alpha_animation: None,
            interactive_move_offset: Point::from((0., 0.)),
            unmap_snapshot: None,
            rounded_corner_damage: Default::default(),
            view_size,
            scale,
            clock,
            options,
        }
    }

    pub fn update_config(
        &mut self,
        view_size: Size<f64, Logical>,
        scale: f64,
        options: Rc<Options>,
    ) {
        if self.view_size != view_size && self.is_sink_restoring() {
            let source = self.animated_tile_size();
            self.sink_presentation
                .as_mut()
                .unwrap()
                .rebase_view(self.view_size, view_size, source);
            self.invalidate_sink_restore_context();
        }
        if self.options.sink.enabled && !options.sink.enabled {
            self.sink_generation = self.sink_generation.wrapping_add(1);
            self.sink_focus_cancelled = false;
            self.sink_presentation = None;
            self.sink_overview_presentation = None;
            self.release_sink_projection();
            self.window.finish_sink_restore();
        }
        // If preset widths or heights changed, clear our stored preset index.
        if self.options.layout.preset_column_widths != options.layout.preset_column_widths {
            self.floating_preset_width_idx = None;
        }
        if self.options.layout.preset_window_heights != options.layout.preset_window_heights {
            self.floating_preset_height_idx = None;
        }

        let thickness_changed =
            self.options.shrink_expand.thickness != options.shrink_expand.thickness;
        if self.is_spine() && !Rc::ptr_eq(&self.options, &options) {
            self.spine_ui.reload_icons();
        }
        self.view_size = view_size;
        self.scale = scale;
        self.options = options;

        self.update_decoration_config();

        self.window.update_config(self.options.blur);

        let border = self.effective_border_width().unwrap_or(0.);
        if let Some(spine) = &mut self.spine {
            if thickness_changed {
                // Keep the current slot and axis; reload is not a new Shrink operation.
                if let Some(updated) = SpineGeometry::new(
                    spine.axis,
                    self.options.shrink_expand.thickness,
                    spine.outer_size(),
                    self.scale,
                    border,
                ) {
                    *spine = updated;
                }
            } else {
                spine.allocate(spine.outer_size(), self.scale, border);
            }
        }
    }

    fn update_decoration_config(&mut self) {
        let round_max1 = |logical| round_logical_in_physical_max1(self.scale, logical);

        let rules = self.window.rules();

        let mut border_config = self.options.layout.border.merged_with(&rules.border);
        border_config.width = round_max1(border_config.width);
        self.border.update_config(border_config.into());

        let mut focus_ring_config = self
            .options
            .layout
            .focus_ring
            .merged_with(&rules.focus_ring);
        focus_ring_config.width = round_max1(focus_ring_config.width);
        self.focus_ring.update_config(focus_ring_config);

        let shadow_config = self.options.layout.shadow.merged_with(&rules.shadow);
        self.shadow.update_config(shadow_config);
    }

    pub fn update_shaders(&mut self) {
        self.spine_ui.update_shaders();
        self.border.update_shaders();
        self.focus_ring.update_shaders();
        self.shadow.update_shaders();
    }

    pub fn update_window(&mut self) {
        let prev_sizing_mode = self.sizing_mode;
        self.sizing_mode = self.window.sizing_mode();

        if let Some(plan) = self.normal_restore {
            self.window.take_animation_snapshot();
            if !self.window.normal_restore_ready() || self.sizing_mode != plan.sizing_mode {
                return;
            }
            self.normal_restore = None;
            self.normal_restore_deadline = None;
            self.window.finish_normal_restore();
        }

        if self.is_spine() {
            // Hidden client commits must not animate or resize the independent outer geometry.
            self.window.take_animation_snapshot();
            self.update_decoration_config();
            let border = self.effective_border_width().unwrap_or(0.);
            let spine = self.spine.as_mut().unwrap();
            spine.allocate(spine.outer_size(), self.scale, border);
            return;
        }

        if let Some(animate_from) = self.window.take_animation_snapshot() {
            let params = if let Some(resize) = self.resize_animation.take() {
                // Compute like in animated_window_size(), but using the snapshot geometry (since
                // the current one is already overwritten).
                let mut size = animate_from.size;

                let val = resize.anim.value();
                let size_from = resize.size_from;
                let tile_size_from = resize.tile_size_from;

                size.w = size_from.w + (size.w - size_from.w) * val;
                size.h = size_from.h + (size.h - size_from.h) * val;

                let mut tile_size = animate_from.size;
                if prev_sizing_mode.is_fullscreen() {
                    tile_size.w = f64::max(tile_size.w, self.view_size.w);
                    tile_size.h = f64::max(tile_size.h, self.view_size.h);
                } else if prev_sizing_mode.is_normal() && !self.border.is_off() {
                    let width = self.border.width();
                    tile_size.w += width * 2.;
                    tile_size.h += width * 2.;
                }

                tile_size.w = tile_size_from.w + (tile_size.w - tile_size_from.w) * val;
                tile_size.h = tile_size_from.h + (tile_size.h - tile_size_from.h) * val;

                let fullscreen_from = resize
                    .fullscreen_progress
                    .map(|anim| anim.clamped_value().clamp(0., 1.))
                    .unwrap_or(if prev_sizing_mode.is_fullscreen() {
                        1.
                    } else {
                        0.
                    });

                let expanded_from = resize
                    .expanded_progress
                    .map(|anim| anim.clamped_value().clamp(0., 1.))
                    .unwrap_or(if prev_sizing_mode.is_normal() { 0. } else { 1. });

                // Also try to reuse the existing offscreen buffer if we have one.
                (
                    size,
                    tile_size,
                    fullscreen_from,
                    expanded_from,
                    resize.offscreen,
                )
            } else {
                let size = animate_from.size;

                // Compute like in tile_size().
                let mut tile_size = size;
                if prev_sizing_mode.is_fullscreen() {
                    tile_size.w = f64::max(tile_size.w, self.view_size.w);
                    tile_size.h = f64::max(tile_size.h, self.view_size.h);
                } else if prev_sizing_mode.is_normal() && !self.border.is_off() {
                    let width = self.border.width();
                    tile_size.w += width * 2.;
                    tile_size.h += width * 2.;
                }

                let fullscreen_from = if prev_sizing_mode.is_fullscreen() {
                    1.
                } else {
                    0.
                };

                let expanded_from = if prev_sizing_mode.is_normal() { 0. } else { 1. };

                (
                    size,
                    tile_size,
                    fullscreen_from,
                    expanded_from,
                    OffscreenBuffer::default(),
                )
            };
            let (size_from, tile_size_from, fullscreen_from, expanded_from, offscreen) = params;

            let change = self.window.size().to_f64().to_point() - size_from.to_point();
            let change = f64::max(change.x.abs(), change.y.abs());
            let tile_change = self.tile_size().to_f64().to_point() - tile_size_from.to_point();
            let tile_change = f64::max(tile_change.x.abs(), tile_change.y.abs());
            let change = f64::max(change, tile_change);
            if change > RESIZE_ANIMATION_THRESHOLD {
                let anim = Animation::new(
                    self.clock.clone(),
                    0.,
                    1.,
                    0.,
                    self.options.animations.window_resize.anim,
                );

                let fullscreen_to = if self.sizing_mode.is_fullscreen() {
                    1.
                } else {
                    0.
                };
                let expanded_to = if self.sizing_mode.is_normal() { 0. } else { 1. };
                let fullscreen_progress = (fullscreen_from != fullscreen_to)
                    .then(|| anim.restarted(fullscreen_from, fullscreen_to, 0.));
                let expanded_progress = (expanded_from != expanded_to)
                    .then(|| anim.restarted(expanded_from, expanded_to, 0.));

                self.resize_animation = Some(ResizeAnimation {
                    anim,
                    size_from,
                    snapshot: animate_from,
                    offscreen,
                    tile_size_from,
                    fullscreen_progress,
                    expanded_progress,
                });
            } else {
                self.resize_animation = None;
            }
        }

        self.update_decoration_config();

        let window_size = self.window_size();
        let radius = self
            .window
            .geometry_corner_radius()
            .fit_to(window_size.w as f32, window_size.h as f32);
        self.rounded_corner_damage.set_corner_radius(radius);
    }

    pub fn advance_animations(&mut self) {
        if let Some(preview) = &mut self.shrink_preview {
            preview.advance();
        }
        if let Some(presentation) = &mut self.sink_overview_presentation {
            presentation.advance();
        }
        if let Some(presentation) = &mut self.sink_presentation {
            presentation.advance();
        }
        if let Some(open) = &mut self.open_animation {
            if open.is_done() {
                self.open_animation = None;
            }
        }

        if let Some(resize) = &mut self.resize_animation {
            if resize.anim.is_done() {
                self.resize_animation = None;
            }
        }

        if let Some(move_) = &mut self.move_x_animation {
            if move_.anim.is_done() {
                self.move_x_animation = None;
            }
        }
        if let Some(move_) = &mut self.move_y_animation {
            if move_.anim.is_done() {
                self.move_y_animation = None;
            }
        }

        if let Some(alpha) = &mut self.alpha_animation {
            if !alpha.hold_after_done && alpha.anim.is_done() {
                self.alpha_animation = None;
            }
        }
    }

    pub fn is_sink_restoring(&self) -> bool {
        self.sink_presentation.as_ref().is_some_and(|p| p.restoring)
    }

    /// Local Overview coordinates and projection buffers belong to the old output context.
    pub(super) fn invalidate_sink_restore_context(&mut self) {
        if self.is_sink_restoring() {
            self.sink_overview_presentation = None;
            self.release_sink_projection();
        }
    }
    /// A restoring tile changes workspace coordinates, but keeps its animation and commit state.
    pub(super) fn translate_sink_restore(
        &mut self,
        delta: Point<f64, Logical>,
        foreground_delta: f64,
    ) {
        if let Some(presentation) = self.sink_presentation.as_mut().filter(|p| p.restoring) {
            presentation.translate(delta);
            if let Some(presentation) = &mut self.sink_overview_presentation {
                presentation.translate(delta + Point::from((0., foreground_delta)));
            }
        }
    }

    pub fn cancel_sink_focus(&mut self) {
        self.sink_focus_cancelled = true;
    }

    pub fn sink_generation(&self) -> u64 {
        self.sink_generation
    }

    pub fn sink_input_blocked(&self) -> bool {
        self.window.sink_parent_input_blocked()
            || self
                .sink_presentation
                .as_ref()
                .is_some_and(|p| p.restoring && (!p.is_done() || !self.window.sink_input_ready()))
            || self
                .sink_overview_presentation
                .as_ref()
                .is_some_and(|p| p.restoring && !p.is_done())
    }

    pub(super) fn release_sink_projection(&self) {
        self.projection_buffer.borrow_mut().take();
    }

    /// Native decoration extent used to reserve Overview space before the first frame.
    pub(super) fn sink_chrome_padding(&self) -> f64 {
        let shadow = self
            .options
            .layout
            .shadow
            .merged_with(&self.window.rules().shadow);
        let shadow_extent = if shadow.on {
            shadow.softness * 1.5
                + shadow.spread.abs()
                + shadow.offset.x.0.abs().max(shadow.offset.y.0.abs())
        } else {
            0.
        };
        let buffer = self.window.buf_loc();
        shadow_extent
            .max(self.visual_border_width().unwrap_or(0.))
            .max(buffer.x.unsigned_abs().max(buffer.y.unsigned_abs()) as f64)
    }

    pub fn are_animations_ongoing(&self) -> bool {
        self.are_transitions_ongoing() || self.window.rules().baba_is_float == Some(true)
    }

    pub fn are_transitions_ongoing(&self) -> bool {
        self.shrink_preview
            .as_ref()
            .is_some_and(ShrinkPreview::is_animating)
            || self
                .sink_overview_presentation
                .as_ref()
                .is_some_and(|p| !p.is_done())
            || self
                .sink_presentation
                .as_ref()
                .is_some_and(|p| !p.is_done())
            || self.open_animation.is_some()
            || self.resize_animation.is_some()
            || self.move_x_animation.is_some()
            || self.move_y_animation.is_some()
            || self
                .alpha_animation
                .as_ref()
                .is_some_and(|alpha| !alpha.anim.is_done())
    }

    pub fn is_moving_between_workspaces(&self) -> bool {
        self.move_y_animation
            .as_ref()
            .is_some_and(|anim| anim.is_between_workspaces)
            || self
                .move_x_animation
                .as_ref()
                .is_some_and(|anim| anim.is_between_workspaces)
    }

    pub fn update_render_elements(&mut self, is_active: bool, view_rect: Rectangle<f64, Logical>) {
        let is_active = if self.is_spine() {
            self.spine_visual_active || self.interactive_move_offset != Point::from((0., 0.))
        } else {
            is_active
        };
        let rules = self.window.rules();
        let animated_tile_size = self.animated_tile_size();
        let expanded_progress = self.expanded_progress();

        let draw_border_with_background = rules
            .draw_border_with_background
            .unwrap_or_else(|| !self.window.has_ssd());
        let draw_border_with_background = draw_border_with_background && !self.is_spine();
        let border_width = self.visual_border_width().unwrap_or(0.);

        // Do the inverse of tile_size() in order to handle the unfullscreen animation for windows
        // that were smaller than the fullscreen size, and therefore their animated_window_size() is
        // currently much smaller than the tile size.
        let mut border_window_size = animated_tile_size;
        border_window_size.w -= border_width * 2.;
        border_window_size.h -= border_width * 2.;

        // FIXME: this takes into account the animation from normal sizing mode to
        // maximized/fullscreen, but it doesn't take into account the corner radius animation from
        // the window itself.
        //
        // Currently, an easy way to see the problem is to start from a window with a nonzero
        // radius, then go from windowed fullscreen (that forces 0 radius) to regular fullscreen.
        // At the start of the animation, windowed fullscreen becomes false, but the window hasn't
        // animated to the normal fullscreen yet, so the radius here jumps to its nonzero value,
        // even though it should remain zero throughout.
        //
        // Later, when windows get the surface shape protocol with radii, this issue will happen
        // when that changes between animated commits.
        let radius = self
            .window
            .geometry_corner_radius()
            .expanded_by(border_width as f32)
            .scaled_by(1. - expanded_progress as f32);
        self.border.update_render_elements(
            border_window_size,
            is_active,
            !draw_border_with_background,
            self.window.is_urgent(),
            Rectangle::new(
                view_rect.loc - Point::from((border_width, border_width)),
                view_rect.size,
            ),
            radius,
            self.scale,
            1. - expanded_progress as f32,
        );

        let radius = if self.visual_border_width().is_some() {
            radius
        } else {
            self.window
                .geometry_corner_radius()
                .scaled_by(1. - expanded_progress as f32)
        };
        self.shadow.update_render_elements(
            animated_tile_size,
            is_active,
            radius,
            self.scale,
            1. - expanded_progress as f32,
        );

        let draw_focus_ring_with_background = if self.border.is_off() {
            draw_border_with_background && !self.is_spine()
        } else {
            false
        };
        let radius = radius.expanded_by(self.focus_ring.width() as f32);
        self.focus_ring.update_render_elements(
            animated_tile_size,
            is_active,
            !draw_focus_ring_with_background,
            self.window.is_urgent(),
            view_rect,
            radius,
            self.scale,
            1. - expanded_progress as f32,
        );

        self.fullscreen_backdrop.resize(animated_tile_size);
        if let Some(preview) = &mut self.shrink_preview {
            let border = self.options.layout.border.merged_with(&rules.border);
            let ring = self
                .options
                .layout
                .focus_ring
                .merged_with(&rules.focus_ring);
            let gradient =
                self.options
                    .layout
                    .spine
                    .resolve(false, self.window.is_urgent(), ring, border);
            preview.update(gradient, self.window.geometry_corner_radius(), self.scale);
        }
        if let Some(spine) = self.spine {
            let border = self.options.layout.border.merged_with(&rules.border);
            let ring = self
                .options
                .layout
                .focus_ring
                .merged_with(&rules.focus_ring);
            let paint = self.options.layout.spine;
            self.spine_ui.update(
                animated_tile_size,
                border_width,
                spine.axis,
                self.scale,
                paint.resolve(is_active, self.window.is_urgent(), ring, border),
                view_rect,
                radius.expanded_by(-self.focus_ring.width() as f32),
                paint.text_color,
            );
        }
    }

    pub fn scale(&self) -> f64 {
        self.scale
    }

    pub fn render_offset(&self) -> Point<f64, Logical> {
        let mut offset = Point::from((0., 0.));

        if let Some(move_) = &self.move_x_animation {
            offset.x += move_.from * move_.anim.value();
        }
        if let Some(move_) = &self.move_y_animation {
            offset.y += move_.from * move_.anim.value();
        }

        offset += self.interactive_move_offset;

        offset
    }

    pub fn start_open_animation(&mut self) {
        self.open_animation = Some(OpenAnimation::new(Animation::new(
            self.clock.clone(),
            0.,
            1.,
            0.,
            self.options.animations.window_open.anim,
        )));
    }

    pub fn resize_animation(&self) -> Option<&Animation> {
        self.resize_animation.as_ref().map(|resize| &resize.anim)
    }

    pub fn animate_move_from(&mut self, from: Point<f64, Logical>) {
        self.animate_move_from_with_config(from, self.options.animations.window_movement.0);
    }

    pub fn animate_move_from_with_config(
        &mut self,
        from: Point<f64, Logical>,
        config: niri_config::Animation,
    ) {
        self.animate_move_x_from_with_config(from.x, config);
        self.animate_move_y_from_with_config(from.y, config);
    }

    pub fn animate_move_x_from(&mut self, from: f64) {
        self.animate_move_x_from_with_config(from, self.options.animations.window_movement.0);
    }

    pub fn animate_move_x_from_with_config(&mut self, from: f64, config: niri_config::Animation) {
        let current_offset = self.render_offset().x;

        // Preserve the previous config if ongoing.
        let move_ = self.move_x_animation.take();
        let current_between = move_
            .as_ref()
            .is_some_and(|move_| move_.is_between_workspaces);
        let anim = move_.map(|move_| move_.anim);
        let anim = anim
            .map(|anim| anim.restarted(1., 0., 0.))
            .unwrap_or_else(|| Animation::new(self.clock.clone(), 1., 0., 0., config));

        self.move_x_animation = Some(MoveAnimation {
            anim,
            from: from + current_offset,
            is_between_workspaces: current_between,
        });
    }

    pub fn animate_move_y_from(&mut self, from: f64) {
        self.animate_move_y_from_with_config(from, self.options.animations.window_movement.0);
    }

    pub fn animate_move_y_from_with_config(&mut self, from: f64, config: niri_config::Animation) {
        let current_offset = self.render_offset().y;

        // Preserve the previous config if ongoing.
        let move_ = self.move_y_animation.take();
        let current_between = move_
            .as_ref()
            .is_some_and(|move_| move_.is_between_workspaces);
        let anim = move_.map(|move_| move_.anim);
        let anim = anim
            .map(|anim| anim.restarted(1., 0., 0.))
            .unwrap_or_else(|| Animation::new(self.clock.clone(), 1., 0., 0., config));

        self.move_y_animation = Some(MoveAnimation {
            anim,
            from: from + current_offset,
            is_between_workspaces: current_between,
        });
    }

    pub fn offset_move_y_anim_current(&mut self, offset: f64) {
        if let Some(move_) = self.move_y_animation.as_mut() {
            // If the anim is almost done, there's little point trying to offset it; we can let
            // things jump. If it turns out like a bad idea, we could restart the anim instead.
            let value = move_.anim.value();
            if value > 0.001 {
                move_.from += offset / value;
            }
        }
    }

    pub fn stop_move_animations(&mut self) {
        self.move_x_animation = None;
        self.move_y_animation = None;
    }

    pub fn set_anim_y_between_workspaces(&mut self) {
        if let Some(anim) = &mut self.move_y_animation {
            anim.is_between_workspaces = true;
        }
    }

    pub fn animate_alpha(&mut self, from: f64, to: f64, config: niri_config::Animation) {
        let from = from.clamp(0., 1.);
        let to = to.clamp(0., 1.);

        let (current, offscreen) = if let Some(alpha) = self.alpha_animation.take() {
            (alpha.anim.clamped_value(), alpha.offscreen)
        } else {
            (from, OffscreenBuffer::default())
        };

        self.alpha_animation = Some(AlphaAnimation {
            anim: Animation::new(self.clock.clone(), current, to, 0., config),
            hold_after_done: false,
            offscreen,
        });
    }

    pub fn ensure_alpha_animates_to_1(&mut self) {
        if let Some(alpha) = &self.alpha_animation {
            if alpha.anim.to() != 1. {
                // Cancel animation instead of starting a new one because the user likely wants to
                // see the tile right away.
                self.alpha_animation = None;
            }
        }
    }

    pub fn hold_alpha_animation_after_done(&mut self) {
        if let Some(alpha) = &mut self.alpha_animation {
            alpha.hold_after_done = true;
        }
    }

    pub fn window(&self) -> &W {
        &self.window
    }

    pub fn window_mut(&mut self) -> &mut W {
        &mut self.window
    }

    pub fn form(&self) -> WindowForm {
        self.spine
            .map_or(WindowForm::Normal, |spine| WindowForm::Spine(spine.axis))
    }

    pub fn is_spine(&self) -> bool {
        self.spine.is_some()
    }

    /// Internal form conversion. The operation layer must validate scope and hand off focus.
    /// No client configure is sent and no normal-size restore record is created.
    pub fn set_spine(&mut self, axis: ShrinkAxis, thickness: f64) -> bool {
        let Some(spine) = self.plan_spine_geometry(axis, thickness) else {
            return false;
        };
        self.spine = Some(spine);
        self.shrink_preview = None;
        self.spine_ui.clear();
        self.spine_visual_active = false;
        self.unmap_snapshot = None;
        self.normal_restore = None;
        self.normal_restore_deadline = None;
        self.window.finish_normal_restore();
        self.resize_animation = None;
        true
    }

    pub fn plan_spine_geometry(&self, axis: ShrinkAxis, thickness: f64) -> Option<SpineGeometry> {
        if !self.sizing_mode.is_normal()
            || !self.window.pending_sizing_mode().is_normal()
            || self.window.is_windowed_fullscreen()
            || self.window.is_pending_windowed_fullscreen()
            || self.sink_presentation.is_some()
            || self.is_normal_restoring()
        {
            return None;
        }
        let allocation = self.tile_expected_or_current_size();
        SpineGeometry::new(
            axis,
            thickness,
            allocation,
            self.scale,
            self.effective_border_width().unwrap_or(0.),
        )
    }

    /// Size allocation after this must come from the current native slot.
    pub fn clear_spine(&mut self) -> bool {
        let cleared = self.spine.take().is_some();
        if cleared {
            self.spine_ui.clear();
            self.spine_visual_active = false;
            self.unmap_snapshot = None;
        }
        cleared
    }

    pub fn set_spine_visual_active(&mut self, active: bool) {
        self.spine_visual_active = self.is_spine() && active;
    }

    pub(crate) fn set_shrink_preview(&mut self, target: Size<f64, Logical>, animate: bool) -> bool {
        let source = self.animated_tile_size();
        let preview = self
            .shrink_preview
            .get_or_insert_with(|| ShrinkPreview::new(self.clock.clone(), source));
        preview.retarget(
            target,
            self.options.animations.window_movement.0,
            animate && !self.options.animations.off,
        )
    }

    pub(crate) fn clear_shrink_preview(&mut self) {
        self.shrink_preview = None;
    }

    pub(crate) fn set_shrink_preview_anchor(&mut self, anchor: Point<f64, Logical>) {
        if let Some(preview) = &mut self.shrink_preview {
            preview.set_anchor(anchor);
        }
    }

    pub fn shrink_preview_pose(&self) -> Option<Size<f64, Logical>> {
        self.shrink_preview.as_ref().map(ShrinkPreview::pose)
    }

    /// Batch form update, before any member is configured. The caller owns the slot snapshot.
    pub(super) fn prepare_normal_restore(&mut self, plan: NormalSizePlan) -> bool {
        if self.spine.take().is_none() {
            return false;
        }
        self.spine_ui.clear();
        self.spine_visual_active = false;
        self.unmap_snapshot = None;
        self.normal_restore = Some(plan);
        self.normal_restore_origin_mode = plan.sizing_mode;
        self.normal_restore_focus_cancelled = true;
        self.resize_animation = None;
        true
    }

    pub(super) fn submit_normal_restore(
        &mut self,
        floating: bool,
        transaction: Transaction,
        settle_matching: bool,
    ) {
        let Some(plan) = self.normal_restore else {
            return;
        };
        let deadline = transaction.deadline();
        self.normal_restore_deadline = Some(deadline);
        if floating {
            self.window
                .request_restore_size_once(plan.client_size, transaction.clone());
        } else {
            self.window.request_size(
                plan.client_size,
                plan.sizing_mode,
                false,
                Some(transaction.clone()),
            );
        }
        self.window.begin_normal_restore(deadline);
        if settle_matching
            && self.window.normal_restore_ready()
            && self.sizing_mode == plan.sizing_mode
        {
            self.window
                .release_matching_normal_restore_transaction(&transaction);
            self.update_window();
        }
    }

    /// Recheck the final intent before a configure is published, retaining the batch deadline.
    pub(super) fn rearm_normal_restore(&mut self) {
        let Some(deadline) = self.normal_restore_deadline else {
            return;
        };
        self.window.begin_normal_restore(deadline);
    }

    pub fn normal_restore_input_blocked(&self) -> bool {
        self.normal_restore.is_some() && !self.window.normal_restore_ready()
    }

    pub fn client_input_blocked(&self) -> bool {
        self.sink_input_blocked() || self.normal_restore_input_blocked()
    }

    pub fn allow_normal_restore_focus(&mut self) {
        self.normal_restore_focus_cancelled = false;
    }

    pub(crate) fn cancel_normal_restore_focus(&mut self) {
        self.normal_restore_focus_cancelled = true;
    }

    pub fn is_normal_restoring(&self) -> bool {
        self.normal_restore_input_blocked()
    }

    pub(super) fn has_normal_restore_allocation(&self) -> bool {
        self.normal_restore.is_some()
    }

    pub fn sizing_mode(&self) -> SizingMode {
        if self.normal_restore.is_none() {
            return self.sizing_mode;
        }
        if self.sizing_mode.is_fullscreen() || self.normal_restore_origin_mode.is_fullscreen() {
            SizingMode::Fullscreen
        } else if self.sizing_mode.is_maximized() || self.normal_restore_origin_mode.is_maximized()
        {
            SizingMode::Maximized
        } else {
            SizingMode::Normal
        }
    }

    fn fullscreen_progress(&self) -> f64 {
        if let Some(resize) = &self.resize_animation {
            if let Some(anim) = &resize.fullscreen_progress {
                return anim.clamped_value().clamp(0., 1.);
            }
        }

        if self.sizing_mode.is_fullscreen() {
            1.
        } else {
            0.
        }
    }

    fn expanded_progress(&self) -> f64 {
        if let Some(resize) = &self.resize_animation {
            if let Some(anim) = &resize.expanded_progress {
                return anim.clamped_value().clamp(0., 1.);
            }
        }

        if self.sizing_mode.is_normal() {
            0.
        } else {
            1.
        }
    }

    /// Returns `None` if the border is hidden and `Some(width)` if it should be shown.
    pub fn effective_border_width(&self) -> Option<f64> {
        if !self.sizing_mode.is_normal() {
            return None;
        }

        if self.border.is_off() {
            return None;
        }

        Some(self.border.width())
    }

    fn visual_border_width(&self) -> Option<f64> {
        if self.border.is_off() {
            return None;
        }

        let expanded_progress = self.expanded_progress();

        // Only hide the border when fully expanded to avoid jarring border appearance.
        if expanded_progress == 1. {
            return None;
        }

        // FIXME: would be cool to, like, gradually resize the border from full width to 0 during
        // fullscreening, but the rest of the code isn't quite ready for that yet. It needs to
        // handle things like computing intermediate tile size when an animated resize starts during
        // an animated unfullscreen resize.
        Some(self.border.width())
    }

    /// Returns the location of the window's visual geometry within this Tile.
    pub fn window_loc(&self) -> Point<f64, Logical> {
        let mut loc = Point::from((0., 0.));

        let window_size = self.animated_window_size();
        let target_size = self.animated_tile_size();

        // Center the window within its tile.
        //
        // - Without borders, the sizes match, so this difference is zero.
        // - Borders always match from all sides, so this difference is pre-rounded to physical.
        // - In fullscreen, if the window is smaller than the tile, then it gets centered, otherwise
        //   the tile size matches the window.
        // - During animations, the window remains centered within the tile; this is important for
        //   the to/from fullscreen animation.
        loc.x += (target_size.w - window_size.w) / 2.;
        loc.y += (target_size.h - window_size.h) / 2.;

        // Round to physical pixels.
        loc = loc
            .to_physical_precise_round(self.scale)
            .to_logical(self.scale);

        loc
    }

    /// Native moving anchors the visible geometry, which is independent of a spine's client.
    pub(super) fn move_anchor_geometry(&self) -> Rectangle<f64, Logical> {
        if self.is_spine() {
            Rectangle::from_size(self.tile_size())
        } else {
            Rectangle::new(self.window_loc(), self.window_size())
        }
    }

    pub fn tile_size(&self) -> Size<f64, Logical> {
        if let Some(spine) = &self.spine {
            return spine.outer_size();
        }
        if let Some(plan) = self.normal_restore {
            return plan.allocation;
        }
        let mut size = self.window_size();

        if self.sizing_mode.is_fullscreen() {
            // Normally we'd just return the fullscreen size here, but this makes things a bit
            // nicer if a fullscreen window is bigger than the fullscreen size for some reason.
            size.w = f64::max(size.w, self.view_size.w);
            size.h = f64::max(size.h, self.view_size.h);
            return size;
        }

        if let Some(width) = self.effective_border_width() {
            size.w += width * 2.;
            size.h += width * 2.;
        }

        size
    }

    pub fn tile_expected_or_current_size(&self) -> Size<f64, Logical> {
        if let Some(spine) = &self.spine {
            return spine.outer_size();
        }
        if let Some(plan) = self.normal_restore {
            return plan.allocation;
        }
        let mut size = self.window_expected_or_current_size();

        if self.sizing_mode.is_fullscreen() {
            // Normally we'd just return the fullscreen size here, but this makes things a bit
            // nicer if a fullscreen window is bigger than the fullscreen size for some reason.
            size.w = f64::max(size.w, self.view_size.w);
            size.h = f64::max(size.h, self.view_size.h);
            return size;
        }

        if let Some(width) = self.effective_border_width() {
            size.w += width * 2.;
            size.h += width * 2.;
        }

        size
    }

    pub fn window_size(&self) -> Size<f64, Logical> {
        let mut size = self.window.size().to_f64();
        size = size
            .to_physical_precise_round(self.scale)
            .to_logical(self.scale);
        size
    }

    pub fn window_expected_or_current_size(&self) -> Size<f64, Logical> {
        let size = self.window.expected_size();
        let mut size = size.unwrap_or_else(|| self.window.size()).to_f64();
        size = size
            .to_physical_precise_round(self.scale)
            .to_logical(self.scale);
        size
    }

    pub fn animated_window_size(&self) -> Size<f64, Logical> {
        let mut size = self.window_size();

        if let Some(resize) = &self.resize_animation {
            let val = resize.anim.value();
            let size_from = resize.size_from.to_f64();

            size.w = f64::max(1., size_from.w + (size.w - size_from.w) * val);
            size.h = f64::max(1., size_from.h + (size.h - size_from.h) * val);
            size = size
                .to_physical_precise_round(self.scale)
                .to_logical(self.scale);
        }

        size
    }

    pub fn animated_tile_size(&self) -> Size<f64, Logical> {
        if let Some(spine) = &self.spine {
            return spine.outer_size();
        }
        if let Some(plan) = self.normal_restore {
            return plan.allocation;
        }
        let mut size = self.tile_size();

        if let Some(resize) = &self.resize_animation {
            let val = resize.anim.value();
            let size_from = resize.tile_size_from.to_f64();

            size.w = f64::max(1., size_from.w + (size.w - size_from.w) * val);
            size.h = f64::max(1., size_from.h + (size.h - size_from.h) * val);
            size = size
                .to_physical_precise_round(self.scale)
                .to_logical(self.scale);
        }

        size
    }

    pub fn buf_loc(&self) -> Point<f64, Logical> {
        let mut loc = Point::from((0., 0.));
        loc += self.window_loc();
        loc += self.window.buf_loc().to_f64();
        loc
    }

    /// Returns a partially-filled [`WindowLayout`].
    ///
    /// Only the sizing properties that a [`Tile`] can fill are filled.
    pub fn ipc_layout_template(&self) -> WindowLayout {
        WindowLayout {
            pos_in_scrolling_layout: None,
            tile_size: self.tile_size().into(),
            window_size: self.window().size().into(),
            tile_pos_in_workspace_view: None,
            window_offset_in_tile: self.window_loc().into(),
        }
    }

    fn is_in_input_region(&self, mut point: Point<f64, Logical>) -> bool {
        point -= self.window_loc().to_f64();
        self.window.is_in_input_region(point)
    }

    fn is_in_activation_region(&self, point: Point<f64, Logical>) -> bool {
        let activation_region = Rectangle::from_size(self.tile_size());
        activation_region.contains(point)
    }

    pub fn hit(&self, point: Point<f64, Logical>) -> Option<HitType> {
        let offset = self.bob_offset();
        let point = point - offset;
        if self.is_spine() {
            let size = self.tile_size();
            if !Rectangle::from_size(size).contains(point) {
                return None;
            }
            let band_x = 4_f64.min(size.w / 3.);
            let band_y = 4_f64.min(size.h / 3.);
            let mut edges = ResizeEdge::empty();
            if point.x < band_x {
                edges |= ResizeEdge::LEFT;
            } else if point.x >= size.w - band_x {
                edges |= ResizeEdge::RIGHT;
            }
            if point.y < band_y {
                edges |= ResizeEdge::TOP;
            } else if point.y >= size.h - band_y {
                edges |= ResizeEdge::BOTTOM;
            }
            return Some(HitType::Spine { edges });
        }

        if self.is_in_input_region(point) {
            let win_pos = self.buf_loc() + offset;
            Some(HitType::Input { win_pos })
        } else if self.is_in_activation_region(point) {
            Some(HitType::Activate {
                is_tab_indicator: false,
            })
        } else {
            None
        }
    }

    pub fn request_tile_size(
        &mut self,
        size: Size<f64, Logical>,
        animate: bool,
        transaction: Option<Transaction>,
    ) {
        let border = self.effective_border_width().unwrap_or(0.);
        if let Some(spine) = &mut self.spine {
            spine.allocate(size, self.scale, border);
            return;
        }
        let size = self.normal_window_size_for_tile_size(size);
        let allocation = self.normal_tile_size_for_window_size(size);
        let changed_restore = self
            .normal_restore
            .as_ref()
            .is_some_and(|plan| plan.client_size != size || plan.sizing_mode != SizingMode::Normal);
        if let Some(plan) = &mut self.normal_restore {
            *plan = NormalSizePlan {
                allocation,
                client_size: size,
                sizing_mode: SizingMode::Normal,
            };
        }

        self.window
            .request_size(size, SizingMode::Normal, animate, transaction);
        if changed_restore {
            self.rearm_normal_restore();
        }
    }

    pub(super) fn request_normal_resize_once(&mut self, size: Size<i32, Logical>, animate: bool) {
        if self.normal_restore.is_none() {
            self.window.request_size_once(size, animate);
            return;
        }
        let changed_restore = self
            .normal_restore
            .as_ref()
            .is_some_and(|plan| plan.client_size != size);
        // Zero is a client-choice request, never the held normal preview's geometry.
        let current = self.window.size();
        let estimate = (
            if size.w == 0 { current.w } else { size.w },
            if size.h == 0 { current.h } else { size.h },
        )
            .into();
        let allocation = self.normal_tile_size_for_window_size(estimate);
        if let Some(plan) = &mut self.normal_restore {
            plan.client_size = size;
            plan.allocation = allocation;
        }
        self.window.request_size_once(size, animate);
        if changed_restore {
            self.rearm_normal_restore();
        }
    }

    /// Convert a normal allocation without consulting the current spine form.
    pub fn normal_window_size_for_tile_size(
        &self,
        mut size: Size<f64, Logical>,
    ) -> Size<i32, Logical> {
        // Can't go through effective_border_width() because we might be fullscreen.
        if !self.border.is_off() {
            let width = self.border.width();
            size.w = f64::max(1., size.w - width * 2.);
            size.h = f64::max(1., size.h - width * 2.);
        }

        // The size request has to be i32 unfortunately, due to Wayland. We floor here instead of
        // round to avoid situations where proportionally-sized columns don't fit on the screen
        // exactly.
        size.to_i32_floor()
    }

    pub fn normal_tile_size_for_window_size(&self, size: Size<i32, Logical>) -> Size<f64, Logical> {
        let mut size = size
            .to_f64()
            .to_physical_precise_round(self.scale)
            .to_logical(self.scale);
        if !self.border.is_off() {
            size += Size::from((self.border.width() * 2., self.border.width() * 2.));
        }
        size
    }

    pub fn tile_width_for_window_width(&self, size: f64) -> f64 {
        if self.is_spine() {
            return size;
        }
        if self.border.is_off() {
            size
        } else {
            size + self.border.width() * 2.
        }
    }

    pub fn tile_height_for_window_height(&self, size: f64) -> f64 {
        if self.is_spine() {
            return size;
        }
        if self.border.is_off() {
            size
        } else {
            size + self.border.width() * 2.
        }
    }

    pub fn window_width_for_tile_width(&self, size: f64) -> f64 {
        if self.is_spine() {
            return size;
        }
        if self.border.is_off() {
            size
        } else {
            size - self.border.width() * 2.
        }
    }

    pub fn window_height_for_tile_height(&self, size: f64) -> f64 {
        if self.is_spine() {
            return size;
        }
        if self.border.is_off() {
            size
        } else {
            size - self.border.width() * 2.
        }
    }

    pub fn request_maximized(
        &mut self,
        size: Size<f64, Logical>,
        animate: bool,
        transaction: Option<Transaction>,
    ) {
        if let Some(plan) = &mut self.normal_restore {
            *plan = NormalSizePlan {
                allocation: size,
                client_size: size.to_i32_round(),
                sizing_mode: SizingMode::Maximized,
            };
        }
        self.window.request_size(
            size.to_i32_round(),
            SizingMode::Maximized,
            animate,
            transaction,
        );
    }

    pub fn request_fullscreen(&mut self, animate: bool, transaction: Option<Transaction>) {
        if let Some(plan) = &mut self.normal_restore {
            *plan = NormalSizePlan {
                allocation: self.view_size,
                client_size: self.view_size.to_i32_round(),
                sizing_mode: SizingMode::Fullscreen,
            };
        }
        self.window.request_size(
            self.view_size.to_i32_round(),
            SizingMode::Fullscreen,
            animate,
            transaction,
        );
    }

    pub fn min_size_nonfullscreen(&self) -> Size<f64, Logical> {
        if let Some(spine) = &self.spine {
            return spine.min_size(self.scale, self.effective_border_width().unwrap_or(0.));
        }
        self.normal_min_size_nonfullscreen()
    }

    pub fn normal_min_size_nonfullscreen(&self) -> Size<f64, Logical> {
        let mut size = self.window.min_size().to_f64();

        // Can't go through effective_border_width() because we might be fullscreen.
        if !self.border.is_off() {
            let width = self.border.width();

            size.w = f64::max(1., size.w);
            size.h = f64::max(1., size.h);

            size.w += width * 2.;
            size.h += width * 2.;
        }

        size
    }

    pub fn max_size_nonfullscreen(&self) -> Size<f64, Logical> {
        if let Some(spine) = &self.spine {
            return spine.max_size(self.scale, self.effective_border_width().unwrap_or(0.));
        }
        self.normal_max_size_nonfullscreen()
    }

    pub fn normal_max_size_nonfullscreen(&self) -> Size<f64, Logical> {
        let mut size = self.window.max_size().to_f64();

        // Can't go through effective_border_width() because we might be fullscreen.
        if !self.border.is_off() {
            let width = self.border.width();

            if size.w > 0. {
                size.w += width * 2.;
            }
            if size.h > 0. {
                size.h += width * 2.;
            }
        }

        size
    }

    pub fn bob_offset(&self) -> Point<f64, Logical> {
        if self.window.rules().baba_is_float != Some(true) {
            return Point::from((0., 0.));
        }

        let y = baba_is_float_offset(self.clock.now(), self.view_size.h);
        let y = round_logical_in_physical(self.scale, y);
        Point::from((0., y))
    }

    fn render_inner<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        location: Point<f64, Logical>,
        mut xray_pos: XrayPos,
        focus_ring: bool,
        push: &mut dyn FnMut(TileRenderElement<R>),
    ) {
        let _span = tracy_client::span!("Tile::render_inner");

        let scale = Scale::from(self.scale);
        let fullscreen_progress = self.fullscreen_progress();
        let expanded_progress = self.expanded_progress();

        let win_alpha = if self.window.is_ignoring_opacity_window_rule() {
            1.
        } else {
            let alpha = self.window.rules().opacity.unwrap_or(1.).clamp(0., 1.);

            // Interpolate towards alpha = 1. at fullscreen.
            let p = fullscreen_progress as f32;
            alpha * (1. - p) + 1. * p
        };

        // This is here rather than in render_offset() because render_offset() is currently assumed
        // by the code to be temporary. So, for example, interactive move will try to "grab" the
        // tile at its current render offset and reset the render offset to zero by cancelling the
        // tile move animations. On the other hand, bob_offset() is not resettable, so adding it in
        // render_offset() would cause obvious animation glitches.
        //
        // This isn't to say that adding it here is perfect; indeed, it kind of breaks view_rect
        // passed to update_render_elements(). But, it works well enough for what it is.
        let bob_offset = self.bob_offset();
        let location = location + bob_offset;
        xray_pos = xray_pos.offset(bob_offset);

        if self.is_spine() {
            let (title, app_id) = self.window.spine_caption();
            let blocked = ctx
                .target
                .should_block_out(self.window.rules().block_out_from);
            self.spine_ui.render(
                ctx.r(),
                location,
                win_alpha,
                blocked,
                &title,
                &app_id,
                &mut |elem| push(elem.into()),
            );
            if let Some(width) = self.visual_border_width() {
                self.border.render(
                    ctx.renderer,
                    location + Point::from((width, width)),
                    &mut |elem| push(elem.into()),
                );
            }
            if focus_ring {
                self.focus_ring
                    .render(ctx.renderer, location, &mut |elem| push(elem.into()));
            }
            self.shadow
                .render(ctx.renderer, location, &mut |elem| push(elem.into()));
            return;
        }

        let window_loc = self.window_loc();
        let window_size = self.window_size();
        let animated_window_size = self.animated_window_size();
        let window_render_loc = location + window_loc;
        let area = Rectangle::new(window_render_loc, animated_window_size);
        xray_pos = xray_pos.offset(window_loc);

        let rules = self.window.rules();

        // Clip to geometry including during the fullscreen animation to help with buggy clients
        // that submit a full-sized buffer before acking the fullscreen state (Firefox).
        let clip_to_geometry = fullscreen_progress < 1. && rules.clip_to_geometry == Some(true);
        let radius = self
            .window
            .geometry_corner_radius()
            .scaled_by(1. - expanded_progress as f32);

        // Popups go on top, whether it's resize or not.
        self.window.render_popups(
            ctx.r(),
            window_render_loc,
            scale,
            win_alpha,
            xray_pos,
            &mut |elem| push(elem.into()),
        );

        // If we're resizing, try to render a shader, or a fallback.
        let mut pushed_resize = false;
        if let Some(resize) = &self.resize_animation {
            if ResizeRenderElement::has_shader(ctx.renderer) {
                let mut ctx = ctx.as_gles();

                if let Some(texture_from) = resize.snapshot.texture(ctx.r(), scale) {
                    let mut window_elements = Vec::new();
                    self.window.render_normal(
                        ctx.r(),
                        Point::from((0., 0.)),
                        scale,
                        1.,
                        &mut |elem| window_elements.push(elem),
                    );

                    let current = resize
                        .offscreen
                        .render(ctx.renderer, scale, &window_elements)
                        .map_err(|err| warn!("error rendering window to texture: {err:?}"))
                        .ok();

                    // Clip blocked-out resizes unconditionally because they use solid color render
                    // elements.
                    let clip_to_geometry =
                        if ctx.target.should_block_out(resize.snapshot.block_out_from)
                            && ctx.target.should_block_out(rules.block_out_from)
                        {
                            true
                        } else {
                            clip_to_geometry
                        };

                    if let Some((elem_current, _sync_point, mut data)) = current {
                        let texture_current = elem_current.texture().clone();
                        // The offset and size are computed in physical pixels and converted to
                        // logical with the same `scale`, so converting them back with rounding
                        // inside the geometry() call gives us the same physical result back.
                        let texture_current_geo = elem_current.geometry(scale);

                        let elem = ResizeRenderElement::new(
                            area,
                            scale,
                            texture_from.clone(),
                            resize.snapshot.size,
                            (texture_current, texture_current_geo),
                            window_size,
                            resize.anim.value() as f32,
                            resize.anim.clamped_value().clamp(0., 1.) as f32,
                            radius,
                            clip_to_geometry,
                            win_alpha,
                        );

                        // We're drawing the resize shader, not the offscreen directly.
                        data.id = elem.id().clone();

                        // This is not a problem for split popups as the code will look for them by
                        // original id when it doesn't find them on the offscreen.
                        self.window.set_offscreen_data(Some(data));
                        push(elem.into());
                        pushed_resize = true;
                    }
                }
            }

            if !pushed_resize {
                let fallback_buffer = SolidColorBuffer::new(area.size, [1., 0., 0., 1.]);
                let elem = SolidColorRenderElement::from_buffer(
                    &fallback_buffer,
                    area.loc,
                    win_alpha,
                    Kind::Unspecified,
                );
                push(elem.into());
                pushed_resize = true;
            }
        }

        // If we're not resizing, render the window itself.
        let has_border_shader = BorderRenderElement::has_shader(ctx.renderer);
        if !pushed_resize {
            let geo = Rectangle::new(window_render_loc, window_size);
            let radius = radius.fit_to(window_size.w as f32, window_size.h as f32);

            let clip_shader = ClippedSurfaceRenderElement::shader(ctx.renderer).cloned();
            let clip = |elem| match elem {
                LayoutElementRenderElement::Wayland(elem) => {
                    // If we should clip to geometry, render a clipped window.
                    if clip_to_geometry {
                        if let Some(shader) = clip_shader.clone() {
                            if ClippedSurfaceRenderElement::will_clip(&elem, scale, geo, radius) {
                                return ClippedSurfaceRenderElement::new(
                                    elem,
                                    scale,
                                    geo,
                                    shader.clone(),
                                    radius,
                                )
                                .into();
                            }
                        }
                    }

                    // Otherwise, render it normally.
                    LayoutElementRenderElement::Wayland(elem).into()
                }
                LayoutElementRenderElement::SolidColor(elem) => {
                    // In this branch we're rendering a blocked-out window with a solid
                    // color. We need to render it with a rounded corner shader even if
                    // clip_to_geometry is false, because in this case we're assuming that
                    // the unclipped window CSD already has corners rounded to the
                    // user-provided radius, so our blocked-out rendering should match that
                    // radius.
                    if radius != CornerRadius::default() && has_border_shader {
                        return BorderRenderElement::new(
                            geo.size,
                            Rectangle::from_size(geo.size),
                            GradientInterpolation::default(),
                            Color::from_color32f(elem.color()),
                            Color::from_color32f(elem.color()),
                            0.,
                            Rectangle::from_size(geo.size),
                            0.,
                            radius,
                            scale.x as f32,
                            1.,
                        )
                        .with_location(geo.loc)
                        .into();
                    }

                    // Otherwise, render the solid color as is.
                    LayoutElementRenderElement::SolidColor(elem).into()
                }
                elem @ LayoutElementRenderElement::BackgroundEffect(_) => {
                    // This is only used on popups for now. If subsurface blur is implemented, this
                    // will need to be handled somehow.
                    error!("background effect clipping is unimplemented");
                    elem.into()
                }
            };

            if clip_to_geometry && clip_shader.is_some() {
                let damage = self.rounded_corner_damage.render(geo);
                push(damage.into());
            }

            self.window
                .render_normal(ctx.r(), window_render_loc, scale, win_alpha, &mut |elem| {
                    push(clip(elem))
                });
        }

        if fullscreen_progress > 0. {
            let alpha = fullscreen_progress as f32;

            // During the un/fullscreen animation, render a border element in order to use the
            // animated corner radius.
            if fullscreen_progress < 1. && has_border_shader {
                let border_width = self.visual_border_width().unwrap_or(0.);
                let radius = self
                    .window
                    .geometry_corner_radius()
                    .expanded_by(border_width as f32)
                    .scaled_by(1. - expanded_progress as f32);

                let size = self.fullscreen_backdrop.size();
                let color = self.fullscreen_backdrop.color();
                let elem = BorderRenderElement::new(
                    size,
                    Rectangle::from_size(size),
                    GradientInterpolation::default(),
                    Color::from_color32f(color),
                    Color::from_color32f(color),
                    0.,
                    Rectangle::from_size(size),
                    0.,
                    radius,
                    scale.x as f32,
                    alpha,
                )
                .with_location(location);
                push(elem.into());
            } else {
                let elem = SolidColorRenderElement::from_buffer(
                    &self.fullscreen_backdrop,
                    location,
                    alpha,
                    Kind::Unspecified,
                );
                push(elem.into());
            }
        }

        if let Some(width) = self.visual_border_width() {
            self.border.render(
                ctx.renderer,
                location + Point::from((width, width)),
                &mut |elem| push(elem.into()),
            );
        }

        // Hide the focus ring when maximized/fullscreened. It's not normally visible anyway due to
        // being outside the monitor or obscured by a solid colored bar, but it is visible under
        // semitransparent bars in maximized state (which is a bit weird) and in the overview (also
        // a bit weird).
        if focus_ring && expanded_progress < 1. {
            self.focus_ring
                .render(ctx.renderer, location, &mut |elem| push(elem.into()));
        }

        if expanded_progress < 1. {
            self.shadow
                .render(ctx.renderer, location, &mut |elem| push(elem.into()));
        }

        let surface_anim_scale = animated_window_size / window_size;
        self.window.render_background_effect(
            ctx.as_gles(),
            area,
            self.scale,
            clip_to_geometry,
            surface_anim_scale,
            radius,
            xray_pos,
            &mut |elem| push(elem.into()),
        );
    }

    pub fn render<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        location: Point<f64, Logical>,
        xray_pos: XrayPos,
        focus_ring: bool,
        push: &mut dyn FnMut(TileRenderElement<R>),
    ) {
        let _span = tracy_client::span!("Tile::render");
        // The workspace projects this same owner exactly once in a separate Z pass.
        if self.sink_presentation.is_some() {
            return;
        }

        let scale = Scale::from(self.scale);

        let tile_alpha = self
            .alpha_animation
            .as_ref()
            .map_or(1., |alpha| alpha.anim.clamped_value()) as f32;

        let mut pushed = false;
        // Capture passes must not replace the output's presentation-feedback mapping.
        // render_inner may also write this slot while rendering a native resize animation.
        let capture_slot = (self.shrink_preview.is_some() && ctx.target != RenderTarget::Output)
            .then(|| self.window.offscreen_data_slot())
            .flatten();
        let output_data = capture_slot
            .as_ref()
            .and_then(|slot| slot.borrow_mut().take());
        self.window().set_offscreen_data(None);

        if let Some(preview) = &self.shrink_preview {
            let mut ctx = ctx.as_gles();
            let mut elements = Vec::new();
            self.render_inner(
                ctx.r(),
                Point::new(0., 0.),
                xray_pos,
                focus_ring,
                &mut |e| elements.push(e),
            );
            if let Ok((elem, _sync, data)) =
                preview
                    .offscreen(ctx.target)
                    .render(ctx.renderer, scale, &elements)
            {
                let blocked = ctx
                    .target
                    .should_block_out(self.window.rules().block_out_from);
                let alpha = if self.window.is_ignoring_opacity_window_rule() {
                    1.
                } else {
                    self.window.rules().opacity.unwrap_or(1.).clamp(0., 1.)
                };
                preview.render(
                    ctx.renderer,
                    location + self.bob_offset() + preview.offset(self.animated_tile_size()),
                    blocked,
                    alpha * tile_alpha,
                    &mut |e| {
                        // Ghost is above the gray source in front-to-back element ordering.
                        push(e.into());
                    },
                );
                let offset = elem.offset();
                if let Some(slot) = &capture_slot {
                    *slot.borrow_mut() = output_data;
                } else {
                    self.window().set_offscreen_data(Some(data));
                }
                push(
                    elem.with_alpha(tile_alpha)
                        .with_offset(location + offset)
                        .with_grayscale()
                        .into(),
                );
                return;
            }
        }

        if let Some(open) = &self.open_animation {
            let mut ctx = ctx.as_gles();
            let mut elements = Vec::new();
            self.render_inner(
                ctx.r(),
                Point::new(0., 0.),
                xray_pos,
                focus_ring,
                &mut |elem| elements.push(elem),
            );
            match open.render(
                ctx.renderer,
                &elements,
                self.animated_tile_size(),
                location,
                scale,
                tile_alpha,
            ) {
                Ok((elem, data)) => {
                    self.window().set_offscreen_data(Some(data));
                    push(elem.into());
                    pushed = true;
                }
                Err(err) => {
                    warn!("error rendering window opening animation: {err:?}");
                }
            }
        } else if let Some(alpha) = &self.alpha_animation {
            let mut ctx = ctx.as_gles();
            let mut elements = Vec::new();
            self.render_inner(
                ctx.r(),
                Point::new(0., 0.),
                xray_pos,
                focus_ring,
                &mut |elem| elements.push(elem),
            );
            match alpha.offscreen.render(ctx.renderer, scale, &elements) {
                Ok((elem, _sync, data)) => {
                    let offset = elem.offset();
                    let elem = elem.with_alpha(tile_alpha).with_offset(location + offset);

                    self.window().set_offscreen_data(Some(data));
                    push(elem.into());
                    pushed = true;
                }
                Err(err) => {
                    warn!("error rendering tile to offscreen for alpha animation: {err:?}");
                }
            }
        }

        if !pushed {
            self.render_inner(ctx, location, xray_pos, focus_ring, &mut |elem| push(elem));
        }
        if let Some(slot) = &capture_slot {
            *slot.borrow_mut() = output_data;
        }
    }

    pub(super) fn render_projected<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        pose: SinkPose,
        xray_pos: XrayPos,
        focus_ring: bool,
    ) -> Option<ProjectedGroup> {
        let mut pose = pose;
        pose.position = pose
            .position
            .to_physical_precise_round(self.scale)
            .to_logical(self.scale);
        if pose.alpha <= 0. {
            return None;
        }
        let open = self
            .open_animation
            .as_ref()
            .map(OpenAnimation::presentation);
        let mut xray_pos = XrayPos {
            pos_in_backdrop: (xray_pos.pos_in_backdrop + pose.position).downscale(pose.scale),
            zoom: xray_pos.zoom * pose.scale,
        };
        let mut ctx = ctx.as_gles();
        let custom_open = Shaders::get(ctx.renderer)
            .program(ProgramType::Open)
            .is_some();
        if let Some(open) = open.as_ref().filter(|_| !custom_open) {
            let factor = open.scale_factor();
            let center = self.animated_tile_size().to_point().downscale(2.);
            xray_pos.pos_in_backdrop =
                (xray_pos.pos_in_backdrop + center.upscale(1. - factor)).downscale(factor);
            xray_pos.zoom *= factor;
        }
        let slot = self.window.offscreen_data_slot();
        let previous_data = slot.as_ref().and_then(|slot| slot.borrow_mut().take());
        let mut elements = Vec::new();
        self.render_inner(
            ctx.r(),
            Point::from((0., 0.)),
            xray_pos,
            focus_ring,
            &mut |element| elements.push(element),
        );
        let native_data = slot.as_ref().and_then(|slot| slot.borrow_mut().take());
        if let Some(slot) = &slot {
            *slot.borrow_mut() = previous_data;
        }
        if elements.is_empty() {
            return None;
        }
        let buffer = self
            .projection_buffer
            .borrow_mut()
            .get_or_insert_with(|| Rc::new(ProjectionBuffer::default()))
            .clone();
        let alpha = self
            .alpha_animation
            .as_ref()
            .map_or(1., |alpha| alpha.anim.clamped_value()) as f32;
        Some(ProjectedGroup::new(
            buffer,
            elements,
            pose.position,
            pose.scale,
            pose.alpha as f32,
            self.scale,
            ctx.target,
            slot,
            native_data,
            open,
            self.animated_tile_size(),
            alpha,
            custom_open,
        ))
    }

    pub fn store_unmap_snapshot_if_empty(
        &mut self,
        renderer: &mut GlesRenderer,
        xray: Option<&mut Xray>,
        xray_has_blocked_out_layers: bool,
        xray_pos: XrayPos,
    ) {
        if self.unmap_snapshot.is_some() {
            return;
        }

        self.unmap_snapshot =
            Some(self.render_snapshot(renderer, xray, xray_has_blocked_out_layers, xray_pos));
    }

    fn render_snapshot(
        &self,
        renderer: &mut GlesRenderer,
        mut xray: Option<&mut Xray>,
        xray_has_blocked_out_layers: bool,
        xray_pos: XrayPos,
    ) -> TileRenderSnapshot {
        let _span = tracy_client::span!("Tile::render_snapshot");

        let mut contents = Vec::new();
        self.render(
            RenderCtx {
                target: RenderTarget::Output,
                renderer,
                xray: xray.as_deref(),
            },
            Point::from((0., 0.)),
            xray_pos,
            false,
            &mut |elem| contents.push(elem),
        );

        let mut contents_with_blocked_out_bg = None;

        // Do a bit of pointer surgery on Xray.
        //
        // The idea is to avoid the combinatorial combination of rendering snapshots for target
        // (Output, Screencast) × Xray target (Output, Screencast, ScreenCapture).
        //
        // Our main goals:
        // - Everything must look unblocked for RenderTarget::Output.
        // - If anything is potentially blocked-out, it must not show up on any screen capture.
        //
        // Right above we rendered a fully-unblocked snapshot for the Output, so that's covered.
        //
        // Next, *only if Xray has any blocked-out surfaces* (which is a rare case), we will render
        // a snapshot where the window itself is unblocked, but the Xray background is blocked. To
        // do this, we swap the Output target buffers in Xray with the Screencast target buffers
        // (which were prepared for us higher up the stack).
        //
        // Finally, we render a fully blocked-out snapshot. If Xray has blocked-out surfaces, then
        // Xray's Screencast buffers are already filled-in, but if not, then we swap in the Output
        // buffers, to avoid an extra render. This is safe since we know there are no blocked
        // surfaces there.
        let output_idx = RenderTarget::Output as usize;
        let screencast_idx = RenderTarget::Screencast as usize;
        let mut screencast_background = None;
        let mut screencast_backdrop = None;
        let mut output_background = None;
        let mut output_backdrop = None;
        if let Some(xray) = &mut xray {
            screencast_background = Some(Rc::clone(&xray.background[screencast_idx]));
            screencast_backdrop = Some(Rc::clone(&xray.backdrop[screencast_idx]));
            output_background = Some(Rc::clone(&xray.background[output_idx]));
            output_backdrop = Some(Rc::clone(&xray.backdrop[output_idx]));

            if xray_has_blocked_out_layers {
                xray.background[output_idx] = screencast_background.clone().unwrap();
                xray.backdrop[output_idx] = screencast_backdrop.clone().unwrap();

                let mut contents = Vec::new();
                self.render(
                    RenderCtx {
                        target: RenderTarget::Output,
                        renderer,
                        xray: Some(xray),
                    },
                    Point::from((0., 0.)),
                    xray_pos,
                    false,
                    &mut |elem| contents.push(elem),
                );
                contents_with_blocked_out_bg = Some(contents);
            } else {
                xray.background[screencast_idx] = output_background.clone().unwrap();
                xray.backdrop[screencast_idx] = output_backdrop.clone().unwrap();
            }
        }

        // A bit of a hack to render blocked out as for screencast, but I think it's fine here.
        let mut blocked_out_contents = Vec::new();
        self.render(
            RenderCtx {
                target: RenderTarget::Screencast,
                renderer,
                xray: xray.as_deref(),
            },
            Point::from((0., 0.)),
            xray_pos,
            false,
            &mut |elem| blocked_out_contents.push(elem),
        );

        // Put everything back to normal.
        if let Some(xray) = &mut xray {
            if xray_has_blocked_out_layers {
                xray.background[output_idx] = output_background.take().unwrap();
                xray.backdrop[output_idx] = output_backdrop.take().unwrap();
            } else {
                xray.background[screencast_idx] = screencast_background.take().unwrap();
                xray.backdrop[screencast_idx] = screencast_backdrop.take().unwrap();
            }
        }

        RenderSnapshot {
            contents,
            contents_with_blocked_out_bg,
            blocked_out_contents,
            block_out_from: self.window.rules().block_out_from,
            size: self.animated_tile_size(),
            texture: Default::default(),
            texture_with_blocked_out_bg: Default::default(),
            blocked_out_texture: Default::default(),
        }
    }

    pub fn take_unmap_snapshot(&mut self) -> Option<TileRenderSnapshot> {
        self.unmap_snapshot.take()
    }

    pub fn border(&self) -> &FocusRing {
        &self.border
    }

    pub fn focus_ring(&self) -> &FocusRing {
        &self.focus_ring
    }

    pub fn options(&self) -> &Rc<Options> {
        &self.options
    }

    #[cfg(test)]
    pub fn view_size(&self) -> Size<f64, Logical> {
        self.view_size
    }

    #[cfg(test)]
    pub fn verify_invariants(&self) {
        use approx::assert_abs_diff_eq;

        assert_eq!(self.sizing_mode, self.window.sizing_mode());

        let scale = self.scale;
        let size = self.tile_size();
        let rounded = size.to_physical_precise_round(scale).to_logical(scale);
        assert_abs_diff_eq!(size.w, rounded.w, epsilon = 1e-5);
        assert_abs_diff_eq!(size.h, rounded.h, epsilon = 1e-5);
    }
}
