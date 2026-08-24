//! Alpha-mask helpers for graphics-layer effects.
//!
//! These utilities provide a dev-facing API for common Jetpack Compose style
//! masking workflows, such as revealing/cutting content with a rounded shape
//! and a feathered opacity transition.

use crate::{CornerRadii, RenderEffect, RuntimeShader};

/// Direction of the gradient cut mask.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CutDirection {
    /// Keep content from the left edge up to the cut progress.
    #[default]
    LeftToRight,
    /// Keep content from the right edge up to the cut progress.
    RightToLeft,
    /// Keep content from the top edge down to the cut progress.
    TopToBottom,
    /// Keep content from the bottom edge up to the cut progress.
    BottomToTop,
}

impl CutDirection {
    fn uniform_code(self) -> f32 {
        match self {
            CutDirection::LeftToRight => 0.0,
            CutDirection::RightToLeft => 1.0,
            CutDirection::TopToBottom => 2.0,
            CutDirection::BottomToTop => 3.0,
        }
    }
}

/// Configuration for a directional gradient cut mask.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientCutMaskSpec {
    /// Reveal progress in [0, 1].
    pub progress: f32,
    /// Width of the edge feather in dp/px.
    pub feather: f32,
    /// Rounded-corner radius of the masked area in dp/px.
    pub corner_radius: f32,
    /// Direction from which content is revealed.
    pub direction: CutDirection,
}

impl Default for GradientCutMaskSpec {
    fn default() -> Self {
        Self {
            progress: 0.5,
            feather: 24.0,
            corner_radius: 16.0,
            direction: CutDirection::LeftToRight,
        }
    }
}

/// Configuration for a directional gradient fade mask that matches
/// `drawWithContent { drawRect(..., blendMode = DstOut) }` behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientFadeMaskSpec {
    /// Axis coordinate where fade starts (fully cut) in dp/px.
    pub start: f32,
    /// Axis coordinate where fade ends (fully visible) in dp/px.
    pub end: f32,
    /// Direction that defines the gradient axis.
    pub direction: CutDirection,
}

impl Default for GradientFadeMaskSpec {
    fn default() -> Self {
        Self {
            start: 0.0,
            end: 64.0,
            direction: CutDirection::TopToBottom,
        }
    }
}

/// WGSL shader for directional cut + rounded rect alpha mask.
///
/// Uniform layout:
/// - 0,1: container size in dp
/// - 2: progress `0,1`
/// - 3: feather in dp
/// - 4: corner radius in dp
/// - 5: direction code (0=L->R, 1=R->L, 2=T->B, 3=B->T)
pub const GRADIENT_CUT_MASK_WGSL: &str = include_str!("../shaders/gradient_cut_mask.wgsl");

/// WGSL shader for rounded-rectangle alpha masking with feathered edges.
///
/// Uniform layout:
/// - 0,1: container size in dp
/// - 2: edge feather in dp
/// - 3,4,5,6: corner radii in dp (top-left, top-right, bottom-right, bottom-left)
pub const ROUNDED_ALPHA_MASK_WGSL: &str = include_str!("../shaders/rounded_alpha_mask.wgsl");

/// WGSL shader for directional fade-out alpha masking (DstOut-style).
///
/// Uniform layout:
/// - 0,1: container size in dp
/// - 2: fade start in dp
/// - 3: fade end in dp
/// - 4: direction code (0=L->R, 1=R->L, 2=T->B, 3=B->T)
pub const GRADIENT_FADE_DST_OUT_WGSL: &str = include_str!("../shaders/gradient_fade_dst_out.wgsl");

/// Builds a directional cut mask effect.
///
/// The resulting `RenderEffect` keeps content from one side up to `progress`
/// with a feathered transition and rounded-rect outer masking.
pub fn gradient_cut_mask_effect(
    spec: &GradientCutMaskSpec,
    area_width: f32,
    area_height: f32,
) -> RenderEffect {
    let mut shader = RuntimeShader::new(GRADIENT_CUT_MASK_WGSL);
    shader.set_float2(0, area_width.max(1.0), area_height.max(1.0));
    shader.set_float(2, spec.progress.clamp(0.0, 1.0));
    shader.set_float(3, spec.feather.max(0.0));
    shader.set_float(4, spec.corner_radius.max(0.0));
    shader.set_float(5, spec.direction.uniform_code());
    RenderEffect::runtime_shader(shader)
}

/// Builds a rounded alpha mask effect without directional cutting.
///
/// Useful for masking other effects (for example blur) to a rounded rectangle
/// with an explicit feathered edge width.
pub fn rounded_alpha_mask_effect(
    area_width: f32,
    area_height: f32,
    corner_radius: f32,
    edge_feather: f32,
) -> RenderEffect {
    rounded_corner_alpha_mask_effect(
        area_width,
        area_height,
        CornerRadii::uniform(corner_radius),
        edge_feather,
    )
}

/// Builds a rounded alpha mask effect with independent corner radii.
pub fn rounded_corner_alpha_mask_effect(
    area_width: f32,
    area_height: f32,
    corner_radii: CornerRadii,
    edge_feather: f32,
) -> RenderEffect {
    let mut shader = RuntimeShader::new(ROUNDED_ALPHA_MASK_WGSL);
    shader.set_float2(0, area_width.max(1.0), area_height.max(1.0));
    shader.set_float(2, edge_feather.max(0.0));
    shader.set_float4(
        3,
        corner_radii.top_left.max(0.0),
        corner_radii.top_right.max(0.0),
        corner_radii.bottom_right.max(0.0),
        corner_radii.bottom_left.max(0.0),
    );
    RenderEffect::runtime_shader(shader)
}

