use cranpose_macros::composable;
use cranpose_ui::{
    Modifier,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::Color;
use cranpose_ui_layout::Alignment;

use crate::material::{
    Glass, GlassFaceResponse, GlassFaceTone, GlassKeyFill, GlassShadow, LiquidModifierExt,
};

pub(crate) fn surface_material(foreground: cranpose_ui_graphics::Color) -> Glass {
    Glass::regular()
        .face_lighting(false)
        .key_fill(GlassKeyFill {
            height_dp: 1.0,
            curvature: 0.7,
            angle_radians: std::f32::consts::FRAC_PI_4,
            saturation: 1.5,
            luma_gain: 0.1,
            offset: 0.9,
            scale_with_surface: false,
        })
        .face_response(GlassFaceResponse {
            gain: 0.97,
            start_dp: 1.0 / 3.0,
            end_dp: 2.0 / 3.0,
            illumination: 0.0,
        })
        .tint(Color::TRANSPARENT)
        .blur_radius(6.0)
        .surface_refraction(31.0)
        .refraction_depth_dp(15.5)
        .transmission_refraction(1.0)
        .saturation(1.0)
        .lift(0.0)
        .contrast(1.0)
        .highlight(1.0)
        .adaptive_frost(foreground, 0.0)
        .adaptive_tone(foreground)
        .shadow_style(GlassShadow::new(
            Color::BLACK.with_alpha(0.162),
            32.0,
            8.0,
            1.5,
        ))
}

pub(crate) fn control_surface_material(foreground: Color) -> Glass {
    control_material(foreground, false)
}

pub(crate) fn card_surface_material(foreground: Color) -> Glass {
    control_material(foreground, true)
}

fn control_material(foreground: Color, card: bool) -> Glass {
    let dark = foreground.r() * 0.2126 + foreground.g() * 0.7152 + foreground.b() * 0.0722 > 0.5;
    surface_material(foreground)
        .key_fill(GlassKeyFill {
            height_dp: 0.533_333_36,
            curvature: 1.0,
            angle_radians: std::f32::consts::FRAC_PI_2,
            saturation: 1.0,
            luma_gain: 1.0,
            offset: -0.3,
            scale_with_surface: false,
        })
        .highlight(0.4)
        .face_tone(GlassFaceTone {
            black: if dark {
                0.125
            } else if card {
                0.434
            } else {
                0.41
            },
            white: if dark {
                1.125
            } else if card {
                1.2
            } else {
                1.055
            },
            saturation: if dark {
                1.3
            } else if card {
                0.6
            } else {
                1.0
            },
            max_luminance: if dark {
                if card { 0.35 } else { 0.6 }
            } else if card {
                0.838
            } else {
                0.96
            },
        })
        .tint(Color::WHITE.with_alpha(if dark { 0.0 } else { 0.2 }))
        .blur_radius(10.0)
        .shadow_style(GlassShadow::new(
            Color::BLACK.with_alpha(if card { 0.12 } else { 0.012 }),
            if card { 32.0 } else { 5.333_333_5 },
            8.0,
            0.0,
        ))
}

/// A container rendered on the Liquid Glass material: its backdrop is
/// refracted/blurred behind `content`, clipped to `glass.shape`. `modifier`
/// applies to the whole surface, glass included, as a Compose modifier does:
/// its offset moves the glass and its padding is space around it.
///
/// ```ignore
/// GlassSurface(Modifier::empty().fill_max_width(), Glass::regular(), || {
///     Text("On glass", Modifier::empty().padding(12.0), body_style);
/// });
/// ```
#[composable]
pub fn GlassSurface(modifier: Modifier, glass: Glass, content: impl FnMut() + 'static) {
    let modifier = modifier.glass_effect(glass);
    Box(
        modifier,
        BoxSpec::default().content_alignment(Alignment::CENTER),
        content,
    );
}
