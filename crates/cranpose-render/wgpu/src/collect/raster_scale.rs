// The scale a walk rasterizes a layer's content at: exactly the window's,
// or at least a scale an animated ancestor holds its raster at.

use cranpose_render_common::geometry::{blur_reach, blur_reach_for_minimum_scale};

const ROUNDED_CLIP_AA_MARGIN: f32 = 1.0;

#[derive(Clone, Copy)]
pub(super) enum RasterScale {
    Exact(f32),
    Minimum(f32),
}

impl RasterScale {
    pub(super) fn within(self, nominal: f32, retained: f32) -> Self {
        if nominal == 1.0 && retained == 1.0 {
            return self;
        }
        if !nominal.is_finite() || nominal <= 0.0 || !retained.is_finite() || retained <= 0.0 {
            return Self::Minimum(0.0);
        }
        let (Self::Exact(parent) | Self::Minimum(parent)) = self;
        Self::Minimum(parent * nominal.min(retained).min(1.0))
    }

    pub(super) fn nominal(self) -> f32 {
        let (Self::Exact(scale) | Self::Minimum(scale)) = self;
        scale
    }

    pub(super) fn shadow_reach(self, radius: f32) -> f32 {
        match self {
            Self::Exact(scale) => blur_reach(radius, scale),
            Self::Minimum(scale) => blur_reach_for_minimum_scale(radius, scale),
        }
    }

    pub(super) fn aa_margin(self) -> f32 {
        let scale = match self {
            Self::Exact(scale) if !scale.is_finite() || scale <= 0.0 => 1.0,
            Self::Exact(scale) | Self::Minimum(scale) => scale,
        };
        if scale.is_finite() && scale > 0.0 {
            ROUNDED_CLIP_AA_MARGIN.max(1.0 / scale)
        } else {
            f32::INFINITY
        }
    }
}
