//! The material of a tray and its controls: the scene's light with a highlight and a rim.
//!
//! The scene's `Lit` material shades by the angle of the light alone, so a rounded edge looks the
//! same as a flat face. This shader adds the light's highlight, which moves with the eye and marks
//! the rounded edges, and a faint rim where a surface turns away from the eye. A control that a ray
//! rests on glows a little. The scene builds one pipeline for the shader, which all trays share.

use std::sync::{Arc, OnceLock};

use cranpose::space::{Material, Shaded};

static SATIN: OnceLock<Arc<str>> = OnceLock::new();

// The shader's values: the highlight's sharpness and strength, the rim's strength and the glow
const SATIN_WGSL: &str = "
fn shade(surface: ShadedSurface) -> vec4<f32> {
    let normal = surface.normal;
    let facing_light = max(dot(normal, surface.to_light), 0.0) * surface.lit;
    let light = surface.ambient + (1.0 - surface.ambient) * facing_light;
    let half_way = normalize(surface.to_light + surface.to_eye);
    let highlight = pow(max(dot(normal, half_way), 0.0), shaded_param(0u)) * shaded_param(1u) * surface.lit;
    let rim = pow(1.0 - max(dot(normal, surface.to_eye), 0.0), 3.0) * shaded_param(2u);
    let glow = shaded_param(3u);
    let color = surface.color.rgb * (light + glow) + vec3<f32>(highlight + rim) * surface.color.a;
    return vec4<f32>(color, surface.color.a);
}
";

// How shiny a surface is.
#[derive(Clone, Copy)]
pub(crate) enum Finish {
    // A control's face: a small bright highlight
    Glossy,
    // The tray's base and a slider's groove: a wide soft highlight
    Matte,
}

// The material of a surface with `finish` that glows by `glow`, from 0 for none.
pub(crate) fn satin(finish: Finish, glow: f32) -> Material {
    let (sharpness, strength, rim) = match finish {
        Finish::Glossy => (48.0, 0.45, 0.18),
        Finish::Matte => (12.0, 0.12, 0.08),
    };
    let mut params = [0.0; Shaded::PARAMS];
    params[..4].copy_from_slice(&[sharpness, strength, rim, glow]);
    Material::Shaded(Shaded {
        shader: Arc::clone(SATIN.get_or_init(|| Arc::from(SATIN_WGSL))),
        params,
    })
}
