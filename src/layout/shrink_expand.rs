//! Geometry of a spine, independent of the client's buffer and size constraints.

use smithay::utils::{Logical, Size};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShrinkAxis {
    /// Lock the outer width, producing a vertical spine.
    Horizontal,
    /// Lock the outer height, producing a horizontal spine.
    Vertical,
}

impl ShrinkAxis {
    pub fn other(self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }
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

/// Candidate geometry and whether it fits the native working-area bounds.
/// A non-fitting move candidate still describes the native minimum-overflow allocation.
#[derive(Debug, Clone, Copy)]
pub struct SpinePlan {
    pub geometry: SpineGeometry,
    pub fits: bool,
}

/// Both axes are calculated from one native layout snapshot, without client requests.
#[derive(Debug, Clone, Copy)]
pub struct SpinePlans {
    horizontal: SpinePlan,
    vertical: SpinePlan,
    preferred: ShrinkAxis,
}

impl SpinePlans {
    pub fn new(
        thickness: f64,
        allocations: [Size<f64, Logical>; 2],
        bounds: Size<f64, Logical>,
        scale: f64,
        border: f64,
        preferred: ShrinkAxis,
    ) -> Option<Self> {
        if !bounds.w.is_finite() || !bounds.h.is_finite() {
            return None;
        }
        let plan = |axis, allocation| {
            let geometry = SpineGeometry::new(axis, thickness, allocation, scale, border)?;
            let size = geometry.outer_size();
            // Candidate sizes occupy whole physical pixels. Do not accept a partial last pixel.
            let fits = (size.w * scale).round() <= (bounds.w * scale).floor()
                && (size.h * scale).round() <= (bounds.h * scale).floor();
            Some(SpinePlan { geometry, fits })
        };
        Some(Self {
            horizontal: plan(ShrinkAxis::Horizontal, allocations[0])?,
            vertical: plan(ShrinkAxis::Vertical, allocations[1])?,
            preferred,
        })
    }

    pub fn get(&self, axis: ShrinkAxis) -> SpinePlan {
        match axis {
            ShrinkAxis::Horizontal => self.horizontal,
            ShrinkAxis::Vertical => self.vertical,
        }
    }

    /// Prediction is only used on the first invocation; legality takes precedence.
    pub fn first(&self) -> Option<SpinePlan> {
        self.next(self.preferred.other())
    }

    pub fn next(&self, current: ShrinkAxis) -> Option<SpinePlan> {
        let other = self.get(current.other());
        let current = self.get(current);
        if other.fits {
            Some(other)
        } else if current.fits {
            Some(current)
        } else {
            None
        }
    }

    /// Keep the original axis if both fit, or if neither fits; only switch when necessary.
    pub fn for_move(&self, original: ShrinkAxis) -> SpinePlan {
        let original = self.get(original);
        let other = self.get(original.geometry.axis.other());
        if !original.fits && other.fits {
            other
        } else {
            original
        }
    }
}
