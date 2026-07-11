//! LiquidGlass effect: a refractive glass material rendered via RuntimeShader.
//!
//! An SDF rounded-rect lens over the backdrop: height-profile refraction along
//! the bezel normal (static lensing, visible without any motion), a
//! motion/tilt displacement term, chromatic aberration at the bezel,
//! saturation/vibrancy, scheme-adaptive exposure, tint blending, a specular
//! rim lit from an explicit light direction, and an anti-banding dither.
//!
//! Typically chained after a [`RenderEffect::blur`] over the backdrop for the
//! frosted "regular" material; used alone for the "clear" material.

use crate::{Color, RenderEffect, RuntimeShader};

/// LiquidGlass WGSL shader source.
///
/// Bindings:
/// - group(0) binding(0): input_texture (the content behind the glass)
/// - group(0) binding(1): input_sampler
/// - group(1) binding(0): uniform array u[64 vec4s]
///
/// Uniform layout (float indices; sizes in dp, converted in-shader):
///   0,1: container size (width, height) dp
///   2,3: rect center (cx, cy) dp
///   4,5: rect size (w, h) dp
///   6: corner radius dp
///   7: bezel width dp
///   8: displacement scale (px at 1x)
///   9: refractive index (1.0 = none, higher = more bending)
///  10: profile exponent (0 = circle, 1 = squircle)
///  11: highlight intensity
///  12,13: tilt (x, y) — motion-driven displacement direction
///  14,15,16,17: tint color (r,g,b,a)
///  18: saturation (1.0 = unchanged)
///  19: chromatic aberration spread (relative, 0 = off)
///  20: lift (−1..1; screen-blend toward white / multiply toward black)
///  21: dither amount (0..1, in 1/255 steps)
///  22,23: specular light direction ((0,1) lights the top edge)
///  24: contrast (1.0 = neutral; ≤0 treated as 1.0)
///  25: edge-band fraction of the bezel carrying the strong lens + CA
///  80: loupe mode (>0.5 replaces the lens terms with the drop optic)
///  81,82: loupe focus offset from the shape center (dp)
///  83: loupe center magnification (m0)
///  84: loupe band start (depth fraction 0..1 where the rim fold begins)
///  85: loupe fold peak (sampling reach at the fold crest, in inradius units)
///  86: loupe band dispersion strength
pub const LIQUID_GLASS_WGSL: &str = include_str!("../shaders/liquid_glass.wgsl");

/// Configuration for the LiquidGlass effect.
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidGlassSpec {
    /// Corner radius of the glass rounded rect, in dp.
    pub corner_radius: f32,
    /// Width of the edge bezel (refractive transition zone), in dp.
    pub bezel_width: f32,
    /// How much the refraction displaces the background, in px.
    pub displacement_scale: f32,
    /// Refractive index (1.0 = no refraction, higher = more bending).
    pub refractive_index: f32,
    /// Surface profile exponent: 0 = circle, 1 = squircle.
    pub profile: f32,
    /// Specular highlight intensity.
    pub highlight: f32,
    /// Motion tilt (x) — gesture/device-motion displacement input.
    pub tilt_angle: f32,
    /// Motion tilt (y).
    pub tilt_pitch: f32,
    /// Saturation/vibrancy multiplier applied to the refracted backdrop.
    pub saturation: f32,
    /// Chromatic aberration spread at the bezel (0 = off, ~0.4 = iOS-like).
    pub chromatic_aberration: f32,
    /// Scheme lift: positive screen-blends toward white (light scheme),
    /// negative multiplies toward black (dark scheme). Screen keeps the
    /// backdrop ghosts colored, unlike an alpha mix.
    pub lift: f32,
    /// Contrast pivot around mid-gray (1.0 = neutral).
    pub contrast: f32,
    /// Fraction of the bezel forming the steep edge-lens band (also carries
    /// the chromatic aberration).
    pub edge_band: f32,
    /// Anti-banding dither amount (0..1, in 1/255 steps).
    pub dither: f32,
    /// Specular light direction; `(0, 1)` lights the top edge.
    pub light_direction: (f32, f32),
}

impl Default for LiquidGlassSpec {
    fn default() -> Self {
        Self {
            corner_radius: 28.0,
            bezel_width: 14.0,
            displacement_scale: 24.0,
            refractive_index: 1.5,
            profile: 0.6,
            highlight: 0.7,
            tilt_angle: 0.0,
            tilt_pitch: 0.0,
            saturation: 1.0,
            chromatic_aberration: 0.0,
            lift: 0.0,
            contrast: 1.0,
            edge_band: 0.5,
            dither: 0.5,
            light_direction: (0.0, 1.0),
        }
    }
}

