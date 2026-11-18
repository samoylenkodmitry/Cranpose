//! The text-drag loupe: a glass drop of `liquid_glass.wgsl` that magnifies
//! the content under the finger.

use crate::{
    GLASS_ACTIVITY_UNIFORM, GLASS_DISPERSION_UNIFORM, GLASS_EFFECT_DENSITY_UNIFORM,
    GLASS_MENISCUS_ABSORPTION_UNIFORM, GLASS_REFRACTION_CURVE_UNIFORM,
    GLASS_TRANSMISSION_REFRACTION_UNIFORM, LIQUID_GLASS_WGSL, RenderEffect, RuntimeShader,
    liquid_glass_runtime_effect,
};

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
