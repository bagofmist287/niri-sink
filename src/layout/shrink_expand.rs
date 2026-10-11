//! Geometry of a spine, independent of the client's buffer and size constraints.

use smithay::utils::{Logical, Size};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShrinkAxis {
    /// Lock the outer width, producing a vertical spine.
    Horizontal,
    /// Lock the outer height, producing a horizontal spine.
    Vertical,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum WindowForm {
    #[default]
    Normal,
    Spine(ShrinkAxis),
}

/// Only presentation geometry is retained. This is not a normal-size restore record.
#[derive(Debug, Clone, Copy)]
pub struct SpineGeometry {
    pub axis: ShrinkAxis,
    thickness: f64,
    outer_size: Size<f64, Logical>,
}

impl SpineGeometry {
    pub fn new(
        axis: ShrinkAxis,
        thickness: f64,
        allocation: Size<f64, Logical>,
        scale: f64,
        border: f64,
    ) -> Option<Self> {
        if !thickness.is_finite()
            || thickness <= 0.
            || !scale.is_finite()
            || scale <= 0.
            || !border.is_finite()
            || border < 0.
            || !allocation.w.is_finite()
            || !allocation.h.is_finite()
        {
            return None;
        }
        let mut geometry = Self {
            axis,
            thickness,
            outer_size: allocation,
        };
        geometry.allocate(allocation, scale, border);
        Some(geometry)
    }

    /// Includes the border and at least one physical pixel of interior.
    pub fn effective_thickness(&self, scale: f64, border: f64) -> f64 {
        (self.thickness.max(border * 2. + 1. / scale) * scale).ceil() / scale
    }

    pub fn allocate(&mut self, allocation: Size<f64, Logical>, scale: f64, border: f64) {
        let t = self.effective_thickness(scale, border);
        let round = |extent: f64| (extent.max(t) * scale).round() / scale;
        self.outer_size = match self.axis {
            ShrinkAxis::Horizontal => Size::from((t, round(allocation.h))),
            ShrinkAxis::Vertical => Size::from((round(allocation.w), t)),
        };
    }

    pub fn outer_size(&self) -> Size<f64, Logical> {
        self.outer_size
    }

    pub fn min_size(&self, scale: f64, border: f64) -> Size<f64, Logical> {
        let t = self.effective_thickness(scale, border);
        Size::from((t, t))
    }

    /// Zero denotes no maximum, matching the native Tile constraint convention.
    pub fn max_size(&self, scale: f64, border: f64) -> Size<f64, Logical> {
        let t = self.effective_thickness(scale, border);
        match self.axis {
            ShrinkAxis::Horizontal => Size::from((t, 0.)),
            ShrinkAxis::Vertical => Size::from((0., t)),
        }
    }
}