/// A rectangular region where the liquid glass effect is applied.
///
/// Coordinates are in dp relative to the effect area.
#[derive(Clone, Debug)]
pub struct LiquidGlassRect {
    /// Left edge in dp.
    pub left: f32,
    /// Top edge in dp.
    pub top: f32,
    /// Width in dp.
    pub width: f32,
    /// Height in dp.
    pub height: f32,
    /// Tint color applied to the glass.
    pub tint_color: Color,
}

/// Build a `RenderEffect` that applies the LiquidGlass shader to a single rect.
///
/// `area_width` and `area_height` are the total effect area size in dp.
pub fn liquid_glass_effect(
    rect: &LiquidGlassRect,
    spec: &LiquidGlassSpec,
    area_width: f32,
    area_height: f32,
) -> RenderEffect {
    let mut shader = RuntimeShader::new(LIQUID_GLASS_WGSL);

    // Compute center in dp
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
    shader.set_float(18, spec.saturation);
    shader.set_float(19, spec.chromatic_aberration);
    shader.set_float(20, spec.lift);
    shader.set_float(21, spec.dither);
    shader.set_float2(22, spec.light_direction.0, spec.light_direction.1);
    shader.set_float(24, spec.contrast);
    shader.set_float(25, spec.edge_band);
    shader.set_input_padding(liquid_glass_input_padding(spec));

    RenderEffect::runtime_shader(shader)
}

/// How far the shader's refracted samples can reach outside the effect rect —
/// the backdrop capture must cover it. The displacement is
/// `(normal + tilt) * bend * scale` with the chromatic-aberration spread on
/// top; `|normal| = 1`, so the static lens contributes even with zero tilt.
fn liquid_glass_input_padding(spec: &LiquidGlassSpec) -> f32 {
    let bend = 1.0 - 1.0 / spec.refractive_index.max(1.0001);
    let tilt = (spec.tilt_angle * spec.tilt_angle + spec.tilt_pitch * spec.tilt_pitch).sqrt();
    let reach = 1.0 + tilt;
    let slope = liquid_glass_max_lens_slope(spec.profile);
    let spread = 1.0 + spec.chromatic_aberration.max(0.0) * 0.5;
    let displacement = reach * bend * spec.displacement_scale.max(0.0) * slope * spread;
    if displacement > 0.0 {
        displacement.ceil() + 2.0
    } else {
        0.0
    }
}

/// Largest combined lens factor the shader can produce: the squircle edge
/// band peaks at 4 at the rim, plus 0.35 × the dome slope.
fn liquid_glass_max_lens_slope(profile: f32) -> f32 {
    let p = profile.clamp(0.0, 1.0);
    4.0 + 0.35 * (2.0 + 2.0 * p)
}

/// The text-drag loupe material: a solid glass drop magnifying an offset
/// focus (the grab point under the finger), displayed inside a capsule
/// floating above it. Measured against the reference recording:
/// dome magnification (`magnification` at the center easing to exactly 1
/// where the rim band starts), a rim FOLD that paints an inverted compressed
/// image of the content just beyond the bubble, chromatic dispersion confined
/// to that band, and the thin interactive-lens rim line.
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidLoupeSpec {
    /// Magnification (the reference loupe measures a uniform ~1.25×).
    pub magnification: f32,
    /// Focus offset from the bubble center, dp (the reference samples 75 dp
    /// below its center: content from under the finger, displayed above).
    pub focus_offset: (f32, f32),
    /// Depth fraction (0..1 of the inradius) where the rim fold band begins.
    pub band_start: f32,
    /// Sampling reach at the fold crest, in inradius units (>1 reaches past
    /// the bubble edge before folding back — the inversion).
    pub fold_peak: f32,
    /// Dispersion strength inside the band (RGB fringes on the folded rim).
    pub dispersion: f32,
    /// Specular rim intensity.
    pub highlight: f32,
    /// Progress of the grow-in (0..1): scales magnification toward 1 and
    /// softens the rim so the bubble inflates out of the text.
    pub progress: f32,
}

