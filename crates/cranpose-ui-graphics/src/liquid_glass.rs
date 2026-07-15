//! LiquidGlass effect: a refractive glass material rendered via RuntimeShader.
//!
//! An SDF rounded-rect lens over the backdrop using the wcKSRD source mapping,
//! blur, edge light, saturation, adaptive exposure, tint, and dither.
//!
//! The wcKSRD optical program samples both sharp and blurred rays from one
//! captured backdrop so displacement never reveals a second scene layer.

use crate::{Color, RenderEffect, RuntimeShader};

/// Uniform slot containing wcKSRD-owned backdrop blur reach in physical pixels.
pub const GLASS_BLUR_RADIUS_UNIFORM: usize = 93;
/// Uniform slot containing the normalized wcKSRD ray-return exponent.
pub const GLASS_REFRACTION_CURVE_UNIFORM: usize = 94;
/// Uniform slot containing normalized wcKSRD spectral dispersion strength.
pub const GLASS_DISPERSION_UNIFORM: usize = 95;
/// Uniform slot controlling displacement of the transmitted backdrop path.
/// Reflected meniscus rays remain independent.
pub const GLASS_TRANSMISSION_REFRACTION_UNIFORM: usize = 96;
/// Uniform slot containing px-per-dp for cover-mode optical bands.
pub const GLASS_EFFECT_DENSITY_UNIFORM: usize = 99;
/// Uniform slot containing continuous optical activity (identity at zero).
pub const GLASS_ACTIVITY_UNIFORM: usize = 111;

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
///   9: wcKSRD refraction depth as a fraction of the shape inradius
///  94: wcKSRD refraction curve exponent (0.05..1.0)
///  95: wcKSRD spectral dispersion strength (0..1)
///  96: transmitted-path refraction strength (0 = fixed backdrop coordinates)
///  99: cover-mode px-per-dp for density-stable optical bands
///  11: highlight intensity
///  14,15,16,17: tint color (r,g,b,a)
///  18: saturation (1.0 = unchanged)
///  20: lift (−1..1; screen-blend toward white / multiply toward black)
///  21: dither amount (0..1, in 1/255 steps)
///  24: contrast (1.0 = neutral; ≤0 treated as 1.0)
///  80: loupe mode (>0.5 replaces the lens terms with the drop optic)
///  81,82: loupe focus offset from the shape center (dp)
///  83: loupe center magnification (m0)
///  84: loupe band start (depth fraction 0..1 where the rim fold begins)
///  85: loupe fold peak (sampling reach at the fold crest, in inradius units)
///  93: wcKSRD blur reach in physical pixels
/// 111: continuous optical activity (0 = exact backdrop identity, 1 = full)
pub const LIQUID_GLASS_WGSL: &str = include_str!("../shaders/liquid_glass.wgsl");

/// Configuration for the LiquidGlass effect.
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidGlassSpec {
    /// Corner radius of the glass rounded rect, in dp.
    pub corner_radius: f32,
    /// wcKSRD refraction depth as a fraction of the shape inradius.
    pub refraction_depth: f32,
    /// wcKSRD ray-return exponent. Lower values return to local sampling
    /// quickly; 1.0 preserves the mirrored fold across the full depth.
    pub refraction_curve: f32,
    /// Backdrop blur reach evaluated by wcKSRD.
    pub blur_radius: f32,
    /// Specular highlight intensity.
    pub highlight: f32,
    /// Saturation/vibrancy multiplier applied to the refracted backdrop.
    pub saturation: f32,
    /// Scheme lift: positive screen-blends toward white (light scheme),
    /// negative multiplies toward black (dark scheme). Screen keeps the
    /// backdrop ghosts colored, unlike an alpha mix.
    pub lift: f32,
    /// Contrast pivot around mid-gray (1.0 = neutral).
    pub contrast: f32,
    /// Anti-banding dither amount (0..1, in 1/255 steps).
    pub dither: f32,
}

