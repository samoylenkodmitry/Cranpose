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

use crate::{Color, GlassProfileCurve, GlassSurfaceProfile, RenderEffect, RuntimeShader};

/// Uniform slot containing whether the physical surface has non-zero depth.
pub const GLASS_SURFACE_ENABLED_UNIFORM: usize = 113;
/// Uniform slot containing the superellipse radial power.
pub const GLASS_SURFACE_RADIAL_POWER_UNIFORM: usize = 114;
/// Uniform slot containing the X-Z profile knot count.
pub const GLASS_SURFACE_X_COUNT_UNIFORM: usize = 115;
/// Uniform slot containing the Y-Z profile knot count.
pub const GLASS_SURFACE_Y_COUNT_UNIFORM: usize = 116;
/// Uniform slot containing the physical profile depth in logical pixels.
pub const GLASS_SURFACE_DEPTH_UNIFORM: usize = 117;
/// First uniform slot containing X-Z `(position, height, tangent)` knots.
pub const GLASS_SURFACE_X_UNIFORM_BASE: usize = 120;
/// First uniform slot containing Y-Z `(position, height, tangent)` knots.
pub const GLASS_SURFACE_Y_UNIFORM_BASE: usize = 140;
/// Uniform slot containing the oval-to-toric principal-axis coupling.
pub const GLASS_SURFACE_AXIS_COUPLING_UNIFORM: usize = 158;

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
///  11: highlight intensity
///  12,13: tilt (x, y) — motion-driven displacement direction
///  14,15,16,17: tint color (r,g,b,a)
///  18: saturation (1.0 = unchanged)
///  19: chromatic aberration spread (relative, 0 = off)
///  20: lift (−1..1; screen-blend toward white / multiply toward black)
///  21: dither amount (0..1, in 1/255 steps)
///  22,23: specular light direction ((0,1) lights the top edge)
///  24: contrast (1.0 = neutral; ≤0 treated as 1.0)
///  80: loupe mode (>0.5 replaces the lens terms with the drop optic)
///  81,82: loupe focus offset from the shape center (dp)
///  83: loupe center magnification (m0)
///  84: loupe band start (depth fraction 0..1 where the rim fold begins)
///  85: loupe fold peak (sampling reach at the fold crest, in inradius units)
///  86: loupe band dispersion strength
/// 113: physical surface enabled
/// 114: oval/superellipse radial power
/// 115,116: X-Z and Y-Z knot counts
/// 117: physical surface depth dp
/// 118,119: principal-axis spectral response (X-Z, Y-Z)
/// 120..137: X-Z knots as `(position, height, tangent)`
/// 140..157: Y-Z knots as `(position, height, tangent)`
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
    /// Physical X-Z/Y-Z cross-sections of the glass surface.
    pub surface_profile: GlassSurfaceProfile,
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
    /// Principal-axis wavelength response for the X-Z and Y-Z profiles.
    pub dispersion_axes: (f32, f32),
    /// Scheme lift: positive screen-blends toward white (light scheme),
    /// negative multiplies toward black (dark scheme). Screen keeps the
    /// backdrop ghosts colored, unlike an alpha mix.
    pub lift: f32,
    /// Contrast pivot around mid-gray (1.0 = neutral).
    pub contrast: f32,
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
            surface_profile: GlassSurfaceProfile::regular(),
            highlight: 0.7,
            tilt_angle: 0.0,
            tilt_pitch: 0.0,
            saturation: 1.0,
            chromatic_aberration: 0.0,
            dispersion_axes: (1.0, 1.0),
            lift: 0.0,
            contrast: 1.0,
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
    shader.set_float2(118, spec.dispersion_axes.0, spec.dispersion_axes.1);
    shader.set_float(20, spec.lift);
    shader.set_float(21, spec.dither);
    shader.set_float2(22, spec.light_direction.0, spec.light_direction.1);
    shader.set_float(24, spec.contrast);
    apply_glass_surface_profile(&mut shader, spec.surface_profile, 1.0);
    shader.set_input_padding(liquid_glass_input_padding(spec, rect.width, rect.height));

    RenderEffect::runtime_shader(shader)
}