impl Default for LiquidLoupeSpec {
    fn default() -> Self {
        Self {
            magnification: 1.25,
            focus_offset: (0.0, 75.0),
            // Measured: the fold owns the outer ~30% of the depth on the
            // long edges; its sampling starts fully PAST the handle dot's
            // zone (band through the dot mirrored its bottom into a skirt)
            // and reaches the next text line's x-band, mirrored near 1:1.
            band_start: 0.78,
            fold_peak: 0.80,
            // The reference fringes are tight (3-5 px at 3x) and live only in
            // the fold band.
            dispersion: 0.22,
            // The reference rim reads as a clear bright line around the whole
            // capsule (peak ~+127 luminance over the backdrop); the
            // interactive-lens rim gain is a whisper, so the loupe drives it
            // through its highlight (calibrated on captures).
            highlight: 4.4,
            progress: 1.0,
        }
    }
}

/// Builds the loupe backdrop effect for a capsule node of `node_size` dp.
/// Explicit-rect mode: the container carries the node size in dp and the
/// shader derives px-per-dp from the renderer-injected pixel rect, so the
/// bubble lands correctly at ANY render scale (live density, robot captures
/// at 1.0, fractional desktop scales).
pub fn liquid_loupe_effect(node_size: (f32, f32), spec: &LiquidLoupeSpec) -> RenderEffect {
    let (w, h) = (node_size.0.max(1.0), node_size.1.max(1.0));
    let progress = spec.progress.clamp(0.0, 1.0);
    // The reference bubble is a fixed-optic lens whose SHAPE grows: it is
    // born already ~85% magnified with a visible rim, and only the residue
    // ramps in with the inflation.
    let magnification = 1.0 + (spec.magnification - 1.0) * (0.85 + 0.15 * progress);
    let mut shader = RuntimeShader::new(LIQUID_GLASS_WGSL);
    shader.set_float2(0, w, h); // container = node size dp
    shader.set_float2(2, w * 0.5, h * 0.5); // capsule centered in the node
    shader.set_float2(4, w, h);
    shader.set_float(6, -1.0); // capsule radius sentinel
    shader.set_float(7, 0.5 * h.min(w)); // bezel = inradius (sheen falloff)
    shader.set_float(11, spec.highlight * (0.45 + 0.55 * progress));
    shader.set_float4(14, 1.0, 1.0, 1.0, 0.0); // no tint
    shader.set_float(18, 1.0); // saturation neutral
    shader.set_float(20, 0.0); // no lift
    shader.set_float(21, 0.5); // dither
                               // Measured on captures: the main specular arc lands on the edge facing
                               // the light vector's tip — (0,1) is the TOP. The reference loupe rim is
                               // brightest on top with a softer bottom counter arc.
    shader.set_float2(22, 0.0, 1.0);
    shader.set_float(24, 1.0); // contrast neutral
    shader.set_float(28, 1.0); // interactive-lens rim style
    shader.set_float(29, 0.45); // soft top glow inside the rim (the
                                // reference's top edge blooms; a bare thin
                                // line read as a flat stroke)
    shader.set_float(80, 1.0); // loupe mode
    shader.set_float2(81, spec.focus_offset.0, spec.focus_offset.1);
    shader.set_float(83, magnification);
    shader.set_float(84, spec.band_start);
    shader.set_float(85, spec.fold_peak);
    shader.set_float(86, spec.dispersion * (0.35 + 0.65 * progress));
    // The capture must cover the farthest sample: the focus offset plus the
    // fold reach past the bubble edge (in dp; paddings are logical units).
    let r_in = 0.5 * w.min(h);
    let focus_reach = (spec.focus_offset.0.powi(2) + spec.focus_offset.1.powi(2)).sqrt();
    let fold_reach = (spec.fold_peak.max(1.0) - 1.0) * r_in;
    shader.set_input_padding((focus_reach + fold_reach + 8.0).ceil());
    RenderEffect::runtime_shader(shader)
}

