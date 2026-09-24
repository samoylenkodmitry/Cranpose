//! LiquidGlass effect: a refractive glass material rendered via RuntimeShader.
//!
//! An SDF rounded-rect lens over the backdrop using the wcKSRD source mapping,
//! blur, edge light, saturation, adaptive exposure, tint, and dither.
//!
//! The wcKSRD optical program samples both sharp and blurred rays from one
//! captured backdrop so displacement never reveals a second scene layer.

use std::{
    cell::RefCell,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::{
    Color, RenderEffect, RuntimeShader, SubstrateSpec, render_effect::ShaderSpecializationCache,
};

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
        flag: "GLASS_DIRECTIONAL_REFRACTION_OFF",
        slots: &[GLASS_REFRACTION_MODE_UNIFORM],
        inactive: |u| slot(u, GLASS_REFRACTION_MODE_UNIFORM) <= 0.5,
    },
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
        flag: "GLASS_PROJECTION_OFF",
        slots: &[168, 169],
        inactive: |u| {
            slot(u, 168) <= 0.0
                || slot(u, 169) <= 0.0
                || (slot(u, 168) == 1.0 && slot(u, 169) == 1.0)
        },
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
        flag: GLASS_PHYSICAL_REFRACTION_OFF_FLAG,
        slots: &[GLASS_PHYSICAL_REFRACTION_DEPTH_ENABLED_UNIFORM],
        inactive: |u| slot(u, GLASS_PHYSICAL_REFRACTION_DEPTH_ENABLED_UNIFORM) <= 0.5,
    },
    LiquidGlassSpecialization {
        flag: GLASS_DISPERSION_OFF_FLAG,
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
        flag: "GLASS_INTERIOR_GUARD",
        slots: &[],
        inactive: |_| true,
    },
];

/// Whether liquid glass compiles a pipeline per material.
///
/// [`specialize_liquid_glass`] can fold the features a material's uniforms
/// leave inactive into `override` constants and split its draw into an
/// interior and a rim. Every distinct fold set is its own pipeline, and a
/// pipeline is the backend's shader compiler run inside the frame that first
/// draws it: around a hundred milliseconds cold on Metal, which keeps no
/// compiled pipeline across launches. A page of thirty materials was sixty
/// compiles and seven seconds of stalls, one at every touch.
///
/// The folds save tile-based mobile GPUs dead ALU, which is where they were
/// measured to earn that. Everywhere else one pipeline per blend mode draws
/// the same picture -- every glass parity suite holds the folded and the
/// plain shader byte-identical -- so folding is on for Android and off for
/// the rest. [`set_glass_material_folds`] moves it for a measurement.
pub fn glass_material_folds_enabled() -> bool {
    GLASS_MATERIAL_FOLDS.load(Ordering::Relaxed)
}

/// Turns per-material folding on or off for this process.
pub fn set_glass_material_folds(enabled: bool) {
    GLASS_MATERIAL_FOLDS.store(enabled, Ordering::Relaxed);
}

static GLASS_MATERIAL_FOLDS: AtomicBool = AtomicBool::new(cfg!(target_os = "android"));

/// Specializes a `liquid_glass.wgsl` shader to its uniforms, folding where
/// [`glass_material_folds_enabled`] says to.
///
/// With folds on, every feature the uniforms leave inactive becomes a raised
/// `override` and the draw is split into interior and rim. Byte-exact: a
/// raised flag substitutes the value the uniform already holds, and the
/// interior guard skips only terms whose weight is zero. With folds off the
/// shader carries no flags and draws whole, from the one pipeline every
/// material shares. Either way an adaptive frost declares the blurred
/// substrate its neighbourhood reads whatever the activity: the declaration
/// also sets the member's capture geometry, so a resting material keeps it
/// although its shader returns before the read. A content mask (uniform 112)
/// returns its source under the glass silhouette and so declares that it
/// leaves a transparent source transparent.
pub fn specialize_liquid_glass(shader: &mut RuntimeShader) {
    specialize_liquid_glass_with_folds(shader, glass_material_folds_enabled());
}

