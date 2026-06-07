//! LiquidGlass effect: a refractive glass material rendered via RuntimeShader.
//!
//! Faithful port of the Android AGSL LiquidGlass shader to WGSL, matching the
//! Jetpack Compose API. Uses SDF-based rounded rectangles with height profiles
//! for refraction, rim specular lighting, and tint color blending.

use crate::{Color, RenderEffect, RuntimeShader};

/// LiquidGlass WGSL shader source — faithful port of Android's LIQUID_GLASS_AGSL.
///
/// Bindings:
/// - group(0) binding(0): input_texture (the content behind the glass)
/// - group(0) binding(1): input_sampler
/// - group(1) binding(0): uniform array u[64 vec4s]
///
/// Uniform layout (float indices, all in pixel/dp units):
///   0,1: container size (width, height) px
///   2,3: rect center (cx, cy) px
///   4,5: rect size (w, h) px
///   6: corner radius px
///   7: bezel width px
///   8: displacement scale (default 44.0)
///   9: refractive index (default 1.8)
///  10: profile exponent (default 1.4)
///  11: highlight intensity (default 0.7)
///  12,13: tilt (angle, pitch) radians
///  14,15,16,17: tint color (r,g,b,a)
pub const LIQUID_GLASS_WGSL: &str = r#"
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

