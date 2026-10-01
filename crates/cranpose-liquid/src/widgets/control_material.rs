use std::sync::{Once, OnceLock};

use cranpose_ui_graphics::{
    Color, RUNTIME_SHADER_PRELUDE_WGSL, RenderEffect, RuntimeShader, ShaderTarget, ShaderWarmUp,
    Size,
};

use super::control_motion::ControlContactKind;
use crate::material::{
    Glass, GlassContourHighlight, GlassDynamics, GlassFaceTone, GlassKeyFill, GlassRefraction,
    GlassShadow, LiquidShape,
};

fn profile(kind: ControlContactKind) -> (f32, f32, f32, f32, f32) {
    match kind {
        ControlContactKind::Thumb => (11.5, 6.9, 0.1, 0.0018, 0.15),
        ControlContactKind::Segment => (6.6, 4.4, 0.0, 0.001, 0.1),
    }
}

pub(super) fn material(kind: ControlContactKind) -> Glass {
    static WARM_UP: Once = Once::new();
    WARM_UP.call_once(|| {
        cranpose_ui_graphics::request_shader_warm_ups([
            ShaderWarmUp {
                shader: shader(false),
                target: ShaderTarget::Layer,
            },
            ShaderWarmUp {
                shader: shader(true),
                target: ShaderTarget::Page,
            },
        ]);
    });
    let (surface_reach, surface_depth, black, shadow_opacity, _) = profile(kind);
    let mut glass = Glass::lens()
        .shape(LiquidShape::Capsule)
        .resting_edge_sharpness(1.0)
        .tint(Color::TRANSPARENT)
        .blur_radius(0.0)
        .saturation(1.0)
        .refraction_depth_dp(36.0)
        .transmission_refraction(1.0)
        .dispersion(0.0)
        .face_lighting(false)
        .key_fill(GlassKeyFill {
            height_dp: 1.0,
            curvature: 0.75,
            angle_radians: 0.0,
            saturation: 1.1765,
            luma_gain: 0.9118,
            offset: 0.1471,
            scale_with_surface: true,
        })
        .face_tone(GlassFaceTone {
            black,
            white: 1.0,
            saturation: 1.0,
            max_luminance: 1.0,
        })
        .highlight(f32::from(kind == ControlContactKind::Thumb))
        .lift(0.0)
        .shadow_style(GlassShadow::new(
            Color::BLACK.with_alpha(shadow_opacity),
            8.0,
            7.0,
            0.0,
        ))
        .no_clip();
    glass.refraction = GlassRefraction::LayeredSurface {
        reach_dp: 9.0,
        return_reach_dp: surface_reach,
        return_depth_dp: surface_depth,
    };
    glass.contour_highlight = Some(GlassContourHighlight {
        height_dp: 1.0,
        offset_dp: -2.0 / 3.0,
        amount: 0.5,
        color_bias: -0.3,
        angle_radians: std::f32::consts::FRAC_PI_2,
        spread_radians: std::f32::consts::FRAC_PI_2,
    });
    glass
}

pub(super) fn dynamics(kind: ControlContactKind, geometry: f32, material: f32) -> GlassDynamics {
    let (reach, depth, _, shadow_opacity, ring_opacity) = profile(kind);
    let material = material.max(0.0);
    let fade = material.min(1.0);
    let rest_shadow = if kind == ControlContactKind::Thumb {
        0.1
    } else {
        0.0
    };
    GlassDynamics {
        activity: Some(material),
        refraction: Some(GlassRefraction::LayeredSurface {
            reach_dp: match kind {
                ControlContactKind::Thumb => 50.0 - 41.0 * geometry,
                ControlContactKind::Segment => 9.0 * geometry.max(0.0),
            },
            return_reach_dp: reach * material,
            return_depth_dp: depth * material,
        }),
        shadow: Some(GlassShadow::new(
            Color::BLACK.with_alpha(rest_shadow * (1.0 - fade) + shadow_opacity * fade),
            2.0 + 6.0 * fade,
            0.5 + 6.5 * fade,
            0.0,
        )),
        ring_shadow: Some((
            GlassShadow::new(Color::BLACK.with_alpha(ring_opacity * fade), 3.0, 8.0, 0.0),
            4.0,
        )),
        ..Default::default()
    }
}

pub(super) fn content_effect(
    size: Size,
    center: (f32, f32),
    lens_size: Size,
    projection: (f32, f32),
    activity: f32,
) -> Option<RenderEffect> {
    effect(size, center, lens_size, projection, activity, false)
}

pub(super) fn foreground_effect(
    size: Size,
    lens_size: Size,
    projection: (f32, f32),
    activity: f32,
) -> Option<RenderEffect> {
    effect(
        size,
        (size.width * 0.5, size.height * 0.5),
        lens_size,
        projection,
        activity,
        true,
    )
}

fn effect(
    size: Size,
    center: (f32, f32),
    lens_size: Size,
    projection: (f32, f32),
    activity: f32,
    foreground: bool,
) -> Option<RenderEffect> {
    if activity <= 0.0 {
        return None;
    }
    let mut shader = shader(foreground);
    shader.set_float4(0, size.width, size.height, center.0, center.1);
    shader.set_float4(
        4,
        lens_size.width,
        lens_size.height,
        projection.0,
        projection.1,
    );
    shader.set_float(8, activity);
    Some(RenderEffect::runtime_shader(shader))
}

fn shader(foreground: bool) -> RuntimeShader {
    static SHADER: OnceLock<RuntimeShader> = OnceLock::new();
    let mut shader = SHADER
        .get_or_init(|| {
            RuntimeShader::new(&format!(
                "{RUNTIME_SHADER_PRELUDE_WGSL}\n{}\n{}",
                cranpose_ui_graphics::LIQUID_GLASS_GEOMETRY_WGSL,
                include_str!("control_content.wgsl"),
            ))
        })
        .clone();
    shader.set_override("CONTROL_FOREGROUND", f64::from(foreground));
    shader
}
