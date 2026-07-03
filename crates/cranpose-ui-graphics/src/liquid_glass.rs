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
pub const LIQUID_GLASS_WGSL: &str = include_str!("../shaders/liquid_glass.wgsl");

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
