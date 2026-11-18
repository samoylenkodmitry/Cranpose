//! Spatially varying backdrop blur.
//!
//! Unlike an alpha gradient drawn over a uniformly blurred surface, this
//! shader changes the sampling kernel radius at every fragment. It is intended
//! for edge-to-edge system-bar treatments where content should move smoothly
//! from sharp to fully frosted without a visible material boundary.

use crate::{RenderEffect, RuntimeShader, SubstrateSpec};

/// Axis and direction used to interpolate the blur radius.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GradientBlurDirection {
    /// The blur radius changes from the start radius at the left edge to the end radius at the
    /// right edge.
    LeftToRight,
    /// The blur radius changes from the start radius at the right edge to the end radius at the
    /// left edge.
    RightToLeft,
    /// The blur radius changes from the start radius at the top edge to the end radius at the
    /// bottom edge.
    #[default]
    TopToBottom,
    /// The blur radius changes from the start radius at the bottom edge to the end radius at the
    /// top edge.
    BottomToTop,
}

impl GradientBlurDirection {
    fn uniform_code(self) -> f32 {
        match self {
            Self::LeftToRight => 0.0,
            Self::RightToLeft => 1.0,
            Self::TopToBottom => 2.0,
            Self::BottomToTop => 3.0,
        }
    }
}

/// The blur levels a gradient blur blends between to realise the radius
/// each fragment asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GradientBlurLevels {
    /// The wide radius and its half and quarter: each row reads close to a
    /// Gaussian blur at its own radius.
    #[default]
    Gaussian,
    /// The wide radius and its half: below half the wide radius a row
    /// blends the sharp content with the half level, so fine detail shows
    /// through the blur as it does under the iOS scroll edge effect. The
    /// renderer blurs one level fewer.
    SharpAndHalf,
}

/// How a gradient blur moves from its start radius to its end radius. The
/// radius holds at the start radius before `ramp_start` and eases to the
/// end radius at `ramp_end`, both fractions of the bounds along the
/// direction; a `ramp_end` past 1 leaves the far edge short of the end
/// radius. `levels` names the blur levels the radius blends between.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientBlurProfile {
    /// The fraction at which the radius starts to leave the start radius.
    pub ramp_start: f32,
    /// The fraction at which the radius reaches the end radius.
    pub ramp_end: f32,
    /// The blur levels the radius blends between.
    pub levels: GradientBlurLevels,
}

impl GradientBlurProfile {
    /// A ramp across the whole bounds through Gaussian levels.
    pub const GAUSSIAN: Self = Self {
        ramp_start: 0.0,
        ramp_end: 1.0,
        levels: GradientBlurLevels::Gaussian,
    };
}

impl Default for GradientBlurProfile {
    fn default() -> Self {
        Self::GAUSSIAN
    }
}

/// WGSL implementation of a per-fragment Gaussian-style blur kernel.
pub static GRADIENT_BLUR_WGSL: &str = runtime_shader_wgsl!("gradient_blur.wgsl");

/// Build a spatially varying blur effect.
///
/// Radii are physical pixels. The radius interpolates smoothly in
/// `direction` over the part of the effect bounds `profile` names; callers
/// normally reach this through `Modifier::backdrop_gradient_blur`, which
/// converts from dp automatically.
/// The shader declares a blurred substrate for each of the profile's
/// levels, the wide radius halved, and realises every radius between as a
/// blend of the level below and the level above it.
pub fn gradient_blur_effect(
    start_radius_px: f32,
    end_radius_px: f32,
    direction: GradientBlurDirection,
    profile: GradientBlurProfile,
) -> RenderEffect {
    let start_radius_px = start_radius_px.max(0.0);
    let end_radius_px = end_radius_px.max(0.0);
    let mut shader = RuntimeShader::new(GRADIENT_BLUR_WGSL);
    shader.set_position_independent(true);
    shader.set_float(0, start_radius_px);
    shader.set_float(1, end_radius_px);
    shader.set_float(2, direction.uniform_code());
    shader.set_float(3, profile.ramp_start);
    shader.set_float(4, profile.ramp_end.max(profile.ramp_start + f32::EPSILON));
    let wide_radius_px = start_radius_px.max(end_radius_px);
    shader.set_input_padding(wide_radius_px.ceil());
    shader.set_batched_source(true);
    // The renderer blurs the capture by each level's share of the wide
    // radius, widest first.
    let level = |share: f32| SubstrateSpec::Blur {
        radius_px: wide_radius_px * share,
    };
    match profile.levels {
        GradientBlurLevels::Gaussian => {
            shader.set_substrates(&[level(1.0), level(0.5), level(0.25)]);
        }
        GradientBlurLevels::SharpAndHalf => shader.set_substrates(&[level(1.0), level(0.5)]),
    }
    RenderEffect::runtime_shader(shader)
}

#[cfg(test)]
#[path = "tests/gradient_blur_tests.rs"]
mod tests;
