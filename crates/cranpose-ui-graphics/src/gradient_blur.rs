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
    LeftToRight,
    RightToLeft,
    #[default]
    TopToBottom,
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

/// WGSL implementation of a per-fragment Gaussian-style blur kernel.
pub const GRADIENT_BLUR_WGSL: &str = include_str!("../shaders/gradient_blur.wgsl");

/// The blur levels' shares of the wide radius: the renderer blurs the
/// capture by each, and the shader blends the sharp source and the levels
/// around the radius each fragment asks for.
pub const GRADIENT_BLUR_LEVELS: [f32; 3] = [1.0, 0.5, 0.25];

/// Build a spatially varying blur effect.
///
/// Radii are physical pixels. The radius interpolates smoothly across the
/// complete effect bounds in `direction`; callers normally reach this through
/// `Modifier::backdrop_gradient_blur`, which converts from dp automatically.
/// The shader declares three blurred substrates, the wide radius halved
/// twice, and realises every radius between as a blend of the level below
/// and the level above it.
pub fn gradient_blur_effect(
    start_radius_px: f32,
    end_radius_px: f32,
    direction: GradientBlurDirection,
) -> RenderEffect {
    let start_radius_px = start_radius_px.max(0.0);
    let end_radius_px = end_radius_px.max(0.0);
    let mut shader = RuntimeShader::new(GRADIENT_BLUR_WGSL);
    shader.set_float(0, start_radius_px);
    shader.set_float(1, end_radius_px);
    shader.set_float(2, direction.uniform_code());
    let wide_radius_px = start_radius_px.max(end_radius_px);
    shader.set_input_padding(wide_radius_px.ceil());
    shader.set_batched_source(true);
    shader.set_substrates(
        GRADIENT_BLUR_LEVELS
            .iter()
            .map(|level| SubstrateSpec::Blur {
                radius_px: wide_radius_px * level,
            })
            .collect(),
    );
    RenderEffect::runtime_shader(shader)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_blur_carries_endpoint_radii_and_capture_padding() {
        let RenderEffect::Shader { shader } =
            gradient_blur_effect(0.5, 18.25, GradientBlurDirection::BottomToTop)
        else {
            panic!("gradient blur must use the spatial runtime shader");
        };
        assert_eq!(&shader.uniforms()[..3], &[0.5, 18.25, 3.0]);
        assert_eq!(shader.input_padding(), 19.0);
        assert_eq!(
            shader.substrates(),
            &[
                SubstrateSpec::Blur { radius_px: 18.25 },
                SubstrateSpec::Blur { radius_px: 9.125 },
                SubstrateSpec::Blur { radius_px: 4.5625 },
            ]
        );
    }
}
