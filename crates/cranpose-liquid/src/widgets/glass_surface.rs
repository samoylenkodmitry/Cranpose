use cranpose_macros::composable;
use cranpose_ui::{
    Modifier,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::Color;
use cranpose_ui_layout::Alignment;

use crate::material::{
    Glass, GlassDynamics, GlassFaceResponse, GlassFaceTone, GlassKeyFill, GlassRefraction,
    GlassShadow, GlassSpecularHighlight, LiquidModifierExt,
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

pub(crate) fn floating_material(foreground: Color, height: f32) -> Glass {
    let dark = foreground.r() * 0.2126 + foreground.g() * 0.7152 + foreground.b() * 0.0722 > 0.5;
    let mut glass = control_surface_material(foreground)
        .blur_radius(0.0)
        .tint_amount(
            crate::appearance::GlassTintAmount::from_percent(50.0).expect("valid glass fill"),
        )
        .pane_blur_radius(32.0)
        .refraction_depth_dp(16.0)
        .face_tone(GlassFaceTone {
            black: if dark { 0.125 } else { 0.4 },
            white: if dark { 1.125 } else { 1.03 },
            saturation: if dark { 1.3 } else { 1.2 },
            max_luminance: if dark { 0.6 } else { 0.94 },
        });
    glass.refraction = GlassRefraction::LayeredSurface {
        reach_dp: 0.0,
        return_reach_dp: height * 0.5,
        return_depth_dp: height * 0.25,
    };
    glass.face_response = None;
    glass.specular_highlight = Some(GlassSpecularHighlight {
        height_dp: 1.0,
        angle_radians: std::f32::consts::FRAC_PI_2,
        spread_radians: 80.0_f32.to_radians(),
        curvature: 0.75,
        diffuse_amount: 0.15,
        diffuse_height_dp: 8.0,
        diffuse_spread_radians: 52.0_f32.to_radians(),
        saturation: 1.1765,
        luma_gain: 0.9118,
        color_bias: 0.1471,
    });
    glass
}

pub(crate) fn floating_dynamics(
    size: cranpose_ui_graphics::Size,
    expansion: f32,
    press: f32,
    center: (f32, f32),
) -> GlassDynamics {
    let height = size.height;
    let lift = if size.width > 0.0 {
        1.0 + 16.0 * expansion / size.width
    } else {
        1.0
    };
    GlassDynamics {
        morph: Some(crate::material::GlassMorph {
            node_size: (size.width, size.height),
            primary: (
                size.width * 0.5,
                size.height * 0.5,
                size.width,
                size.height,
                -1.0,
            ),
            ..Default::default()
        }),
        contact_lighting: true,
        touch: Some((center.0, center.1, press.clamp(0.0, 1.0))),
        touch_radius_dp: Some(height * 0.75),
        refraction: Some(GlassRefraction::LayeredSurface {
            reach_dp: 0.0,
            return_reach_dp: height * 0.5 / lift,
            return_depth_dp: height * 0.25 / lift,
        }),
        ring_shadow: Some((
            GlassShadow::new(Color::BLACK.with_alpha(0.06), 5.0, 8.0, 0.0),
            4.0,
        )),
        ..Default::default()
    }
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