/// How far the shader's refracted samples can reach outside the effect rect —
/// the backdrop capture must cover it. The displacement is
/// `(normal + tilt) * bend * scale` with the chromatic-aberration spread on
/// top; `|normal| = 1`, so the static lens contributes even with zero tilt.
fn liquid_glass_input_padding(spec: &LiquidGlassSpec, width: f32, height: f32) -> f32 {
    let bend = 1.0 - 1.0 / spec.refractive_index.max(1.0001);
    let tilt = (spec.tilt_angle * spec.tilt_angle + spec.tilt_pitch * spec.tilt_pitch).sqrt();
    let half_extent = 0.5 * width.min(height).max(1.0);
    let slope = glass_surface_max_slope(spec.surface_profile, half_extent);
    let reach = slope + tilt;
    let axis_scale = spec.dispersion_axes.0.max(spec.dispersion_axes.1);
    let spread = 1.0 + axis_scale * spec.chromatic_aberration.max(0.0) * 0.5;
    let displacement = reach * bend * spec.displacement_scale.max(0.0) * slope * spread;
    if displacement > 0.0 {
        displacement.ceil() + 2.0
    } else {
        0.0
    }
}

/// Conservative maximum physical slope of a surface whose smaller half
/// extent is `half_extent`.
pub fn glass_surface_max_slope(profile: GlassSurfaceProfile, half_extent: f32) -> f32 {
    let x_slope = curve_max_abs_tangent(profile.x_profile());
    let y_slope = curve_max_abs_tangent(profile.y_profile());
    // The second term conservatively covers the derivative of the X/Y blend
    // weight on strongly non-circular superellipses.
    let blend_slope = profile.radial_power() * 0.5;
    (x_slope.max(y_slope) + blend_slope) * profile.depth() / half_extent.max(1.0)
}

fn curve_max_abs_tangent(profile: GlassProfileCurve) -> f32 {
    (0..=64)
        .map(|step| profile.evaluate(step as f32 / 64.0).1.abs())
        .fold(0.0, f32::max)
}

/// Packs one physical surface definition into the shared liquid-glass shader
/// layout. `depth_scale` converts the authored logical depth into the shader's
/// geometry units without changing the cross-sections.
pub fn apply_glass_surface_profile(
    shader: &mut RuntimeShader,
    profile: GlassSurfaceProfile,
    depth_scale: f32,
) {
    let depth = profile.depth() * depth_scale.max(0.0);
    shader.set_float(
        GLASS_SURFACE_ENABLED_UNIFORM,
        if depth > f32::EPSILON { 1.0 } else { 0.0 },
    );
    shader.set_float(GLASS_SURFACE_RADIAL_POWER_UNIFORM, profile.radial_power());
    shader.set_float(
        GLASS_SURFACE_X_COUNT_UNIFORM,
        profile.x_profile().knots().len() as f32,
    );
    shader.set_float(
        GLASS_SURFACE_Y_COUNT_UNIFORM,
        profile.y_profile().knots().len() as f32,
    );
    shader.set_float(GLASS_SURFACE_DEPTH_UNIFORM, depth);
    shader.set_float(GLASS_SURFACE_AXIS_COUPLING_UNIFORM, profile.axis_coupling());
    pack_profile_curve(shader, GLASS_SURFACE_X_UNIFORM_BASE, profile.x_profile());
    pack_profile_curve(shader, GLASS_SURFACE_Y_UNIFORM_BASE, profile.y_profile());
}

