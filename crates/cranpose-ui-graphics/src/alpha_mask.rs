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
#[path = "tests/alpha_mask_tests.rs"]
mod tests;