/// [`specialize_liquid_glass`] with folding decided by the caller rather
/// than the process: a test of the folds asks for them whatever the platform.
pub fn specialize_liquid_glass_with_folds(shader: &mut RuntimeShader, folds: bool) {
    const _: () = assert!(LIQUID_GLASS_SPECIALIZATIONS.len() <= u32::BITS as usize);
    const CACHE_CAPACITY: usize = 32;
    type SpecializationKey = (
        u32,
        Option<u32>,
        bool,
        bool,
        bool,
        Option<u32>,
        u8,
        Option<u32>,
    );
    thread_local! {
        static CACHE: RefCell<ShaderSpecializationCache<SpecializationKey, CACHE_CAPACITY>> =
            const { RefCell::new(ShaderSpecializationCache::new()) };
    }
    let uniforms = shader.uniforms();
    let flags = if folds {
        LIQUID_GLASS_SPECIALIZATIONS
            .iter()
            .enumerate()
            .fold(0, |flags, (index, specialization)| {
                flags | (u32::from((specialization.inactive)(uniforms)) << index)
            })
    } else {
        0
    };
    let substrate_radius = (slot(uniforms, GLASS_ADAPTIVE_FROST_UNIFORM) > 0.0).then(|| {
        (GLASS_ADAPTIVE_NEIGHBOURHOOD_DP * slot(uniforms, GLASS_EFFECT_DENSITY_UNIFORM).max(1.0))
            .to_bits()
    });
    let mean_tone = slot(uniforms, GLASS_ADAPTIVE_TONE_UNIFORM) > 0.5;
    let pane_radius = (slot(uniforms, GLASS_PANE_BLEND_UNIFORM) > 0.0)
        .then(|| slot(uniforms, GLASS_PANE_BLEND_UNIFORM).to_bits());
    let projection = [
        slot(uniforms, GLASS_OPTICAL_PROJECTION_UNIFORM),
        slot(uniforms, GLASS_OPTICAL_PROJECTION_UNIFORM + 1),
    ];
    let projected = projection.iter().all(|v| *v > 0.0) && projection != [1.0, 1.0];
    let stage = slot(uniforms, GLASS_OPTICAL_STAGE_UNIFORM) as u8;
    let backdrop_radius = (stage == 2 && slot(uniforms, GLASS_BACKDROP_BLUR_UNIFORM) > 0.0)
        .then(|| slot(uniforms, GLASS_BACKDROP_BLUR_UNIFORM).to_bits());
    shader.set_preserves_transparency(slot(uniforms, 112) > 0.5);
    CACHE.with_borrow_mut(|cache| {
        cache.apply(
            shader,
            (
                flags,
                substrate_radius,
                folds,
                mean_tone,
                projected,
                pane_radius,
                stage,
                backdrop_radius,
            ),
            |shader, &(flags, radius, folds, mean_tone, projected, pane_radius, stage, backdrop_radius)| {
                for (index, specialization) in LIQUID_GLASS_SPECIALIZATIONS.iter().enumerate() {
                    if flags & (1 << index) != 0 {
                        shader.set_override(specialization.flag, 1.0);
                    } else {
                        shader.clear_override(specialization.flag);
                    }
                }
                shader.set_draw_split((folds && !projected).then_some(GLASS_RIM_DRAW_OVERRIDE));
                shader.set_specialization_exact(true);
                let substrate = radius.map(|radius| SubstrateSpec::Blur {
                    radius_px: f32::from_bits(radius),
                });
                let mut substrates = arrayvec::ArrayVec::<SubstrateSpec, 3>::new();
                substrates.extend(substrate);
                if mean_tone {
                    substrates.push(SubstrateSpec::Mean);
                }
                if let Some(radius) = pane_radius {
                    substrates.push(SubstrateSpec::Blur {
                        radius_px: f32::from_bits(radius),
                    });
                }
                if matches!(stage, 1 | 2) {
                    substrates.clear();
                    if let Some(radius) = backdrop_radius {
                        substrates.push(SubstrateSpec::Blur { radius_px: f32::from_bits(radius) });
                    }
                }
                shader.set_substrates(&substrates);
            },
        );
    });
}

/// The `override NAME: i32` of `liquid_glass.wgsl` the renderer sets to
/// draw the glass as its interior and its rim, each without the other's
/// fetches.
pub const GLASS_RIM_DRAW_OVERRIDE: &str = "GLASS_RIM_DRAW";

/// The fold raised when a material's dispersion is zero.
pub const GLASS_DISPERSION_OFF_FLAG: &str = "GLASS_DISPERSION_OFF";

/// The fold raised when a material's physical refraction is disabled.
pub const GLASS_PHYSICAL_REFRACTION_OFF_FLAG: &str = "GLASS_PHYSICAL_REFRACTION_OFF";

/// The reach of the adaptive frost's neighbourhood in dp: the renderer
/// blurs the capture by this radius at the effect's density and the shader
/// reads that substrate once where it sampled nine points this far apart.
pub const GLASS_ADAPTIVE_NEIGHBOURHOOD_DP: f32 = 16.0;

/// Wraps a fully configured `liquid_glass.wgsl` shader as a render effect,
/// specialized to the features its uniforms enable. Edge lenses render the
/// outer warp, inner warp, and chromatic lighting in three successive images.
/// Content masks and other refraction modes use one image.
pub fn liquid_glass_runtime_effect(shader: RuntimeShader) -> RenderEffect {
    if slot(shader.uniforms(), GLASS_REFRACTION_MODE_UNIFORM) >= 1.5
        && slot(shader.uniforms(), 112) <= 0.5
        && slot(shader.uniforms(), GLASS_OPTICAL_STAGE_UNIFORM) == 0.0
    {
        let stage = |index: u8| {
            let mut pass = shader.clone();
            pass.set_float(GLASS_OPTICAL_STAGE_UNIFORM, f32::from(index));
            if index < 3 {
                pass.set_output_support(None);
                pass.set_output_padding(0.0);
            }
            glass_shader_effect(pass)
        };
        stage(1).then(stage(2)).then(stage(3))
    } else {
        glass_shader_effect(shader)
    }
}