fn pack_profile_curve(shader: &mut RuntimeShader, base: usize, profile: GlassProfileCurve) {
    for (index, knot) in profile.knots().iter().enumerate() {
        let offset = base + index * 3;
        shader.set_float(offset, knot.position());
        shader.set_float(offset + 1, knot.height());
        shader.set_float(offset + 2, knot.tangent());
    }
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
    /// Dispersion strength inside the band (RGB fringes on the folded rim)
    /// and across the rim ring's side arcs.
    pub dispersion: f32,
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
            // The reference fringes measure 3-5 px (at 3x) IN the band and
            // RAMP continuously along mirrored strokes (R-B splitting from
            // ~10 to ~50-60 channel units tip-to-tip). 0.15 left the
            // x-chroma sub-pixel through most of the band — strokes read
            // rigid/un-fringed until the rim.
            dispersion: 0.20,
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
    // dispersion never animate. `progress` is the CONTENT ALPHA — the
    // dissolve blends the whole lens output toward the plain backdrop
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
    shader.set_float(7, 0.5 * h.min(w)); // bezel = inradius (sheen falloff)
    shader.set_float(11, spec.highlight);
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
    shader.set_float(83, spec.magnification);
    shader.set_float(84, spec.band_start);
    shader.set_float(85, spec.fold_peak);
    shader.set_float(86, spec.dispersion);
    shader.set_float(87, spec.seam_lift);
    shader.set_float(90, alpha.max(1.0e-3));
    shader.set_float2(118, 1.0, 1.0);
    apply_glass_surface_profile(&mut shader, GlassSurfaceProfile::lens(), 1.0);
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
    shader.set_float(11, 0.19 * p); // rim intensity (the reference settled
                                    // pill peaks ~x1.9 of its baseline on
                                    // BOTH long edges)
                                    // Settled material (measured on the reference still: white text behind
                                    // the pill reads ~242/255 through it, the dark card dims ~x0.78): a
                                    // WHISPER of dark tint plus a mild contrast pivot — not the heavy
                                    // dim+lift that flattened ghosts into an opaque-looking fill.
    shader.set_float4(14, 0.0, 0.0, 0.0, 0.04 * p);
    shader.set_float(18, 1.0 + 0.10 * p); // mild vibrancy
    shader.set_float(19, 0.0); // no dispersion on the menu
    shader.set_float(20, -0.06 * p);
    shader.set_float(24, 1.0 + 0.05 * p); // gentle contrast pivot
    shader.set_float(88, 0.7); // bottom rim clearly softer than the top
    shader.set_float(89, 0.45); // rim holds ~45% strength off the lit arc
    shader.set_float(21, 0.5);
    // Measured on captures: (0,1) puts the crisp arc on the TOP edge with
    // the 0.45x counter on the bottom — the reference hierarchy.
    shader.set_float2(22, 0.0, 1.0);
    shader.set_float2(118, 1.0, 1.0);
    apply_glass_surface_profile(
        &mut shader,
        GlassSurfaceProfile::regular()
            .with_depth(2.4)
            .expect("menu surface depth is valid"),
        1.0,
    );
    shader.set_input_padding(12.0);
    let lens = RenderEffect::runtime_shader(shader);
    if blur_radius_px > 0.5 {
        // The reference RESOLVES its blur through the materialize: the
        // half-faded body is a strong smudge that sharpens to a near-clear
        // settled pill (white text behind reads ~242/255 through it). A
        // whisper of frost remains at rest — a flat tint reads as paint,
        // not glass.
        let radius = (blur_radius_px * (1.0 - p)).max(blur_radius_px * 0.08);
        RenderEffect::blur(radius).then(lens)
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
    fn physical_surface_blends_oval_and_biconic_axis_profiles() {
        assert!(LIQUID_GLASS_WGSL.contains("fn sample_profile"));
        assert!(LIQUID_GLASS_WGSL.contains("fn sample_glass_surface"));
        assert!(LIQUID_GLASS_WGSL.contains("let y_weight = b / q"));
        assert!(LIQUID_GLASS_WGSL.contains("let profile_delta = y_sample.height - x_sample.height"));
        assert!(LIQUID_GLASS_WGSL.contains("profile_delta * dw_du"));
        assert!(LIQUID_GLASS_WGSL.contains("profile_delta * dw_dv"));
        assert!(LIQUID_GLASS_WGSL.contains("let axis_coupling = clamp(get_float(158u)"));
        assert!(LIQUID_GLASS_WGSL.contains("let coupling_weight = axis_coupling;"));
        assert!(LIQUID_GLASS_WGSL.contains("let toric_height ="));
        assert!(
            LIQUID_GLASS_WGSL.contains("let height = oval_height + height_delta * coupling_weight")
        );
        assert!(LIQUID_GLASS_WGSL.contains("(toric_dz_du - oval_dz_du) * coupling_weight"));
        assert!(LIQUID_GLASS_WGSL.contains("(toric_dz_dv - oval_dz_dv) * coupling_weight"));
        assert!(!LIQUID_GLASS_WGSL.contains("coupling_gradient"));
        assert!(!LIQUID_GLASS_WGSL.contains("recessed_face_gradient"));
        assert!(!LIQUID_GLASS_WGSL.contains("d_height_dx"));
    }

    #[test]
    fn one_surface_normal_drives_refraction_dispersion_and_lighting() {
        assert!(LIQUID_GLASS_WGSL.contains("let optical_gradient = surface_gradient + tilt * 0.18"));
        assert!(LIQUID_GLASS_WGSL.contains("let surface_normal = normalize"));
        assert!(
            LIQUID_GLASS_WGSL.contains("base_displacement = -optical_gradient * bend * disp_scale")
        );
        assert!(LIQUID_GLASS_WGSL.contains("spectral_displacement = base_displacement"));
        assert!(
            LIQUID_GLASS_WGSL.contains("base_displacement + spectral_displacement * (scale - 1.0)")
        );
        assert!(LIQUID_GLASS_WGSL.contains("dot(surface_normal, half_direction)"));
        assert!(!LIQUID_GLASS_WGSL.contains("inner_contour"));
        assert!(!LIQUID_GLASS_WGSL.contains("magnify != 1.0"));
    }

    #[test]
    fn returning_meniscus_uses_normal_driven_transmission_loss() {
        assert!(LIQUID_GLASS_WGSL.contains("let return_transmission ="));
        assert!(LIQUID_GLASS_WGSL.contains("return_transmission * mix(0.008, 0.11, rim_style)"));
        assert!(!LIQUID_GLASS_WGSL.contains("painted_ridge"));
    }

    #[test]
    fn content_recolor_detects_bounded_broad_glyphs_at_multiple_scales() {
        assert!(LIQUID_GLASS_WGSL.contains("fn sampled_luma"));
        assert!(LIQUID_GLASS_WGSL.contains("let detail_step = vec2<f32>(4.5 * s)"));
        assert!(LIQUID_GLASS_WGSL.contains("let support_step = vec2<f32>(18.0 * s)"));
        assert!(LIQUID_GLASS_WGSL.contains("let bounded_dark_region"));
        assert!(
            LIQUID_GLASS_WGSL.contains("max(edge_detail, max(dark_stroke, bounded_dark_region))")
        );
        assert!(
            LIQUID_GLASS_WGSL.contains("let recolor_face = mix(1.0, 1.0 - smoothstep(0.82, 0.98")
        );
        assert!(!LIQUID_GLASS_WGSL.contains("let recolor_face = mix(1.0, surface_interior"));
        let recolor = LIQUID_GLASS_WGSL
            .find("if content_detail > 0.0")
            .expect("content recolor stage");
        let tone = LIQUID_GLASS_WGSL
            .find("// Tone pipeline")
            .expect("tone stage");
        let frost = LIQUID_GLASS_WGSL
            .find("// Adaptive frost")
            .expect("adaptive frost stage");
        assert!(recolor < tone && tone < frost);
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
            dispersion_axes: (0.25, 1.75),
            lift: 0.12,
            contrast: 1.05,
            dither: 1.0,
            light_direction: (0.3, 0.7),
            surface_profile: GlassSurfaceProfile::lens()
                .with_axis_coupling(0.35)
                .expect("axis coupling"),
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
        assert_eq!(u[113], 1.0);
        assert_eq!(u[114], spec.surface_profile.radial_power());
        assert_eq!(
            u[115],
            spec.surface_profile.x_profile().knots().len() as f32
        );
        assert_eq!(
            u[116],
            spec.surface_profile.y_profile().knots().len() as f32
        );
        assert_eq!(u[117], spec.surface_profile.depth());
        assert_eq!(&u[118..120], &[0.25, 1.75]);
        assert_eq!(u[158], 0.35);
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
        let slope = glass_surface_max_slope(spec.surface_profile, 50.0);
        let min_padding = bend * spec.displacement_scale * slope;
        assert!(
            shader.input_padding() >= min_padding,
            "static lensing must capture backdrop beyond the rect: {} < {min_padding}",
            shader.input_padding()
        );
    }

    #[test]
    fn surface_slope_scales_with_depth_and_inverse_extent() {
        let profile = GlassSurfaceProfile::lens();
        let base = glass_surface_max_slope(profile, 20.0);
        let deep = glass_surface_max_slope(
            profile.with_depth(profile.depth() * 2.0).expect("depth"),
            20.0,
        );
        let wide = glass_surface_max_slope(profile, 40.0);
        assert!((deep / base - 2.0).abs() < 1.0e-5);
        assert!((wide / base - 0.5).abs() < 1.0e-5);
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
        let slope = glass_surface_max_slope(spec.surface_profile, 50.0);
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
