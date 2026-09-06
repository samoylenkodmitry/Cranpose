//! LiquidGlass effect: a refractive glass material rendered via RuntimeShader.
//!
//! An SDF rounded-rect lens over the backdrop using the wcKSRD source mapping,
//! blur, edge light, saturation, adaptive exposure, tint, and dither.
//!
//! The wcKSRD optical program samples both sharp and blurred rays from one
//! captured backdrop so displacement never reveals a second scene layer.

use crate::{Color, RenderEffect, RuntimeShader, SubstrateSpec};

/// One pipeline-overridable flag of `liquid_glass.wgsl` and the uniform
/// slots it folds away.
///
/// The shader gates each optional feature on a uniform; a raised flag
/// replaces that uniform read with the feature's inactive value, which is
/// the value the uniform holds when `inactive` reports true, so the
/// specialized pipeline computes exactly what the general one did and the
/// compiler removes the dead feature. A flag with no slots is an
/// optimization the reference pipeline leaves off and every material
/// raises: it skips only work whose result is exactly zero. See
/// [`specialize_liquid_glass`].
#[derive(Clone, Copy, Debug)]
pub struct LiquidGlassSpecialization {
    /// The `override NAME: bool` declared by the shader.
    pub flag: &'static str,
    /// Uniform slots the flag replaces.
    pub slots: &'static [usize],
    /// Whether the uniforms hold the feature's inactive value.
    pub inactive: fn(&[f32]) -> bool,
}

fn slot(uniforms: &[f32], index: usize) -> f32 {
    uniforms.get(index).copied().unwrap_or(0.0)
}