impl Default for LiquidGlassSpec {
    fn default() -> Self {
        Self {
            corner_radius: 28.0,
            refraction_depth: 0.34,
            refraction_curve: 0.25,
            blur_radius: 0.0,
            highlight: 0.7,
            saturation: 1.0,
            lift: 0.0,
            contrast: 1.0,
            dither: 0.5,
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
    shader.set_float(9, spec.refraction_depth.clamp(0.0, 2.0));
    shader.set_float(
        GLASS_REFRACTION_CURVE_UNIFORM,
        spec.refraction_curve.clamp(0.05, 1.0),
    );
    shader.set_float(11, spec.highlight);
    shader.set_float4(
        14,
        rect.tint_color.r(),
        rect.tint_color.g(),
        rect.tint_color.b(),
        rect.tint_color.a(),
    );
    shader.set_float(18, spec.saturation);
    shader.set_float(20, spec.lift);
    shader.set_float(21, spec.dither);
    shader.set_float(24, spec.contrast);
    shader.set_float(GLASS_BLUR_RADIUS_UNIFORM, spec.blur_radius.max(0.0));
    shader.set_float(GLASS_TRANSMISSION_REFRACTION_UNIFORM, 1.0);
    shader.set_float(GLASS_EFFECT_DENSITY_UNIFORM, 1.0);
    shader.set_float(GLASS_ACTIVITY_UNIFORM, 1.0);
    shader.set_input_padding(liquid_glass_input_padding(spec));

    RenderEffect::runtime_shader(shader)
}

/// How far the shader's refracted and internally reflected samples can reach
/// outside the effect rect.
fn liquid_glass_input_padding(spec: &LiquidGlassSpec) -> f32 {
    spec.blur_radius.max(2.0).ceil()
}

/// The text-drag loupe material: a solid glass drop magnifying an offset
/// focus (the grab point under the finger), displayed inside a capsule
/// floating above it. Measured against the reference recording:
/// dome magnification (`magnification` at the center easing to exactly 1
/// where the rim band starts), a rim FOLD that paints an inverted compressed
/// image of the content just beyond the bubble and the thin wcKSRD rim line.
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
    /// The fold floor: the bottom band never re-displays content nearer the
    /// focus line than this clearance (dp). The dragged handle's dot hangs
    /// just below the line; the caller sets this past the dot's bottom so
    /// the mirror shows the next line, never a second pink lobe.
    pub seam_lift: f32,
    /// Specular rim intensity.
    pub highlight: f32,
    /// Content alpha (0..1): 1 while the lens lives (grow included — the
    /// optics never animate); the dissolve lowers it, blending the whole
    /// lens output toward the plain backdrop before the terminal vanish.
    pub progress: f32,
    /// Corner radius (dp). The newborn reference is a flat-topped SQUIRCLE,
    /// not a circle: the caller passes ~0.38·height at birth, morphing to
    /// the capsule's half-height as the width fills out. <= 0 = capsule.
    pub corner_radius: f32,
}

impl Default for LiquidLoupeSpec {
    fn default() -> Self {
        Self {
            magnification: 1.25,
            focus_offset: (0.0, 75.0),
            // The fold occupies the outer 40% of the long-edge depth. The center
            // handle occludes it while the remaining band mirrors the next
            // line near 1:1.
            band_start: 0.60,
            fold_peak: 0.80,
            seam_lift: 26.0,
            // The reference rim reads as a clear bright line around the whole
            // capsule (peak ~+127 luminance over the backdrop); the
            // interactive-lens rim gain is a whisper, so the loupe drives it
            // through its highlight (calibrated on captures).
            highlight: 6.2,
            progress: 1.0,
            corner_radius: 0.0,
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
    // The lens is FIXED-OPTIC through its whole life: magnification, rim and
    // `progress` is the CONTENT ALPHA — the dissolve blends the whole lens
    // output toward the plain backdrop
    // (uniform 90), which is how the reference reads translucent mid-fade
    // while its magnified glyphs stay magnified.
    let alpha = spec.progress.clamp(0.0, 1.0);
    let mut shader = RuntimeShader::new(LIQUID_GLASS_WGSL);
    shader.set_float2(0, w, h); // container = node size dp
    shader.set_float2(2, w * 0.5, h * 0.5); // capsule centered in the node
    shader.set_float2(4, w, h);
    if spec.corner_radius > 0.0 {
        shader.set_float(6, spec.corner_radius.min(0.5 * h.min(w)));
    } else {
        shader.set_float(6, -1.0); // capsule radius sentinel
    }
    shader.set_float(9, 0.34);
    shader.set_float(GLASS_REFRACTION_CURVE_UNIFORM, 0.25);
    shader.set_float(GLASS_DISPERSION_UNIFORM, 1.0);
    shader.set_float(GLASS_TRANSMISSION_REFRACTION_UNIFORM, 1.0);
    shader.set_float(GLASS_EFFECT_DENSITY_UNIFORM, 1.0);
    shader.set_float(GLASS_ACTIVITY_UNIFORM, 1.0);
    shader.set_float(11, spec.highlight);
    shader.set_float4(14, 1.0, 1.0, 1.0, 0.0); // no tint
    shader.set_float(18, 1.0); // saturation neutral
    shader.set_float(20, 0.0); // no lift
    shader.set_float(21, 0.5); // dither
    shader.set_float(24, 1.0); // contrast neutral
    shader.set_float(28, 1.0); // interactive-lens rim style
    shader.set_float(80, 1.0); // loupe mode
    shader.set_float2(81, spec.focus_offset.0, spec.focus_offset.1);
    shader.set_float(83, spec.magnification);
    shader.set_float(84, spec.band_start);
    shader.set_float(85, spec.fold_peak);
    shader.set_float(87, spec.seam_lift);
    shader.set_float(90, alpha.max(1.0e-3));
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
    shader.set_float(9, 0.10 * p);
    shader.set_float(GLASS_REFRACTION_CURVE_UNIFORM, 0.25);
    shader.set_float(GLASS_TRANSMISSION_REFRACTION_UNIFORM, 1.0);
    shader.set_float(GLASS_EFFECT_DENSITY_UNIFORM, 1.0);
    shader.set_float(GLASS_ACTIVITY_UNIFORM, 1.0);
    shader.set_float(11, 0.19 * p); // rim intensity (the reference settled
                                    // pill peaks ~x1.9 of its baseline on
                                    // BOTH long edges)
                                    // Settled material (measured on the reference still: white text behind
                                    // the pill reads ~242/255 through it, the dark card dims ~x0.78): a
                                    // WHISPER of dark tint plus a mild contrast pivot — not the heavy
                                    // dim+lift that flattened ghosts into an opaque-looking fill.
    shader.set_float4(14, 0.0, 0.0, 0.0, 0.04 * p);
    shader.set_float(18, 1.0 + 0.10 * p); // mild vibrancy
    shader.set_float(20, -0.06 * p);
    shader.set_float(24, 1.0 + 0.05 * p); // gentle contrast pivot
    shader.set_float(21, 0.5);
    let blur_radius = if blur_radius_px > 0.5 {
        (blur_radius_px * (1.0 - p)).max(blur_radius_px * 0.08)
    } else {
        0.0
    };
    shader.set_float(GLASS_BLUR_RADIUS_UNIFORM, blur_radius);
    shader.set_input_padding(12.0 + blur_radius);
    RenderEffect::runtime_shader(shader)
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

    fn rect() -> LiquidGlassRect {
        LiquidGlassRect {
            left: 100.0,
            top: 50.0,
            width: 200.0,
            height: 100.0,
            tint_color: Color(0.5, 0.5, 1.0, 0.1),
        }
    }

    #[test]
    fn liquid_glass_spec_defaults_match_wcksrd() {
        let spec = LiquidGlassSpec::default();
        assert_eq!(spec.corner_radius, 28.0);
        assert_eq!(spec.refraction_depth, 0.34);
        assert_eq!(spec.refraction_curve, 0.25);
        assert_eq!(spec.blur_radius, 0.0);
        assert_eq!(spec.saturation, 1.0);
        assert_eq!(spec.lift, 0.0);
        assert_eq!(spec.contrast, 1.0);
    }

    #[test]
    fn liquid_glass_effect_packs_the_single_optical_program() {
        let spec = LiquidGlassSpec {
            refraction_depth: 0.72,
            refraction_curve: 0.8,
            blur_radius: 6.0,
            saturation: 1.6,
            lift: 0.12,
            contrast: 1.05,
            dither: 1.0,
            ..LiquidGlassSpec::default()
        };
        let RenderEffect::Shader { shader } = liquid_glass_effect(&rect(), &spec, 800.0, 600.0)
        else {
            panic!("liquid glass must be one runtime shader");
        };
        let uniforms = shader.uniforms();
        assert_eq!(&uniforms[0..6], &[800.0, 600.0, 200.0, 100.0, 200.0, 100.0]);
        assert_eq!(uniforms[9], 0.72);
        assert_eq!(uniforms[GLASS_REFRACTION_CURVE_UNIFORM], 0.8);
        assert_eq!(uniforms[GLASS_TRANSMISSION_REFRACTION_UNIFORM], 1.0);
        assert_eq!(uniforms[GLASS_EFFECT_DENSITY_UNIFORM], 1.0);
        assert_eq!(uniforms[18], 1.6);
        assert_eq!(uniforms[20], 0.12);
        assert_eq!(uniforms[21], 1.0);
        assert_eq!(uniforms[24], 1.05);
        assert_eq!(uniforms[GLASS_BLUR_RADIUS_UNIFORM], 6.0);
        assert_eq!(shader.input_padding(), 6.0);
    }

    #[test]
    fn refraction_depth_is_clamped_at_the_shader_boundary() {
        for (input, expected) in [(-1.0, 0.0), (0.8, 0.8), (3.0, 2.0)] {
            let spec = LiquidGlassSpec {
                refraction_depth: input,
                ..LiquidGlassSpec::default()
            };
            let RenderEffect::Shader { shader } = liquid_glass_effect(&rect(), &spec, 800.0, 600.0)
            else {
                panic!("liquid glass must be one runtime shader");
            };
            assert_eq!(shader.uniforms()[9], expected);
        }
    }

    #[test]
    fn refraction_curve_is_clamped_at_the_shader_boundary() {
        for (input, expected) in [(-1.0, 0.05), (0.8, 0.8), (3.0, 1.0)] {
            let spec = LiquidGlassSpec {
                refraction_curve: input,
                ..LiquidGlassSpec::default()
            };
            let RenderEffect::Shader { shader } = liquid_glass_effect(&rect(), &spec, 800.0, 600.0)
            else {
                panic!("liquid glass must be one runtime shader");
            };
            assert_eq!(shader.uniforms()[GLASS_REFRACTION_CURVE_UNIFORM], expected);
        }
    }

    #[test]
    fn loupe_and_menu_use_the_same_wcksrd_program() {
        let RenderEffect::Shader { shader: loupe } =
            liquid_loupe_effect((117.0, 82.0), &LiquidLoupeSpec::default())
        else {
            panic!("loupe must use the shared runtime shader");
        };
        assert_eq!(loupe.uniforms()[9], 0.34);
        assert_eq!(loupe.uniforms()[GLASS_REFRACTION_CURVE_UNIFORM], 0.25);
        assert_eq!(loupe.uniforms()[GLASS_DISPERSION_UNIFORM], 1.0);
        assert_eq!(loupe.uniforms()[80], 1.0);
        assert!(loupe.input_padding() >= 75.0);

        let RenderEffect::Shader { shader: menu } =
            liquid_menu_glass_effect((240.0, 44.0), 8.0, 1.0)
        else {
            panic!("menu must use the shared runtime shader");
        };
        assert_eq!(menu.uniforms()[9], 0.10);
        assert_eq!(menu.uniforms()[GLASS_REFRACTION_CURVE_UNIFORM], 0.25);
        assert!((menu.uniforms()[GLASS_BLUR_RADIUS_UNIFORM] - 0.64).abs() < 1.0e-6);
    }

    #[test]
    fn liquid_glass_effect_multi_handles_empty_and_multiple_rects() {
        let spec = LiquidGlassSpec::default();
        assert!(liquid_glass_effect_multi(&[], &spec, 800.0, 600.0).is_none());
        let rects = [rect(), rect()];
        assert!(matches!(
            liquid_glass_effect_multi(&rects, &spec, 800.0, 600.0),
            Some(RenderEffect::Chain { .. })
        ));
    }
}