fn glass_shader_effect(mut shader: RuntimeShader) -> RenderEffect {
    specialize_liquid_glass(&mut shader);
    if matches!(
        slot(shader.uniforms(), GLASS_OPTICAL_STAGE_UNIFORM),
        1.0 | 2.0
    ) {
        shader.set_draw_split(None);
    }
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
/// lens (values <= 1 mean no zoom; its projection model controls the rim).
pub const GLASS_OPTICAL_ZOOM_UNIFORM: usize = 89;
/// Uniform slot (two floats) containing the optical-zoom axis offset from
/// the SDF center, in dp — a leaning lens magnifies about the content it
/// rides, not its shifted silhouette.
pub const GLASS_OPTICAL_ZOOM_ANCHOR_UNIFORM: usize = 128;
/// Uniform slot selecting radial (0), edge-normal surface (1), or outward edge lens (2) projection.
pub const GLASS_REFRACTION_MODE_UNIFORM: usize = 130;
/// Uniform slot containing ray reach in dp: inward for surface projection,
/// outward for the edge lens. The refraction mode selects the direction.
pub const GLASS_EDGE_REFRACTION_REACH_UNIFORM: usize = 131;
/// Uniform slot disabling diffuse face lighting when set to one; zero preserves it.
pub const GLASS_FACE_LIGHTING_OFF_UNIFORM: usize = 132;
/// Uniform slot containing the touch glow radius in dp; nonpositive values use 58 dp.
pub const GLASS_TOUCH_RADIUS_UNIFORM: usize = 133;
/// Whether the graphics layer supplies the active material silhouette coverage.
pub const GLASS_LAYER_CLIPPED_UNIFORM: usize = 134;
/// Seven consecutive floats for opposing edge lights: height in dp, linear falloff,
/// direction in radians, reflected chroma gain, luminance gain, additive offset,
/// and whether height follows the surface transform.
/// A zero height keeps the material's dome lighting.
pub const GLASS_KEY_FILL_UNIFORM: usize = 135;
/// Four consecutive floats for face response: attenuation, starting and ending
/// screen depths in dp, and illumination added after edge lighting.
pub const GLASS_FACE_RESPONSE_UNIFORM: usize = 142;
/// Primary capsule transition smoothing in dp; zero preserves the primary outline.
pub const GLASS_CAPSULE_SMOOTHING_UNIFORM: usize = 146;
/// Edge-lens image stage: zero requests the complete chain, one renders the outer warp,
/// two renders the inner warp, and three filters the resulting image and lights it.
pub const GLASS_OPTICAL_STAGE_UNIFORM: usize = 147;
/// Eight consecutive floats for a custom edge spectrum: angle, signed tap spacing
/// in dp, vertical scale, near and far opacity, fade depth and extent in dp, and enable flag.
pub const GLASS_EDGE_SPECTRUM_UNIFORM: usize = 148;

/// Eight uniform slots for inset shadow RGBA, radius, vertical offset, spread, and presence.
pub const GLASS_INNER_SHADOW_UNIFORM: usize = 156;

/// Two uniform slots for an independent inner return depth in dp and its presence.
pub const GLASS_EDGE_RETURN_DEPTH_UNIFORM: usize = 164;

/// Horizontal and vertical scale of the complete optical field about its primary center.
pub const GLASS_OPTICAL_PROJECTION_UNIFORM: usize = 168;
/// Enables the mean-backdrop face transfer curve; zero preserves the material's fixed tone.
pub const GLASS_ADAPTIVE_TONE_UNIFORM: usize = 166;
/// Pane blur radius in pixels and normal-blend tint fraction in the following slot.
pub const GLASS_PANE_BLEND_UNIFORM: usize = 170;
/// Radius in physical pixels and opacity of the blurred backdrop mixed with the outer edge warp.
pub const GLASS_BACKDROP_BLUR_UNIFORM: usize = 172;
/// Enables foreground insertion between the inner warp and chromatic pass. The inner
/// warp applies the face's tone before content; the final pass disperses and lights it.
pub const GLASS_FOREGROUND_CONTENT_UNIFORM: usize = 174;

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
pub const LIQUID_GLASS_WGSL: &str = concat!(
    include_str!("../shaders/glass_geometry.wgsl"),
    include_str!("../shaders/liquid_glass.wgsl"),
);

/// WGSL distance and circular displacement functions shared by glass and its content.
/// Concatenate this source once with the runtime shader prelude and a fragment stage.
pub const LIQUID_GLASS_GEOMETRY_WGSL: &str = include_str!("../shaders/glass_geometry.wgsl");

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
#[path = "tests/liquid_glass_tests.rs"]
mod tests;