/// Every specialization flag of `liquid_glass.wgsl`, the single table the
/// shader's `override` declarations, [`specialize_liquid_glass`] and the
/// contract tests share.
pub const LIQUID_GLASS_SPECIALIZATIONS: &[LiquidGlassSpecialization] = &[
    LiquidGlassSpecialization {
        flag: "GLASS_LOUPE_OFF",
        slots: &[80],
        inactive: |u| slot(u, 80) == 0.0,
    },
    LiquidGlassSpecialization {
        flag: "GLASS_FOLD_OFF",
        slots: &[GLASS_FOLD_DEPTH_UNIFORM],
        inactive: |u| slot(u, GLASS_FOLD_DEPTH_UNIFORM) == 0.0,
    },
    LiquidGlassSpecialization {
        flag: "GLASS_SCENE_SHAPES_OFF",
        slots: &[30],
        inactive: |u| slot(u, 30) == 0.0,
    },
    LiquidGlassSpecialization {
        flag: "GLASS_WOBBLE_OFF",
        slots: &[32, 26],
        inactive: |u| slot(u, 32) == 0.0 && slot(u, 26) == 0.0,
    },
    LiquidGlassSpecialization {
        flag: "GLASS_ELLIPSE_BLEND_OFF",
        slots: &[110],
        inactive: |u| slot(u, 110) == 0.0,
    },
    LiquidGlassSpecialization {
        flag: "GLASS_STRAIN_OFF",
        slots: &[106, 107, 108, 109],
        inactive: |u| {
            let axis_identity = (slot(u, 106) == 0.0 && slot(u, 107) == 0.0)
                || (slot(u, 106) == 1.0 && slot(u, 107) == 0.0);
            let ratio_identity = slot(u, 108) <= 0.0
                || slot(u, 109) <= 0.0
                || (slot(u, 108) == 1.0 && slot(u, 109) == 1.0);
            axis_identity && ratio_identity
        },
    },
    LiquidGlassSpecialization {
        flag: "GLASS_ZOOM_ANCHOR_OFF",
        slots: &[
            GLASS_OPTICAL_ZOOM_ANCHOR_UNIFORM,
            GLASS_OPTICAL_ZOOM_ANCHOR_UNIFORM + 1,
        ],
        inactive: |u| {
            slot(u, GLASS_OPTICAL_ZOOM_ANCHOR_UNIFORM) == 0.0
                && slot(u, GLASS_OPTICAL_ZOOM_ANCHOR_UNIFORM + 1) == 0.0
        },
    },
    LiquidGlassSpecialization {
        flag: "GLASS_TOUCH_OFF",
        slots: &[120],
        inactive: |u| slot(u, 120) <= 0.0,
    },
    LiquidGlassSpecialization {
        flag: "GLASS_CONTENT_MASK_OFF",
        slots: &[112],
        inactive: |u| slot(u, 112) <= 0.5,
    },
    LiquidGlassSpecialization {
        flag: "GLASS_OPTICAL_BLUR_OFF",
        slots: &[GLASS_BLUR_RADIUS_UNIFORM],
        inactive: |u| slot(u, GLASS_BLUR_RADIUS_UNIFORM) <= 0.0,
    },
    LiquidGlassSpecialization {
        flag: "GLASS_SHADOW_OFF",
        slots: &[102],
        inactive: |u| slot(u, 102) <= 0.0,
    },
    LiquidGlassSpecialization {
        flag: "GLASS_ZOOM_OFF",
        slots: &[GLASS_OPTICAL_ZOOM_UNIFORM],
        inactive: |u| slot(u, GLASS_OPTICAL_ZOOM_UNIFORM) <= 1.0,
    },
    LiquidGlassSpecialization {
        flag: "GLASS_PHYSICAL_REFRACTION_OFF",
        slots: &[GLASS_PHYSICAL_REFRACTION_DEPTH_ENABLED_UNIFORM],
        inactive: |u| slot(u, GLASS_PHYSICAL_REFRACTION_DEPTH_ENABLED_UNIFORM) <= 0.5,
    },
    LiquidGlassSpecialization {
        flag: "GLASS_FULL_TRANSMISSION",
        slots: &[GLASS_TRANSMISSION_REFRACTION_UNIFORM],
        inactive: |u| slot(u, GLASS_TRANSMISSION_REFRACTION_UNIFORM) >= 1.0,
    },
    LiquidGlassSpecialization {
        flag: "GLASS_DISPERSION_OFF",
        slots: &[GLASS_DISPERSION_UNIFORM],
        inactive: |u| slot(u, GLASS_DISPERSION_UNIFORM) <= 0.0,
    },
    LiquidGlassSpecialization {
        flag: "GLASS_ADAPTIVE_FROST_OFF",
        slots: &[GLASS_ADAPTIVE_FROST_UNIFORM],
        inactive: |u| slot(u, GLASS_ADAPTIVE_FROST_UNIFORM) <= 0.0,
    },
    LiquidGlassSpecialization {
        flag: "GLASS_INK_OFF",
        slots: &[127],
        inactive: |u| slot(u, 127) <= 0.0,
    },
    LiquidGlassSpecialization {
        flag: "GLASS_RIM_STYLE_OFF",
        slots: &[GLASS_RIM_STYLE_UNIFORM],
        inactive: |u| slot(u, GLASS_RIM_STYLE_UNIFORM) <= 0.0,
    },
    LiquidGlassSpecialization {
        flag: "GLASS_RESTING_OFF",
        slots: &[GLASS_ACTIVITY_UNIFORM],
        inactive: |u| slot(u, GLASS_ACTIVITY_UNIFORM) >= 1.0,
    },
    LiquidGlassSpecialization {
        flag: "GLASS_REFRACTION_CURVE_DEFAULT",
        slots: &[GLASS_REFRACTION_CURVE_UNIFORM],
        inactive: |u| {
            let curve = slot(u, GLASS_REFRACTION_CURVE_UNIFORM);
            curve <= 0.0 || curve == GLASS_REFRACTION_CURVE_DEFAULT
        },
    },
    LiquidGlassSpecialization {
        flag: "GLASS_INTERIOR_GUARD",
        slots: &[],
        inactive: |_| true,
    },
];

