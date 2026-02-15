//! Alpha-mask helpers for graphics-layer effects.
//!
//! These utilities provide a dev-facing API for common Jetpack Compose style
//! masking workflows, such as revealing/cutting content with a rounded shape
//! and a feathered opacity transition.

use crate::{RenderEffect, RuntimeShader};

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
/// - 2: progress [0,1]
/// - 3: feather in dp
/// - 4: corner radius in dp
/// - 5: direction code (0=L->R, 1=R->L, 2=T->B, 3=B->T)
pub const GRADIENT_CUT_MASK_WGSL: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn fullscreen_vs(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var output: VertexOutput;
    let x = f32(i32(vertex_index & 1u) * 2 - 1);
    let y = f32(i32(vertex_index >> 1u) * 2 - 1);
    output.uv = vec2<f32>(x * 0.5 + 0.5, 1.0 - (y * 0.5 + 0.5));
    output.position = vec4<f32>(x, y, 0.0, 1.0);
    return output;
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> u: array<vec4<f32>, 64>;

fn get_float(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

fn get_vec2(index: u32) -> vec2<f32> {
    return vec2<f32>(get_float(index), get_float(index + 1u));
}

fn sd_round_rect(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

fn rounded_rect_alpha(local_px: vec2<f32>, size_px: vec2<f32>, corner_radius_px: f32) -> f32 {
    let half = size_px * 0.5;
    let p = local_px - half;
    let d = sd_round_rect(p, half, corner_radius_px);
    return 1.0 - smoothstep(-1.0, 1.0, d);
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = input.uv;
    let tex_size = vec2<f32>(textureDimensions(input_texture));

    // Effect layer pixel rect injected by renderer in uniform slot 62.
    let effect_rect = vec4<f32>(get_float(248u), get_float(249u), get_float(250u), get_float(251u));
    let container_dp = get_vec2(0u);

    // dp -> pixel mapping for local effect coordinates.
    let dp_scale = effect_rect.zw / max(container_dp, vec2<f32>(1.0));
    let s = min(dp_scale.x, dp_scale.y);

    let local_px = uv * tex_size - effect_rect.xy;
    let size_px = container_dp * dp_scale;

    let progress = clamp(get_float(2u), 0.0, 1.0);
    let feather_px = max(get_float(3u) * s, 0.001);
    let corner_radius_px = max(get_float(4u) * s, 0.0);
    let direction = get_float(5u);

    var axis_value = local_px.x;
    var axis_extent = max(size_px.x, 0.001);

    if (direction >= 0.5 && direction < 1.5) {
        axis_value = size_px.x - local_px.x;
        axis_extent = max(size_px.x, 0.001);
    } else if (direction >= 1.5 && direction < 2.5) {
        axis_value = local_px.y;
        axis_extent = max(size_px.y, 0.001);
    } else if (direction >= 2.5) {
        axis_value = size_px.y - local_px.y;
        axis_extent = max(size_px.y, 0.001);
    }

    var directional_alpha = 1.0;
    if (progress < 1.0) {
        let cut_edge = progress * axis_extent;
        directional_alpha = smoothstep(cut_edge + feather_px * 0.5, cut_edge - feather_px * 0.5, axis_value);
    }
    let shape_alpha = rounded_rect_alpha(local_px, size_px, corner_radius_px);
    let mask = directional_alpha * shape_alpha;

    let sample = textureSample(input_texture, input_sampler, uv);
    return sample * mask;
}
"#;

/// WGSL shader for rounded-rectangle alpha masking with feathered edges.
///
/// Uniform layout:
/// - 0,1: container size in dp
/// - 2: corner radius in dp
/// - 3: edge feather in dp
pub const ROUNDED_ALPHA_MASK_WGSL: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn fullscreen_vs(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var output: VertexOutput;
    let x = f32(i32(vertex_index & 1u) * 2 - 1);
    let y = f32(i32(vertex_index >> 1u) * 2 - 1);
    output.uv = vec2<f32>(x * 0.5 + 0.5, 1.0 - (y * 0.5 + 0.5));
    output.position = vec4<f32>(x, y, 0.0, 1.0);
    return output;
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> u: array<vec4<f32>, 64>;

fn get_float(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

fn get_vec2(index: u32) -> vec2<f32> {
    return vec2<f32>(get_float(index), get_float(index + 1u));
}

fn sd_round_rect(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

fn rounded_rect_alpha(local_px: vec2<f32>, size_px: vec2<f32>, corner_radius_px: f32, feather_px: f32) -> f32 {
    let half = size_px * 0.5;
    let p = local_px - half;
    let d = sd_round_rect(p, half, corner_radius_px);
    let half_feather = max(feather_px * 0.5, 0.001);
    return 1.0 - smoothstep(-half_feather, half_feather, d);
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = input.uv;
    let tex_size = vec2<f32>(textureDimensions(input_texture));

    // Effect layer pixel rect injected by renderer in uniform slot 62.
    let effect_rect = vec4<f32>(get_float(248u), get_float(249u), get_float(250u), get_float(251u));
    let container_dp = get_vec2(0u);

    // dp -> pixel mapping for local effect coordinates.
    let dp_scale = effect_rect.zw / max(container_dp, vec2<f32>(1.0));
    let s = min(dp_scale.x, dp_scale.y);

    let local_px = uv * tex_size - effect_rect.xy;
    let size_px = container_dp * dp_scale;

    let corner_radius_px = max(get_float(2u) * s, 0.0);
    let feather_px = max(get_float(3u) * s, 0.0);
    let mask = rounded_rect_alpha(local_px, size_px, corner_radius_px, feather_px);

    let sample = textureSample(input_texture, input_sampler, uv);
    return sample * mask;
}
"#;

/// WGSL shader for directional fade-out alpha masking (DstOut-style).
///
/// Uniform layout:
/// - 0,1: container size in dp
/// - 2: fade start in dp
/// - 3: fade end in dp
/// - 4: direction code (0=L->R, 1=R->L, 2=T->B, 3=B->T)
pub const GRADIENT_FADE_DST_OUT_WGSL: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn fullscreen_vs(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var output: VertexOutput;
    let x = f32(i32(vertex_index & 1u) * 2 - 1);
    let y = f32(i32(vertex_index >> 1u) * 2 - 1);
    output.uv = vec2<f32>(x * 0.5 + 0.5, 1.0 - (y * 0.5 + 0.5));
    output.position = vec4<f32>(x, y, 0.0, 1.0);
    return output;
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> u: array<vec4<f32>, 64>;

fn get_float(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

fn get_vec2(index: u32) -> vec2<f32> {
    return vec2<f32>(get_float(index), get_float(index + 1u));
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = input.uv;
    let tex_size = vec2<f32>(textureDimensions(input_texture));

    // Effect layer pixel rect injected by renderer in uniform slot 62.
    let effect_rect = vec4<f32>(get_float(248u), get_float(249u), get_float(250u), get_float(251u));
    let container_dp = get_vec2(0u);

    // dp -> pixel mapping for local effect coordinates.
    let dp_scale = effect_rect.zw / max(container_dp, vec2<f32>(1.0));

    let local_px = uv * tex_size - effect_rect.xy;
    let size_px = container_dp * dp_scale;
    let direction = get_float(4u);

    var axis_value = local_px.x;
    var axis_scale = dp_scale.x;
    if (direction >= 0.5 && direction < 1.5) {
        axis_value = size_px.x - local_px.x;
        axis_scale = dp_scale.x;
    } else if (direction >= 1.5 && direction < 2.5) {
        axis_value = local_px.y;
        axis_scale = dp_scale.y;
    } else if (direction >= 2.5) {
        axis_value = size_px.y - local_px.y;
        axis_scale = dp_scale.y;
    }

    let start_px = get_float(2u) * axis_scale;
    let end_px = get_float(3u) * axis_scale;
    let span = max(abs(end_px - start_px), 0.001);

    var keep_alpha = 1.0;
    if (end_px >= start_px) {
        keep_alpha = clamp((axis_value - start_px) / span, 0.0, 1.0);
    } else {
        keep_alpha = clamp((start_px - axis_value) / span, 0.0, 1.0);
    }

    let sample = textureSample(input_texture, input_sampler, uv);
    return sample * keep_alpha;
}
"#;

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
    let mut shader = RuntimeShader::new(ROUNDED_ALPHA_MASK_WGSL);
    shader.set_float2(0, area_width.max(1.0), area_height.max(1.0));
    shader.set_float(2, corner_radius.max(0.0));
    shader.set_float(3, edge_feather.max(0.0));
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
        assert_eq!(u[2], 14.0);
        assert_eq!(u[3], 6.0);
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