/// Builds a directional gradient fade mask with DstOut semantics.
///
/// Equivalent to drawing an opaque->transparent gradient on top of content
/// using destination-out blending: the fade start is fully cut, and the end
/// is fully preserved.
pub fn gradient_fade_dst_out_effect(
    spec: &GradientFadeMaskSpec,
    area_width: f32,
    area_height: f32,
) -> RenderEffect {
    let mut shader = RuntimeShader::new(GRADIENT_FADE_DST_OUT_WGSL);
    shader.set_float2(0, area_width.max(1.0), area_height.max(1.0));
    shader.set_float(2, spec.start);
    shader.set_float(3, spec.end);
    shader.set_float(4, spec.direction.uniform_code());
    RenderEffect::runtime_shader(shader)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_cut_spec_defaults() {
        let spec = GradientCutMaskSpec::default();
        assert_eq!(spec.progress, 0.5);
        assert_eq!(spec.feather, 24.0);
        assert_eq!(spec.corner_radius, 16.0);
        assert_eq!(spec.direction, CutDirection::LeftToRight);
    }

    #[test]
    fn gradient_fade_spec_defaults() {
        let spec = GradientFadeMaskSpec::default();
        assert_eq!(spec.start, 0.0);
        assert_eq!(spec.end, 64.0);
        assert_eq!(spec.direction, CutDirection::TopToBottom);
    }

    #[test]
    fn gradient_cut_effect_sets_uniforms() {
        let spec = GradientCutMaskSpec {
            progress: 0.33,
            feather: 18.0,
            corner_radius: 20.0,
            direction: CutDirection::BottomToTop,
        };
        let effect = gradient_cut_mask_effect(&spec, 320.0, 180.0);
        let RenderEffect::Shader { shader } = effect else {
            panic!("expected shader render effect");
        };

        let u = shader.uniforms();
        assert_eq!(u[0], 320.0);
        assert_eq!(u[1], 180.0);
        assert_eq!(u[2], 0.33);
        assert_eq!(u[3], 18.0);
        assert_eq!(u[4], 20.0);
        assert_eq!(u[5], 3.0);
    }

    #[test]
    fn gradient_cut_effect_clamps_values() {
        let spec = GradientCutMaskSpec {
            progress: 2.4,
            feather: -3.0,
            corner_radius: -8.0,
            direction: CutDirection::RightToLeft,
        };
        let effect = gradient_cut_mask_effect(&spec, 0.0, 0.0);
        let RenderEffect::Shader { shader } = effect else {
            panic!("expected shader render effect");
        };

        let u = shader.uniforms();
        assert_eq!(u[0], 1.0);
        assert_eq!(u[1], 1.0);
        assert_eq!(u[2], 1.0);
        assert_eq!(u[3], 0.0);
        assert_eq!(u[4], 0.0);
        assert_eq!(u[5], 1.0);
    }

    #[test]
    fn rounded_alpha_mask_uses_dedicated_shader_uniforms() {
        let effect = rounded_alpha_mask_effect(240.0, 120.0, 14.0, 6.0);
        let RenderEffect::Shader { shader } = effect else {
            panic!("expected shader render effect");
        };

        assert_eq!(shader.source(), ROUNDED_ALPHA_MASK_WGSL);
        let u = shader.uniforms();
        assert_eq!(u[0], 240.0);
        assert_eq!(u[1], 120.0);
        assert_eq!(u[2], 6.0);
        assert_eq!(u[3], 14.0);
        assert_eq!(u[4], 14.0);
        assert_eq!(u[5], 14.0);
        assert_eq!(u[6], 14.0);
    }

    #[test]
    fn rounded_corner_alpha_mask_sets_per_corner_uniforms() {
        let effect = rounded_corner_alpha_mask_effect(
            240.0,
            120.0,
            CornerRadii {
                top_left: 4.0,
                top_right: 8.0,
                bottom_right: 12.0,
                bottom_left: 16.0,
            },
            2.0,
        );
        let RenderEffect::Shader { shader } = effect else {
            panic!("expected shader render effect");
        };

        assert_eq!(shader.source(), ROUNDED_ALPHA_MASK_WGSL);
        let u = shader.uniforms();
        assert_eq!(u[0], 240.0);
        assert_eq!(u[1], 120.0);
        assert_eq!(u[2], 2.0);
        assert_eq!(u[3], 4.0);
        assert_eq!(u[4], 8.0);
        assert_eq!(u[5], 12.0);
        assert_eq!(u[6], 16.0);
    }

    #[test]
    fn gradient_fade_dst_out_effect_sets_uniforms() {
        let spec = GradientFadeMaskSpec {
            start: 24.0,
            end: 52.0,
            direction: CutDirection::BottomToTop,
        };
        let effect = gradient_fade_dst_out_effect(&spec, 300.0, 180.0);
        let RenderEffect::Shader { shader } = effect else {
            panic!("expected shader render effect");
        };

        assert_eq!(shader.source(), GRADIENT_FADE_DST_OUT_WGSL);
        let u = shader.uniforms();
        assert_eq!(u[0], 300.0);
        assert_eq!(u[1], 180.0);
        assert_eq!(u[2], 24.0);
        assert_eq!(u[3], 52.0);
        assert_eq!(u[4], 3.0);
    }
}