/// The text edit-menu material measured from the reference: a 44 dp glass
/// capsule of high transparency — weak backdrop blur (text behind stays
/// readable through the body), a whisper of dark tint, a ~2 px top rim
/// highlight and faint side rims. `progress` (0..1) materializes the
/// material: at 0 the glass is optically absent (the menu fades in as a
/// smudge that sharpens), at 1 it carries the full rim and tint.
/// `blur_radius_px` is the backdrop blur in physical px (density-scaled by
/// the caller; everything else is dp in explicit-rect mode).
pub fn liquid_menu_glass_effect(
    node_size: (f32, f32),
    blur_radius_px: f32,
    progress: f32,
) -> RenderEffect {
    let (w, h) = (node_size.0.max(1.0), node_size.1.max(1.0));
    let p = progress.clamp(0.0, 1.0);
    let mut shader = RuntimeShader::new(LIQUID_GLASS_WGSL);
    shader.set_float2(0, w, h); // container = node size dp
    shader.set_float2(2, w * 0.5, h * 0.5);
    shader.set_float2(4, w, h);
    shader.set_float(6, -1.0); // capsule
    shader.set_float(7, 8.0); // bezel dp
    shader.set_float(8, 2.5 * p); // subtle edge refraction (the reference
                                  // menu barely bends what grazes its edge)
    shader.set_float(9, 1.4);
    shader.set_float(10, 0.6);
    shader.set_float(11, 0.18 * p); // top rim highlight (calibrated: the
                                    // reference top spike is a subtle +33
                                    // luminance over the body)
    shader.set_float4(14, 0.0, 0.0, 0.0, 0.15 * p); // dark tint (reference
                                                    // dims backdrop ~24%)
    shader.set_float(18, 1.0 + 0.25 * p); // mild vibrancy
    shader.set_float(19, 0.0); // no dispersion on the menu
    shader.set_float(20, -0.09 * p); // dim bright backdrop content like the
                                     // reference (ghosts stay smudges)
    shader.set_float(88, 0.97); // bottom rim ~equal to the top (measured)
    shader.set_float(89, 0.6); // rim holds ~60% strength at the side tangents
    shader.set_float(21, 0.5);
    // Measured on captures: (0,1) puts the crisp arc on the TOP edge with
    // the 0.45x counter on the bottom — the reference hierarchy.
    shader.set_float2(22, 0.0, 1.0);
    shader.set_float(24, 1.0);
    shader.set_float(25, 0.5);
    shader.set_input_padding(12.0);
    let lens = RenderEffect::runtime_shader(shader);
    if blur_radius_px > 0.5 {
        // Full blur from the first fade frame: scaling the radius with the
        // fade left raw, crisp backdrop text showing through the half-faded
        // body — the reference smudges from the start.
        RenderEffect::blur(blur_radius_px).then(lens)
    } else {
        lens
    }
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
        assert_eq!(spec.refractive_index, 1.5);
        assert_eq!(spec.saturation, 1.0);
        assert_eq!(spec.chromatic_aberration, 0.0);
        assert_eq!(spec.lift, 0.0);
        assert_eq!(spec.contrast, 1.0);
        assert_eq!(spec.light_direction, (0.0, 1.0));
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
        let spec = LiquidGlassSpec {
            saturation: 1.6,
            chromatic_aberration: 0.4,
            lift: 0.12,
            contrast: 1.05,
            edge_band: 0.4,
            dither: 1.0,
            light_direction: (0.3, 0.7),
            ..LiquidGlassSpec::default()
        };
        let effect = liquid_glass_effect(&rect, &spec, 800.0, 600.0);
        let RenderEffect::Shader { shader } = effect else {
            panic!("expected Shader effect");
        };
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
        // material extension slots
        assert_eq!(u[18], 1.6);
        assert_eq!(u[19], 0.4);
        assert_eq!(u[20], 0.12);
        assert_eq!(u[21], 1.0);
        assert_eq!(u[22], 0.3);
        assert_eq!(u[23], 0.7);
        assert_eq!(u[24], 1.05);
        assert_eq!(u[25], 0.4);
    }

    #[test]
    fn liquid_glass_declares_padding_for_static_lensing() {
        // Even with no tilt the bezel lenses along its normal, so the backdrop
        // capture must extend past the rect.
        let spec = LiquidGlassSpec::default();
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
        let bend = 1.0 - 1.0 / spec.refractive_index;
        let min_padding = bend * spec.displacement_scale;
        assert!(
            shader.input_padding() >= min_padding,
            "static lensing must capture backdrop beyond the rect: {} < {min_padding}",
            shader.input_padding()
        );
    }

    #[test]
    fn liquid_glass_padding_covers_max_shader_displacement() {
        let spec = LiquidGlassSpec {
            tilt_angle: 0.5,
            tilt_pitch: 0.3,
            chromatic_aberration: 0.6,
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
        let tilt = (spec.tilt_angle * spec.tilt_angle + spec.tilt_pitch * spec.tilt_pitch).sqrt();
        let slope = 2.0 + 2.0 * spec.profile.clamp(0.0, 1.0);
        let spread = 1.0 + spec.chromatic_aberration * 0.5;
        let max_displacement = (1.0 + tilt) * bend * spec.displacement_scale * slope * spread;
        assert!(
            shader.input_padding() >= max_displacement,
            "backdrop capture must cover the largest refracted sample: {} < {max_displacement}",
            shader.input_padding()
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