/// Recomputes the specialization flags from the current uniforms, removing
/// overrides for features that have become active. The compiled material carries only
/// the features it uses. Byte-exact: a raised flag substitutes the value the
/// uniform already holds, and the interior guard skips only terms whose
/// weight is zero. An adaptive frost on an active material declares the
/// blurred substrate its neighbourhood reads; a resting material (activity
/// 0) returns before that read and declares none.
pub fn specialize_liquid_glass(shader: &mut RuntimeShader) {
    let uniforms: Vec<f32> = shader.uniforms().to_vec();
    for specialization in LIQUID_GLASS_SPECIALIZATIONS {
        if (specialization.inactive)(&uniforms) {
            shader.set_override(specialization.flag, 1.0);
        } else {
            shader.clear_override(specialization.flag);
        }
    }
    shader.set_draw_split(Some(GLASS_RIM_DRAW_OVERRIDE));
    let substrates = if slot(&uniforms, GLASS_ADAPTIVE_FROST_UNIFORM) > 0.0
        && slot(&uniforms, GLASS_ACTIVITY_UNIFORM) > 0.0
    {
        vec![SubstrateSpec::Blur {
            radius_px: GLASS_ADAPTIVE_NEIGHBOURHOOD_DP
                * slot(&uniforms, GLASS_EFFECT_DENSITY_UNIFORM).max(1.0),
        }]
    } else {
        Vec::new()
    };
    shader.set_substrates(substrates);
}

/// The `override NAME: i32` of `liquid_glass.wgsl` the renderer sets to
/// draw the glass as its interior and its rim, each without the other's
/// fetches.
pub const GLASS_RIM_DRAW_OVERRIDE: &str = "GLASS_RIM_DRAW";

/// The reach of the adaptive frost's neighbourhood in dp: the renderer
/// blurs the capture by this radius at the effect's density and the shader
/// reads that substrate once where it sampled nine points this far apart.
pub const GLASS_ADAPTIVE_NEIGHBOURHOOD_DP: f32 = 16.0;

/// Wraps a fully configured `liquid_glass.wgsl` shader as a render effect,
/// specialized to the features its uniforms enable.
pub fn liquid_glass_runtime_effect(mut shader: RuntimeShader) -> RenderEffect {
    specialize_liquid_glass(&mut shader);
    shader.set_batched_source(true);
    RenderEffect::runtime_shader(shader)
}