// Uniform accessors
fn get_float(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

fn get_vec2(index: u32) -> vec2<f32> {
    return vec2<f32>(get_float(index), get_float(index + 1u));
}

fn get_vec4(index: u32) -> vec4<f32> {
    return vec4<f32>(get_float(index), get_float(index + 1u), get_float(index + 2u), get_float(index + 3u));
}

// SDF for rounded rectangle — matches Android sdRoundRect
fn sd_round_rect(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

// Height profile: glass surface curvature.
// Matches Android AGSL exactly:
//   circle:   x * x          (0 at edge, 1 deep inside)
//   squircle: 1 - pow(1-x,4) (0 at edge, 1 deep inside)
// `x` is normalized distance from edge inward (0=edge, 1=deep inside).
fn height_profile(x: f32, profile: f32) -> f32 {
    let xc = clamp(x, 0.0, 1.0);
    let h_circle = xc * xc;
    let h_squircle = 1.0 - pow(1.0 - xc, 4.0);
    return mix(h_circle, h_squircle, clamp(profile, 0.0, 1.0));
}

// Derivative of height profile — well-behaved, no singularities.
//   circle':   2 * x
//   squircle': 4 * pow(1-x, 3)
fn d_height_dx(x: f32, profile: f32) -> f32 {
    let xc = clamp(x, 0.0, 1.0);
    let d_circle = 2.0 * xc;
    let d_squircle = 4.0 * pow(1.0 - xc, 3.0);
    return mix(d_circle, d_squircle, clamp(profile, 0.0, 1.0));
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = input.uv;
    let tex_size = vec2<f32>(textureDimensions(input_texture));

    // Effect layer pixel rect injected by the renderer at uniform slot 62
    // (x_offset, y_offset, width, height) in viewport pixels.
    let effect_rect = get_vec4(248u);
    let container_dp = get_vec2(0u);

    // dp → pixel scale: effect area pixel size / container dp size
    let dp_scale = effect_rect.zw / max(container_dp, vec2<f32>(1.0));
    let s = min(dp_scale.x, dp_scale.y);

    // Fragment position in effect-local pixel coordinates
    let coord = uv * tex_size - effect_rect.xy;

    let center = get_vec2(2u) * dp_scale;
    let rect_size = get_vec2(4u) * dp_scale;
    let corner_radius = get_float(6u) * s;
    let bezel = get_float(7u) * s;
    let disp_scale = get_float(8u) * s;
    let ri = get_float(9u);
    let profile = get_float(10u);
    let highlight = get_float(11u);
    let tilt_angle = get_float(12u);
    let tilt_pitch = get_float(13u);
    let tint_color = get_vec4(14u);

    let half_size = rect_size * 0.5;

    // SDF distance from rounded rect edge (negative = inside)
    let p = coord - center;
    let d = sd_round_rect(p, half_size, corner_radius);

    // If outside the glass rect, pass through original
    if d > 0.0 {
        return textureSample(input_texture, input_sampler, uv);
    }

    // Normalized distance: 0 at rect edge, 1 at one bezel-width inside
    let x = clamp(-d / max(bezel, 0.001), 0.0, 1.0);

    // Height and slope from profile (bezel curvature)
    let slope = d_height_dx(x, profile);

    // Refraction displacement (matches Android AGSL):
    //   bend = slope * (1 - 1/ri)
    //   tilt is a raw vec2 (angle, pitch), NOT trigonometric
    //   displacement = tilt * bend * scale
    let bend = slope * (1.0 - 1.0 / max(ri, 1.0001));
    let tilt = vec2<f32>(tilt_angle, tilt_pitch);
    let disp = tilt * bend * disp_scale;
    let displaced_uv = clamp(uv + disp / tex_size, vec2<f32>(0.0), vec2<f32>(1.0));

    // Sample refracted background
    var outc = textureSample(input_texture, input_sampler, displaced_uv);

    // Tint color blending (matches Android: mix(outc.rgb, tint.rgb, tint.a))
    let tint_a = tint_color.a;
    outc = vec4<f32>(mix(outc.rgb, tint_color.rgb, tint_a), outc.a);

    // SDF gradient for specular normal computation
    let eps = 0.5;
    let d_dx = sd_round_rect(p + vec2<f32>(eps, 0.0), half_size, corner_radius);
    let d_dy = sd_round_rect(p + vec2<f32>(0.0, eps), half_size, corner_radius);
    let grad = vec2<f32>(d_dx - d, d_dy - d) / eps;
    let grad_len = length(grad);
    let inward_normal = select(vec2<f32>(0.0), -grad / grad_len, grad_len > 0.001);

    // Specular rim highlight (matches Android AGSL):
    //   rim = pow(1-x, 3), facing = 0.5 + 0.5 * dot(N, lightDir)
    //   spec added as: outc + spec * specA * 0.85
    let light_dir = normalize(tilt + vec2<f32>(0.0, 0.5));
    let rim = pow(max(1.0 - x, 0.0), 3.0);
    let facing = 0.5 + 0.5 * dot(inward_normal, light_dir);
    let spec_a = highlight * rim * facing;
    outc = vec4<f32>(outc.rgb + vec3<f32>(spec_a * 0.85), outc.a);

    return outc;
}
"#;

/// Configuration for the LiquidGlass effect.
///
/// Defaults match Android's `LiquidGlassSpec` companion defaults.
#[derive(Clone, Debug)]
pub struct LiquidGlassSpec {
    /// Corner radius of the glass rounded rect, in dp/px.
    pub corner_radius: f32,
    /// Width of the edge bezel (transition zone), in dp/px.
    pub bezel_width: f32,
    /// How much the refraction displaces the background, in pixels.
    pub displacement_scale: f32,
    /// Refractive index (1.0 = no refraction, higher = more bending).
    pub refractive_index: f32,
    /// Surface profile exponent: 0 = circle, 1 = squircle.
    pub profile: f32,
    /// Specular highlight intensity.
    pub highlight: f32,
    /// Tilt angle (radians) — horizontal light direction.
    pub tilt_angle: f32,
    /// Tilt pitch (radians) — vertical light direction.
    pub tilt_pitch: f32,
}

impl Default for LiquidGlassSpec {
    fn default() -> Self {
        Self {
            corner_radius: 28.0,
            bezel_width: 14.0,
            displacement_scale: 44.0,
            refractive_index: 1.8,
            profile: 1.4,
            highlight: 0.7,
            tilt_angle: 0.0,
            tilt_pitch: 0.0,
        }
    }
}

/// A rectangular region where the liquid glass effect is applied.
///
/// Coordinates are in dp/pixels relative to the effect area, matching the
/// Android `LiquidGlassRect` API.
#[derive(Clone, Debug)]
pub struct LiquidGlassRect {
    /// Left edge in dp/px.
    pub left: f32,
    /// Top edge in dp/px.
    pub top: f32,
    /// Width in dp/px.
    pub width: f32,
    /// Height in dp/px.
    pub height: f32,
    /// Tint color applied to the glass.
    pub tint_color: Color,
}

/// Build a `RenderEffect` that applies the LiquidGlass shader to a single rect.
///
/// `area_width` and `area_height` are the total effect area size in dp/pixels.
pub fn liquid_glass_effect(
    rect: &LiquidGlassRect,
    spec: &LiquidGlassSpec,
    area_width: f32,
    area_height: f32,
) -> RenderEffect {
    let mut shader = RuntimeShader::new(LIQUID_GLASS_WGSL);

    // Compute center in pixels
    let cx = rect.left + rect.width * 0.5;
    let cy = rect.top + rect.height * 0.5;

    // Uniform layout — see doc comment on LIQUID_GLASS_WGSL
    shader.set_float2(0, area_width, area_height); // container size
    shader.set_float2(2, cx, cy); // rect center
    shader.set_float2(4, rect.width, rect.height); // rect size
    shader.set_float(6, spec.corner_radius);
    shader.set_float(7, spec.bezel_width);
    shader.set_float(8, spec.displacement_scale);
    shader.set_float(9, spec.refractive_index);
    shader.set_float(10, spec.profile);
    shader.set_float(11, spec.highlight);
    shader.set_float2(12, spec.tilt_angle, spec.tilt_pitch);
    shader.set_float4(
        14,
        rect.tint_color.r(),
        rect.tint_color.g(),
        rect.tint_color.b(),
        rect.tint_color.a(),
    );
    shader.set_input_padding(liquid_glass_input_padding(spec));

    RenderEffect::runtime_shader(shader)
}

fn liquid_glass_input_padding(spec: &LiquidGlassSpec) -> f32 {
    let bend = 1.0 - 1.0 / spec.refractive_index.max(1.0001);
    let tilt = spec.tilt_angle.abs().max(spec.tilt_pitch.abs());
    let slope = liquid_glass_max_height_slope(spec.profile);
    let displacement = tilt * bend * spec.displacement_scale.max(0.0) * slope;
    if displacement > 0.0 {
        displacement.ceil() + 2.0
    } else {
        0.0
    }
}

fn liquid_glass_max_height_slope(profile: f32) -> f32 {
    let p = profile.clamp(0.0, 1.0);
    2.0 + 2.0 * p
}

/// Build a chained `RenderEffect` for multiple liquid glass rects.
///
/// Each rect is applied as a separate shader pass chained together.
pub fn liquid_glass_effect_multi(
    rects: &[LiquidGlassRect],
    spec: &LiquidGlassSpec,
    area_width: f32,
    area_height: f32,
) -> Option<RenderEffect> {
    let mut result: Option<RenderEffect> = None;
    for rect in rects {
        let effect = liquid_glass_effect(rect, spec, area_width, area_height);
        result = Some(match result {
            Some(existing) => existing.then(effect),
            None => effect,
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liquid_glass_spec_defaults() {
        let spec = LiquidGlassSpec::default();
        assert_eq!(spec.corner_radius, 28.0);
        assert_eq!(spec.bezel_width, 14.0);
        assert_eq!(spec.displacement_scale, 44.0);
        assert_eq!(spec.refractive_index, 1.8);
        assert_eq!(spec.profile, 1.4);
        assert_eq!(spec.highlight, 0.7);
    }

    #[test]
    fn liquid_glass_rect_fields() {
        let rect = LiquidGlassRect {
            left: 50.0,
            top: 30.0,
            width: 200.0,
            height: 100.0,
            tint_color: Color(0.6, 0.8, 1.0, 0.15),
        };
        assert_eq!(rect.left, 50.0);
        assert_eq!(rect.width, 200.0);
    }

    #[test]
    fn liquid_glass_effect_single() {
        let rect = LiquidGlassRect {
            left: 50.0,
            top: 30.0,
            width: 200.0,
            height: 100.0,
            tint_color: Color(0.5, 0.5, 1.0, 0.1),
        };
        let spec = LiquidGlassSpec::default();
        let effect = liquid_glass_effect(&rect, &spec, 800.0, 600.0);
        assert!(matches!(effect, RenderEffect::Shader { .. }));
    }

    #[test]
    fn liquid_glass_effect_uniforms() {
        let rect = LiquidGlassRect {
            left: 100.0,
            top: 50.0,
            width: 200.0,
            height: 100.0,
            tint_color: Color(0.5, 0.5, 1.0, 0.1),
        };
        let spec = LiquidGlassSpec::default();
        let effect = liquid_glass_effect(&rect, &spec, 800.0, 600.0);
        if let RenderEffect::Shader { shader } = effect {
            let u = shader.uniforms();
            // container size
            assert_eq!(u[0], 800.0);
            assert_eq!(u[1], 600.0);
            // center = (left + width/2, top + height/2) = (200, 100)
            assert_eq!(u[2], 200.0);
            assert_eq!(u[3], 100.0);
            // rect size
            assert_eq!(u[4], 200.0);
            assert_eq!(u[5], 100.0);
            // corner radius (default 28.0)
            assert_eq!(u[6], 28.0);
            // displacement scale
            assert_eq!(u[8], 44.0);
            // refractive index
            assert_eq!(u[9], 1.8);
            assert_eq!(shader.input_padding(), 0.0);
        } else {
            panic!("expected Shader effect");
        }
    }

    #[test]
    fn liquid_glass_declares_backdrop_input_padding_for_tilted_refraction() {
        let rect = LiquidGlassRect {
            left: 0.0,
            top: 0.0,
            width: 140.0,
            height: 100.0,
            tint_color: Color(0.5, 0.5, 1.0, 0.1),
        };
        let effect = liquid_glass_effect(
            &rect,
            &LiquidGlassSpec {
                tilt_angle: 0.5,
                tilt_pitch: 0.3,
                ..LiquidGlassSpec::default()
            },
            140.0,
            100.0,
        );
        let RenderEffect::Shader { shader } = effect else {
            panic!("expected Shader effect");
        };
        assert!(
            shader.input_padding() >= 11.0,
            "tilted liquid glass must capture enough backdrop for displaced samples"
        );
    }

    #[test]
    fn liquid_glass_padding_covers_max_shader_displacement() {
        let spec = LiquidGlassSpec {
            tilt_angle: 0.5,
            tilt_pitch: 0.3,
            ..LiquidGlassSpec::default()
        };
        let effect = liquid_glass_effect(
            &LiquidGlassRect {
                left: 0.0,
                top: 0.0,
                width: 140.0,
                height: 100.0,
                tint_color: Color(0.5, 0.5, 1.0, 0.1),
            },
            &spec,
            140.0,
            100.0,
        );
        let RenderEffect::Shader { shader } = effect else {
            panic!("expected Shader effect");
        };
        let bend = 1.0 - 1.0 / spec.refractive_index.max(1.0001);
        let max_shader_displacement = spec.tilt_angle.abs() * bend * spec.displacement_scale * 4.0;

        assert!(
            shader.input_padding() >= max_shader_displacement.ceil() + 2.0,
            "backdrop capture must cover the shader's largest refracted sample"
        );
    }

    #[test]
    fn liquid_glass_effect_multi_chains() {
        let rects = vec![
            LiquidGlassRect {
                left: 10.0,
                top: 10.0,
                width: 100.0,
                height: 100.0,
                tint_color: Color(1.0, 0.0, 0.0, 0.1),
            },
            LiquidGlassRect {
                left: 200.0,
                top: 200.0,
                width: 100.0,
                height: 100.0,
                tint_color: Color(0.0, 0.0, 1.0, 0.1),
            },
        ];
        let spec = LiquidGlassSpec::default();
        let effect = liquid_glass_effect_multi(&rects, &spec, 800.0, 600.0);
        assert!(effect.is_some());
        assert!(matches!(effect.unwrap(), RenderEffect::Chain { .. }));
    }

    #[test]
    fn liquid_glass_effect_multi_empty() {
        let spec = LiquidGlassSpec::default();
        let effect = liquid_glass_effect_multi(&[], &spec, 800.0, 600.0);
        assert!(effect.is_none());
    }
}
