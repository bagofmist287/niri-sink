//! Sink presentation and commit barrier metadata. None of these objects owns a window.
use crate::animation::{Animation, Clock};
use smithay::utils::{Logical, Point, Rectangle, Serial, Size};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SinkPose {
    pub position: Point<f64, Logical>,
    pub scale: f64,
    pub alpha: f64,
}
impl SinkPose {
    pub fn normal(position: Point<f64, Logical>) -> Self {
        Self {
            position,
            scale: 1.,
            alpha: 1.,
        }
    }
    pub fn interpolate(self, to: Self, progress: f64) -> Self {
        Self {
            position: self.position + (to.position - self.position).upscale(progress),
            scale: (self.scale + (to.scale - self.scale) * progress).max(1e-6),
            alpha: (self.alpha + (to.alpha - self.alpha) * progress).clamp(0., 1.),
        }
    }
    pub fn rectangle(self, source: Size<f64, Logical>) -> Rectangle<f64, Logical> {
        Rectangle::new(self.position, source.upscale(self.scale))
    }
}

pub fn desktop_pose(
    source: Size<f64, Logical>,
    area: Rectangle<f64, Logical>,
    depth: usize,
    config: niri_config::Sink,
) -> SinkPose {
    let fit = (area.size.w / source.w.max(1.))
        .min(area.size.h / source.h.max(1.))
        .clamp(0., 1.);
    let level = config.levels[depth.min(3)];
    let scale = fit * level.scale;
    SinkPose {
        position: area.loc + (area.size - source.upscale(scale)).to_point().downscale(2.),
        scale,
        alpha: if depth < config.visible_depth {
            level.opacity
        } else {
            0.
        },
    }
}

#[derive(Debug)]
pub struct SinkPresentation {
    pub restoring: bool,
    from: SinkPose,
    to: SinkPose,
    animation: Animation,
    finished: bool,
}
impl SinkPresentation {
    pub fn new(
        clock: Clock,
        from: SinkPose,
        to: SinkPose,
        restoring: bool,
        config: niri_config::Animation,
    ) -> Self {
        Self {
            restoring,
            from,
            to,
            animation: Animation::new(clock, 0., 1., 0., config),
            finished: false,
        }
    }
    pub fn pose(&self) -> SinkPose {
        if self.finished {
            self.to
        } else {
            self.from.interpolate(self.to, self.animation.value())
        }
    }
    pub fn pose_toward(&self, to: SinkPose) -> SinkPose {
        if self.finished {
            to
        } else {
            self.from.interpolate(to, self.animation.value())
        }
    }
    pub fn is_done(&self) -> bool {
        self.finished || self.animation.is_done()
    }
    pub fn advance(&mut self) {
        if self.animation.is_done() {
            self.finished = true;
        }
    }

    pub fn retarget(&mut self, to: SinkPose, config: niri_config::Animation) {
        if self.to == to {
            return;
        }
        self.from = self.pose();
        self.finished = false;
        self.to = to;
        self.animation = self.animation.restarted(0., 1., 0.);
        self.animation.replace_config(config);
    }
}

/// Acks only acknowledge a configure. Only a processed surface commit can advance this barrier.
#[derive(Debug, Clone, Copy)]
pub struct SinkInputBarrier {
    pub serial: Option<Serial>,
    pub deadline: Instant,
    committed: bool,
}
impl SinkInputBarrier {
    pub fn new(deadline: Instant) -> Self {
        Self {
            serial: None,
            deadline,
            committed: false,
        }
    }
    pub fn on_configure(&mut self, serial: Serial) {
        if self.serial.is_none() {
            self.serial = Some(serial);
        }
    }
    pub fn on_commit(&mut self, serial: Serial) {
        if self
            .serial
            .is_some_and(|target| serial.is_no_older_than(&target))
        {
            self.committed = true;
        }
    }
    pub fn ready(&self, now: Instant) -> bool {
        self.committed || now >= self.deadline
    }
    pub fn degraded(&self, now: Instant) -> bool {
        !self.committed && now >= self.deadline
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn fit_center_and_horizon_preserve_native_sizes() {
        let area = Rectangle::new((30., 50.).into(), (1000., 600.).into());
        for source in [(400., 200.), (3000., 800.), (100., 2000.)] {
            let source = Size::from(source);
            for depth in 0..8 {
                let pose = desktop_pose(source, area, depth, Default::default());
                let rect = pose.rectangle(source);
                assert_eq!(
                    rect.loc + rect.size.to_point().downscale(2.),
                    area.loc + area.size.to_point().downscale(2.)
                );
                assert!(rect.size.w <= area.size.w && rect.size.h <= area.size.h);
                assert!(pose.scale <= 1.);
                assert_eq!(pose.alpha > 0., depth < 2);
            }
        }
    }
    #[test]
    fn redirection_starts_at_current_pose() {
        let from = SinkPose::normal((10., 20.).into());
        let to = desktop_pose(
            (800., 400.).into(),
            Rectangle::from_size((1000., 600.).into()),
            1,
            Default::default(),
        );
        let current = from.interpolate(to, 0.4);
        assert_eq!(current.interpolate(from, 0.), current);
        assert_eq!(current.interpolate(from, 1.), from);
        assert!((0. ..=1.).contains(&from.interpolate(to, 5.).alpha));
        assert!(to.interpolate(from, -5.).scale > 0.);
    }
    #[test]
    fn only_corresponding_commit_or_original_deadline_releases_input() {
        let now = Instant::now();
        let deadline = now + Duration::from_millis(300);
        let mut barrier = SinkInputBarrier::new(deadline);
        barrier.on_configure(Serial::from(100));
        assert!(!barrier.ready(now));
        barrier.on_commit(Serial::from(99));
        assert!(!barrier.ready(now));
        barrier.on_configure(Serial::from(101));
        assert_eq!(barrier.deadline, deadline);
        assert!(!barrier.ready(deadline - Duration::from_nanos(1)));
        assert!(barrier.ready(deadline));
        assert!(barrier.degraded(deadline));
        barrier.on_commit(Serial::from(102));
        assert!(barrier.ready(now));
        assert!(!barrier.degraded(deadline));
    }
}