/// Uniform slot containing the adaptive frost strength; its neighbourhood
/// reads the substrate the renderer packs beside the source.
pub const GLASS_ADAPTIVE_FROST_UNIFORM: usize = 91;
/// Uniform slot containing wcKSRD-owned backdrop blur reach in physical pixels.
pub const GLASS_BLUR_RADIUS_UNIFORM: usize = 93;
/// Uniform slot containing the normalized wcKSRD ray-return exponent.
pub const GLASS_REFRACTION_CURVE_UNIFORM: usize = 94;
/// The ray-return exponent the shader uses when the slot is unset, and the
/// one value `GLASS_REFRACTION_CURVE_DEFAULT` folds; every other value stays
/// a uniform so an animated curve keys one pipeline.
pub const GLASS_REFRACTION_CURVE_DEFAULT: f32 = 0.25;
/// Uniform slot containing normalized wcKSRD spectral dispersion strength.
pub const GLASS_DISPERSION_UNIFORM: usize = 95;
/// Uniform slot controlling displacement of the transmitted backdrop path.
/// Reflected meniscus rays remain independent.
pub const GLASS_TRANSMISSION_REFRACTION_UNIFORM: usize = 96;
/// Uniform slot containing an optional physical wcKSRD refraction depth in
/// dp.
pub const GLASS_PHYSICAL_REFRACTION_DEPTH_UNIFORM: usize = 98;
/// Uniform slot containing px-per-dp for cover-mode optical bands.
pub const GLASS_EFFECT_DENSITY_UNIFORM: usize = 99;
/// Uniform slot controlling energy absorbed by the meniscus transmission
/// path. Reflection and spectral return remain independent.
pub const GLASS_MENISCUS_ABSORPTION_UNIFORM: usize = 100;
/// Uniform slot selecting physical refraction depth from slot 98 instead of
/// the normalized inradius-relative depth from slot 9.
pub const GLASS_PHYSICAL_REFRACTION_DEPTH_ENABLED_UNIFORM: usize = 101;
/// Uniform slot containing the interactive rim-fold band depth in dp (the
/// shader resolves it against the live shape inradius; zero = fold off).
pub const GLASS_FOLD_DEPTH_UNIFORM: usize = 88;
/// Uniform slot containing the uniform face magnification ratio of a riding
/// lens (values <= 1 mean no zoom; the rim band keeps the wcKSRD mapping).
pub const GLASS_OPTICAL_ZOOM_UNIFORM: usize = 89;
/// Uniform slot (two floats) containing the optical-zoom axis offset from
/// the SDF center, in dp — a leaning lens magnifies about the content it
/// rides, not its shifted silhouette.
pub const GLASS_OPTICAL_ZOOM_ANCHOR_UNIFORM: usize = 128;
/// Uniform slot selecting the rim style: 0 is the regular surface rim, 1
/// the lens rim whose meniscus reflects, transmits with loss and carries
/// the long-edge specular.
pub const GLASS_RIM_STYLE_UNIFORM: usize = 28;
/// Uniform slot containing continuous optical activity (identity at zero).
pub const GLASS_ACTIVITY_UNIFORM: usize = 111;
/// Uniform slot containing the base surface tint that remains when optical
/// activity reaches zero. The four consecutive floats are RGBA.
pub const GLASS_RESTING_TINT_UNIFORM: usize = 113;

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
///  98: physical wcKSRD refraction depth in dp
///  99: cover-mode px-per-dp for density-stable optical bands
/// 100: meniscus transmission absorption (0 = clear, 1 = full lens absorption)
/// 101: physical-refraction-depth selector (>0.5 = slot 98, else slot 9)
///  11: highlight intensity
///  14,15,16,17: tint color (r,g,b,a)
///  18: saturation (1.0 = unchanged)
///  20: lift (−1..1; screen-blend toward white / multiply toward black)
///  21: dither amount (0..1, in 1/255 steps)
///  24: contrast (1.0 = neutral; ≤0 treated as 1.0)
///  80: loupe mode (>0.5 replaces the lens terms with the drop optic)
///  81,82: loupe focus offset from the shape center (dp)
///  83: loupe center magnification (m0)
///  90: loupe optical activity (0 = identity, 1 = fully raised drop)
///  93: wcKSRD blur reach in physical pixels
/// 111: continuous optical activity (0 = exact backdrop identity, 1 = full)
/// 113..116: resting surface tint RGBA (transparent = no resting surface)
/// 122,123: ambient light return direction (screen-space vector; zero =
///          unset -> light overhead, return glow at the bottom rim)
/// 124..126: ink recolor RGB — the lens recolors dark transmitted ink
/// 127: ink recolor strength (0 = off)
pub const LIQUID_GLASS_WGSL: &str = include_str!("../shaders/liquid_glass.wgsl");

/// Uniform slot of the ambient light return direction (x at 122, y at 123).
pub const GLASS_LIGHT_DIRECTION_UNIFORM: usize = 122;

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
    /// Energy absorbed from the transmitted ray at the meniscus. This does
    /// not reduce the reflected or spectrally separated light paths.
    pub meniscus_absorption: f32,
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
            meniscus_absorption: 1.0,
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

    let cx = rect.left + rect.width * 0.5;
    let cy = rect.top + rect.height * 0.5;

    shader.set_float2(0, area_width, area_height);
    shader.set_float2(2, cx, cy);
    shader.set_float2(4, rect.width, rect.height);
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
    shader.set_float(
        GLASS_MENISCUS_ABSORPTION_UNIFORM,
        spec.meniscus_absorption.clamp(0.0, 1.0),
    );
    shader.set_float(GLASS_ACTIVITY_UNIFORM, 1.0);
    shader.set_input_padding(liquid_glass_input_padding(spec));

    liquid_glass_runtime_effect(shader)
}

