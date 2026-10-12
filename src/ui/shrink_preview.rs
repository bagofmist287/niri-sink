//! A compositor-owned preview; never a layout member or a Wayland window.
use niri_config::{Color, CornerRadius, Gradient};
use smithay::backend::renderer::element::Kind;
use smithay::utils::{Logical, Point, Rectangle, Size};

use crate::animation::{Animation, Clock};
use crate::render_helpers::border::BorderRenderElement;
use crate::render_helpers::offscreen::OffscreenBuffer;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::shaders::{ProgramType, Shaders};
use crate::render_helpers::solid_color::{SolidColorBuffer, SolidColorRenderElement};
use crate::ui::spine::SpineRenderElement;

#[derive(Debug)]
pub struct ShrinkPreview {
    from: Size<f64, Logical>,
    to: Size<f64, Logical>,
    animation: Option<Animation>,
    clock: Clock,
    anchor: Point<f64, Logical>,
    offscreen: [OffscreenBuffer; 3],
    fill: BorderRenderElement,
    fallback: SolidColorBuffer,
    blocked: SolidColorBuffer,
}

impl ShrinkPreview {
    pub fn new(clock: Clock, from: Size<f64, Logical>) -> Self {
        Self {
            from,
            to: from,
            animation: None,
            clock,
            anchor: Point::from((0., 0.)),
            offscreen: std::array::from_fn(|_| OffscreenBuffer::default()),
            fill: BorderRenderElement::empty_with_program(ProgramType::ShrinkGhost),
            fallback: SolidColorBuffer::default(),
            blocked: SolidColorBuffer::default(),
        }
    }

    pub fn offscreen(&self, target: crate::render_helpers::RenderTarget) -> &OffscreenBuffer {
        &self.offscreen[target as usize]
    }

    pub fn retarget(
        &mut self,
        to: Size<f64, Logical>,
        config: niri_config::Animation,
        animate: bool,
    ) -> bool {
        if self.to == to {
            return false;
        }
        self.from = self.pose();
        self.to = to;
        self.animation = animate.then(|| Animation::new(self.clock.clone(), 0., 1., 0., config));
        true
    }

    pub fn pose(&self) -> Size<f64, Logical> {
        let p = self
            .animation
            .as_ref()
            .map_or(1., |a| a.clamped_value().clamp(0., 1.));
        Size::from((
            self.from.w + (self.to.w - self.from.w) * p,
            self.from.h + (self.to.h - self.from.h) * p,
        ))
    }

    pub fn set_anchor(&mut self, anchor: Point<f64, Logical>) {
        self.anchor = anchor;
    }

    pub fn offset(&self, source: Size<f64, Logical>) -> Point<f64, Logical> {
        let pose = self.pose();
        Point::from((
            (source.w - pose.w) * self.anchor.x,
            (source.h - pose.h) * self.anchor.y,
        ))
    }

    pub fn is_animating(&self) -> bool {
        self.animation.as_ref().is_some_and(|a| !a.is_done())
    }

    pub fn advance(&mut self) {
        if self.animation.as_ref().is_some_and(Animation::is_done) {
            self.animation = None;
        }
    }

    pub fn update(&mut self, gradient: Gradient, radius: CornerRadius, scale: f64) {
        let size = self
            .pose()
            .to_physical_precise_round(scale)
            .to_logical(scale);
        self.fill.update(
            size,
            Rectangle::from_size(size),
            gradient.in_,
            gradient.from,
            gradient.to,
            0.,
            Rectangle::from_size(size),
            0.,
            radius.fit_to(size.w as f32, size.h as f32),
            scale as f32,
            1.,
        );
        // Native shader fallback: simple premultiplied midpoint, preserving configured alpha.
        let from = gradient.from.to_array_premul();
        let to = gradient.to.to_array_premul();
        self.fallback.resize(size);
        self.fallback
            .set_color(Color::from_array_premul(std::array::from_fn(|i| {
                (from[i] + to[i]) / 2.
            })));
        self.blocked.resize(size);
        self.blocked.set_color([0., 0., 0., 1.]);
    }

    pub fn render(
        &self,
        renderer: &mut impl NiriRenderer,
        location: Point<f64, Logical>,
        blocked: bool,
        alpha: f32,
        push: &mut dyn FnMut(SpineRenderElement),
    ) {
        if blocked {
            push(
                SolidColorRenderElement::from_buffer(
                    &self.blocked,
                    location,
                    alpha,
                    Kind::Unspecified,
                )
                .into(),
            );
        } else if Shaders::get(renderer)
            .program(ProgramType::ShrinkGhost)
            .is_some()
        {
            push(
                self.fill
                    .clone()
                    .with_location(location)
                    .with_alpha(alpha)
                    .into(),
            );
        } else {
            push(
                SolidColorRenderElement::from_buffer(
                    &self.fallback,
                    location,
                    alpha,
                    Kind::Unspecified,
                )
                .into(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retarget_uses_current_pose_and_stops_render_animation_without_ending_preview() {
        let mut clock = Clock::with_time(std::time::Duration::ZERO);
        let mut preview = ShrinkPreview::new(clock.clone(), (600., 400.).into());
        preview.retarget(
            (32., 400.).into(),
            niri_config::Animations::default().window_movement.0,
            true,
        );
        assert_eq!(preview.pose(), Size::from((600., 400.)));
        clock.set_unadjusted(std::time::Duration::from_millis(50));
        let intermediate = preview.pose();
        assert!(intermediate.w < 600. && intermediate.w >= 32.);
        preview.retarget(
            (600., 32.).into(),
            niri_config::Animations::default().window_movement.0,
            true,
        );
        assert_eq!(preview.pose(), intermediate);
        clock.set_complete_instantly(true);
        preview.advance();
        assert!(!preview.is_animating());
        assert_eq!(preview.pose(), Size::from((600., 32.)));
        preview.retarget(
            (32., 400.).into(),
            niri_config::Animations::default().window_movement.0,
            false,
        );
        assert_eq!(preview.pose(), Size::from((32., 400.)));
        assert!(!preview.is_animating());
    }
}