/// How far the shader's refracted and internally reflected samples can reach
/// outside the effect rect.
fn liquid_glass_input_padding(spec: &LiquidGlassSpec) -> f32 {
    spec.blur_radius.max(2.0).ceil()
}

/// The text-drag loupe material: a solid glass drop magnifying an offset
/// focus (the grab point under the finger), displayed inside a capsule
/// floating above it. ONE continuous wcKSRD field (example/shaders.txt):
/// `sample = focus + p·lens_scale/m` — the magnified face, the rim's
/// descending-branch inversion and the rim line all come from the same
/// displacement mapping, with no band boundaries.
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidLoupeSpec {
    /// Magnification (the reference loupe measures a uniform ~1.25×).
    pub magnification: f32,
    /// Focus offset from the bubble center, dp (the reference samples 75 dp
    /// below its center: content from under the finger, displayed above).
    pub focus_offset: (f32, f32),
    /// Spectral separation of the meniscus return.
    pub dispersion: f32,
    /// Specular rim intensity.
    pub highlight: f32,
    /// Continuous optical activity. Geometry is owned by the caller; this
    /// coordinate raises and lowers refraction, magnification, dispersion,
    /// fold return, and edge light without cross-fading sampled content.
    pub activity: f32,
    /// Corner radius in dp. The text loupe follows the smaller half-extent as
    /// it grows, producing a narrow capsule at birth and the full horizontal
    /// capsule at rest. Values <= 0 select that capsule radius automatically.
    pub corner_radius: f32,
}

impl Default for LiquidLoupeSpec {
    fn default() -> Self {
        Self {
            magnification: 1.25,
            focus_offset: (0.0, 75.0),
            dispersion: 0.36,
            highlight: 0.42,
            activity: 1.0,
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
    let activity = spec.activity.clamp(0.0, 1.0);
    let mut shader = RuntimeShader::new(LIQUID_GLASS_WGSL);
    shader.set_float2(0, w, h);
    shader.set_float2(2, w * 0.5, h * 0.5);
    shader.set_float2(4, w, h);
    if spec.corner_radius > 0.0 {
        shader.set_float(6, spec.corner_radius.min(0.5 * h.min(w)));
    } else {
        shader.set_float(6, -1.0);
    }
    shader.set_float(9, 0.34 * activity);
    shader.set_float(GLASS_REFRACTION_CURVE_UNIFORM, 0.25);
    shader.set_float(
        GLASS_DISPERSION_UNIFORM,
        spec.dispersion.clamp(0.0, 1.0) * activity,
    );
    shader.set_float(GLASS_TRANSMISSION_REFRACTION_UNIFORM, 1.0);
    shader.set_float(GLASS_EFFECT_DENSITY_UNIFORM, 1.0);
    shader.set_float(GLASS_MENISCUS_ABSORPTION_UNIFORM, 1.0);
    shader.set_float(GLASS_ACTIVITY_UNIFORM, 1.0);
    shader.set_float(11, spec.highlight * activity);
    shader.set_float4(14, 1.0, 1.0, 1.0, 0.0);
    shader.set_float(18, 1.0);
    shader.set_float(20, 0.0);
    shader.set_float(21, 0.5);
    shader.set_float(24, 1.0);
    shader.set_float(28, activity);
    shader.set_float(80, 1.0);
    shader.set_float2(81, spec.focus_offset.0, spec.focus_offset.1);
    shader.set_float(83, 1.0 + (spec.magnification.max(0.2) - 1.0) * activity);
    shader.set_float(90, activity);
    let focus_reach = (spec.focus_offset.0.powi(2) + spec.focus_offset.1.powi(2)).sqrt();
    shader.set_input_padding((focus_reach + 8.0).ceil());
    liquid_glass_runtime_effect(shader)
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
    shader.set_float2(0, w, h);
    shader.set_float2(2, w * 0.5, h * 0.5);
    shader.set_float2(4, w, h);
    shader.set_float(6, -1.0);
    shader.set_float(9, 0.10 * p);
    shader.set_float(GLASS_REFRACTION_CURVE_UNIFORM, 0.25);
    shader.set_float(GLASS_TRANSMISSION_REFRACTION_UNIFORM, 1.0);
    shader.set_float(GLASS_EFFECT_DENSITY_UNIFORM, 1.0);
    shader.set_float(GLASS_ACTIVITY_UNIFORM, 1.0);
    shader.set_float(11, 0.19 * p);
    shader.set_float4(14, 0.0, 0.0, 0.0, 0.04 * p);
    shader.set_float(18, 1.0 + 0.10 * p);
    shader.set_float(20, -0.06 * p);
    shader.set_float(24, 1.0 + 0.05 * p);
    shader.set_float(21, 0.5);
    let requested_blur = if blur_radius_px > 0.5 {
        blur_radius_px * (1.0 - 0.4 * p)
    } else {
        0.0
    };
    const WCKSRD_OPTICAL_BLUR_RADIUS_PX: f32 = 2.0;
    let (wcksrd_blur, gaussian_blur) = if requested_blur > WCKSRD_OPTICAL_BLUR_RADIUS_PX {
        (0.0, requested_blur)
    } else {
        (requested_blur, 0.0)
    };
    shader.set_float(GLASS_BLUR_RADIUS_UNIFORM, wcksrd_blur);
    shader.set_input_padding(12.0 + requested_blur);
    let optical = liquid_glass_runtime_effect(shader);
    if gaussian_blur > f32::EPSILON {
        RenderEffect::blur_with_edge_treatment(gaussian_blur, crate::TileMode::Mirror).then(optical)
    } else {
        optical
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

    fn rect() -> LiquidGlassRect {
        LiquidGlassRect {
            left: 100.0,
            top: 50.0,
            width: 200.0,
            height: 100.0,
            tint_color: Color(0.5, 0.5, 1.0, 0.1),
        }
    }

    fn shader_lines_reading(slot: usize) -> Vec<&'static str> {
        let needles = [format!("get_float({slot}u)"), format!("get_vec2({slot}u)")];
        LIQUID_GLASS_WGSL
            .lines()
            .filter(|line| needles.iter().any(|needle| line.contains(needle)))
            .collect()
    }

    #[test]
    fn every_specialization_flag_is_a_shader_override_and_guards_all_of_its_slot_reads() {
        for specialization in LIQUID_GLASS_SPECIALIZATIONS {
            let declaration = format!("override {}: bool = false;", specialization.flag);
            assert!(
                LIQUID_GLASS_WGSL.contains(&declaration),
                "`{declaration}` missing from liquid_glass.wgsl"
            );
            let mut guarded_reads = 0;
            for slot in specialization.slots {
                for line in shader_lines_reading(*slot) {
                    assert!(
                        line.contains(specialization.flag),
                        "slot {slot} is read outside its `{}` guard: `{}`",
                        specialization.flag,
                        line.trim()
                    );
                    guarded_reads += 1;
                }
            }
            assert!(
                guarded_reads > 0 || specialization.slots.is_empty(),
                "`{}` guards no uniform read; the flag would fold nothing",
                specialization.flag
            );
        }
        let declared: Vec<&str> = LIQUID_GLASS_WGSL
            .lines()
            .filter_map(|line| line.strip_prefix("override "))
            .filter(|rest| rest.contains(": bool"))
            .filter_map(|rest| rest.split(':').next())
            .collect();
        for flag in &declared {
            assert!(
                LIQUID_GLASS_SPECIALIZATIONS
                    .iter()
                    .any(|specialization| specialization.flag == *flag),
                "shader override `{flag}` is missing from LIQUID_GLASS_SPECIALIZATIONS, so \
                 nothing ever raises it"
            );
        }
        assert_eq!(declared.len(), LIQUID_GLASS_SPECIALIZATIONS.len());
    }

    fn raised_flags(effect: &RenderEffect) -> Vec<&'static str> {
        let RenderEffect::Shader { shader } = effect else {
            panic!("liquid glass must be one runtime shader");
        };
        shader.overrides().iter().map(|(flag, _)| *flag).collect()
    }

    #[test]
    fn a_resting_glass_declares_no_substrate() {
        let mut shader = RuntimeShader::new(LIQUID_GLASS_WGSL);
        shader.set_float(GLASS_ADAPTIVE_FROST_UNIFORM, 0.42);
        shader.set_float(GLASS_EFFECT_DENSITY_UNIFORM, 2.0);
        shader.set_float(GLASS_ACTIVITY_UNIFORM, 0.0);
        specialize_liquid_glass(&mut shader);
        assert!(
            shader.substrates().is_empty(),
            "a resting glass returns before its adaptive block: {:?}",
            shader.substrates()
        );
        shader.set_float(GLASS_ACTIVITY_UNIFORM, 0.25);
        specialize_liquid_glass(&mut shader);
        assert_eq!(
            shader.substrates().len(),
            1,
            "any activity reads the substrate"
        );
    }

    #[test]
    fn only_the_default_refraction_curve_folds_and_other_values_share_one_key() {
        let hash_for = |curve: f32| {
            let mut shader = RuntimeShader::new(LIQUID_GLASS_WGSL);
            shader.set_float(GLASS_REFRACTION_CURVE_UNIFORM, curve);
            specialize_liquid_glass(&mut shader);
            let folded = shader
                .overrides()
                .iter()
                .any(|(flag, _)| *flag == "GLASS_REFRACTION_CURVE_DEFAULT");
            (folded, shader.overrides_hash())
        };
        assert!(hash_for(0.25).0, "the default folds");
        assert!(hash_for(0.0).0, "an unset slot is the default");
        let (folded, animated_a) = hash_for(0.26);
        assert!(!folded, "any other value stays a uniform");
        let (_, animated_b) = hash_for(0.27);
        let (_, card) = hash_for(0.62);
        assert_eq!(
            animated_a, animated_b,
            "an animated curve keys one pipeline"
        );
        assert_eq!(animated_a, card);
        assert_ne!(animated_a, hash_for(0.25).1);
    }

    #[test]
    fn a_plain_pane_raises_every_flag() {
        let flags = raised_flags(&liquid_glass_effect(
            &rect(),
            &LiquidGlassSpec::default(),
            800.0,
            600.0,
        ));
        let every_flag: Vec<&str> = LIQUID_GLASS_SPECIALIZATIONS
            .iter()
            .map(|specialization| specialization.flag)
            .collect();
        let mut sorted = every_flag.clone();
        sorted.sort_unstable();
        assert_eq!(
            flags, sorted,
            "a plain pane uses no optional feature, so every flag folds"
        );
    }

    #[test]
    fn respecializing_mutated_uniforms_matches_fresh_shader_and_preserves_caller_override() {
        let mut shader = RuntimeShader::new(LIQUID_GLASS_WGSL);
        shader.set_override("CALLER_OVERRIDE", 7.0);
        specialize_liquid_glass(&mut shader);
        shader.set_float(GLASS_RIM_STYLE_UNIFORM, 1.0);
        specialize_liquid_glass(&mut shader);

        let mut fresh = RuntimeShader::new(LIQUID_GLASS_WGSL);
        fresh.set_override("CALLER_OVERRIDE", 7.0);
        fresh.set_float(GLASS_RIM_STYLE_UNIFORM, 1.0);
        specialize_liquid_glass(&mut fresh);
        assert_eq!(shader.overrides(), fresh.overrides());
        assert_eq!(shader.overrides_hash(), fresh.overrides_hash());
        assert!(shader.overrides().contains(&("CALLER_OVERRIDE", 7.0)));

        shader.set_float(GLASS_RIM_STYLE_UNIFORM, 0.0);
        specialize_liquid_glass(&mut shader);
        let mut inactive = RuntimeShader::new(LIQUID_GLASS_WGSL);
        inactive.set_override("CALLER_OVERRIDE", 7.0);
        specialize_liquid_glass(&mut inactive);
        assert_eq!(shader.overrides(), inactive.overrides());
        assert_eq!(shader.overrides_hash(), inactive.overrides_hash());
        assert!(shader.overrides().contains(&("CALLER_OVERRIDE", 7.0)));
    }

    #[test]
    fn a_blurred_dispersive_loupe_keeps_its_features_live() {
        let flags = raised_flags(&liquid_loupe_effect(
            (200.0, 120.0),
            &LiquidLoupeSpec::default(),
        ));
        for live in ["GLASS_LOUPE_OFF", "GLASS_DISPERSION_OFF"] {
            assert!(
                !flags.contains(&live),
                "{live} must stay live for a loupe: {flags:?}"
            );
        }
        assert!(flags.contains(&"GLASS_FOLD_OFF"));
        assert!(flags.contains(&"GLASS_SCENE_SHAPES_OFF"));

        let blurred = liquid_glass_effect(
            &rect(),
            &LiquidGlassSpec {
                blur_radius: 1.5,
                ..LiquidGlassSpec::default()
            },
            800.0,
            600.0,
        );
        assert!(
            !raised_flags(&blurred).contains(&"GLASS_OPTICAL_BLUR_OFF"),
            "an optical blur radius keeps the wcKSRD footprint live"
        );
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
        assert_eq!(spec.meniscus_absorption, 1.0);
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
            meniscus_absorption: 0.3,
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
        assert_eq!(uniforms[GLASS_MENISCUS_ABSORPTION_UNIFORM], 0.3);
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
        let loupe_spec = LiquidLoupeSpec::default();
        let RenderEffect::Shader { shader: loupe } =
            liquid_loupe_effect((117.0, 82.0), &loupe_spec)
        else {
            panic!("loupe must use the shared runtime shader");
        };
        assert_eq!(loupe.uniforms()[9], 0.34);
        assert_eq!(loupe.uniforms()[GLASS_REFRACTION_CURVE_UNIFORM], 0.25);
        assert_eq!(
            loupe.uniforms()[GLASS_DISPERSION_UNIFORM],
            loupe_spec.dispersion
        );
        assert_eq!(loupe.uniforms()[80], 1.0);
        assert!(loupe.input_padding() >= 75.0);

        let RenderEffect::Chain { first, second } =
            liquid_menu_glass_effect((240.0, 44.0), 8.0, 1.0)
        else {
            panic!("a heavy settled blur must chain a Gaussian into the shader");
        };
        let RenderEffect::Blur { radius_x, .. } = *first else {
            panic!("the chain's first stage is the Gaussian remainder");
        };
        assert!(radius_x > 0.0);
        let RenderEffect::Shader { shader: menu } = *second else {
            panic!("the chain's second stage is the wcKSRD program");
        };
        assert_eq!(menu.uniforms()[9], 0.10);
        assert_eq!(menu.uniforms()[GLASS_REFRACTION_CURVE_UNIFORM], 0.25);
        assert!(menu.uniforms()[GLASS_BLUR_RADIUS_UNIFORM] <= 2.0 + 1.0e-6);
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
