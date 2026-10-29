use std::sync::{Mutex, PoisonError};

use cranpose_ui::{Color, Size};
use cranpose_ui_graphics::{RenderEffect, ShaderTarget, ShaderWarmUp};

use super::tab_bar::{BAR_HEIGHT, BLOB_HEIGHT, TAB_WIDTH};
use crate::{
    material::{
        Glass, GlassDynamics, GlassFaceResponse, GlassKeyFill, GlassMorph, GlassShadow,
        cached_glass_content_layers, neutral_surface_tint,
    },
    theme::LiquidColors,
};

const FLIGHT_LENS_INFLATION: f32 = 16.0;

pub(super) fn tab_flight_lens_material(foreground: Color, activity: f32) -> Glass {
    let activity = activity.clamp(0.0, 1.0);
    Glass::lens()
        .face_lighting(false)
        .key_fill(GlassKeyFill {
            height_dp: 1.0,
            curvature: 0.7 + 0.1 * activity,
            angle_radians: std::f32::consts::FRAC_PI_4,
            saturation: 1.5,
            luma_gain: 0.4367,
            offset: 1.125,
            scale_with_surface: true,
        })
        .no_clip()
        .face_response(GlassFaceResponse {
            gain: 1.0 - 0.03 * activity,
            start_dp: 1.0 / 3.0,
            end_dp: 2.0 / 3.0,
            illumination: 0.0,
        })
        .tint(neutral_surface_tint(
            foreground,
            0.07333333 * (1.0 - activity),
            0.1440678 * (1.0 - activity),
        ))
        .blur_radius(0.0)
        .backdrop_blur(4.0, 1.0 - activity)
        .edge_refraction(9.0 * activity)
        .refraction_depth_dp(36.0)
        .refraction_curve(0.25)
        .optical_zoom(1.0)
        .meniscus_absorption(0.0)
        .inner_shadow(GlassShadow::new(
            Color::BLACK.with_alpha(0.12 * activity),
            3.0,
            7.0,
            0.0,
        ))
        .fold_depth(0.0)
        .edge_spectrum(crate::material::GlassSpectrum {
            angle_radians: std::f32::consts::FRAC_PI_2
                - std::f32::consts::PI * 7.0 / 12.0 * activity,
            step_dp: (4.068 / (112.24138 / 36.0)) / (std::f32::consts::PI / 12.0).cos()
                * (-5.0 + 8.684211 * activity)
                / 3.6842105,
            vertical_scale: 0.8265,
            opacity_near: activity,
            opacity_far: 1.0 - activity,
            fade_depth_dp: 14.0 * activity,
            extent_dp: 38.88889 * activity,
        })
        .dispersion(0.0)
        .lift(0.0)
        .highlight(activity)
        .rim_reflection(activity)
        .shadow_style(GlassShadow::new(
            Color::BLACK.with_alpha(0.1 * activity),
            8.0,
            7.0,
            0.0,
        ))
}

pub(super) fn tab_flight_tint_multiplier(activity: f32) -> f32 {
    1.0 - 0.25 * activity.clamp(0.0, 1.0)
}

pub(super) fn accessory_surfaces_touch(edge_gap: f32) -> bool {
    edge_gap <= 0.0
}

pub(super) fn tab_lens_base_size(cell_width: f32, activity: f32) -> (f32, f32) {
    let inflation = FLIGHT_LENS_INFLATION * activity.clamp(0.0, 1.0);
    (cell_width + inflation, BLOB_HEIGHT + inflation)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TabFlightGeometry {
    pub(super) center: (f32, f32),
    pub(super) base_size: Size,
    pub(super) strain: Size,
    pub(super) lens_position: f32,
    pub(super) lens_activity: f32,
    pub(super) resting_tint: Color,
    pub(super) accessory_center: Option<(f32, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TabFlightNode {
    pub(super) origin: (f32, f32),
    pub(super) size: Size,
}

pub(super) fn tab_lens_deformed_size(base: Size, strain: Size) -> Size {
    Size::new(
        base.width * (1.0 + strain.width.clamp(-0.35, 0.35)),
        base.height * (1.0 + strain.height.clamp(-0.35, 0.35)),
    )
}

pub(super) fn tab_flight_dynamics(
    geometry: TabFlightGeometry,
    node: TabFlightNode,
) -> GlassDynamics {
    let activity = geometry.lens_activity.clamp(0.0, 1.0);
    let size = geometry.base_size;
    let deformed = tab_lens_deformed_size(size, geometry.strain);
    let radius = size.width.min(size.height) * 0.5;
    let glue = 20.0;
    let shapes = geometry
        .accessory_center
        .filter(|(x, _)| {
            let edge_gap = (*x - geometry.center.0).abs() - size.width * 0.5 - BAR_HEIGHT * 0.5;
            accessory_surfaces_touch(edge_gap)
        })
        .map(|(x, y)| {
            vec![(
                x - node.origin.0,
                y - node.origin.1,
                BAR_HEIGHT,
                BAR_HEIGHT,
                -1.0,
            )]
        })
        .unwrap_or_default();
    GlassDynamics {
        edge_return_depth_dp: Some(7.0 * activity),
        optical_projection: Some((deformed.width / size.width, deformed.height / size.height)),
        morph: Some(GlassMorph {
            node_size: (node.size.width, node.size.height),
            primary: (
                geometry.center.0 - node.origin.0,
                geometry.center.1 - node.origin.1,
                size.width,
                size.height,
                radius,
            ),
            shapes,
            glue,
            wobble_amplitude: 0.0,
            wobble_phase: geometry.lens_position * 0.045,
            bulge_amplitude: 0.0,
            bulge_direction: 0.0,
            ellipse_blend: 0.0,
            capsule_smoothing_dp: ((size.width - size.height).abs() * 0.5).min(radius * 0.6)
                * activity,
            deformation: None,
            zoom_anchor: (0.0, 0.0),
        }),
        activity: Some(1.0),
        resting_tint: Some(geometry.resting_tint),
        tint_alpha_multiplier: Some(tab_flight_tint_multiplier(geometry.lens_activity)),
        ..Default::default()
    }
}

// Asks for the pipelines the lens draws with once touched, where a bar
// first composes at this density. A tap strains the resting lens, a press
// grows it to full activity, and a drag to the end presses it against the
// accessory; each draws pipelines the resting lens does not. The lens's
// size picks none of them, so a lens over a default tab stands for all.
pub(super) fn request_touch_warm_ups(colors: &LiquidColors, accessory_center: Option<(f32, f32)>) {
    static REQUESTED: Mutex<Vec<(u32, bool)>> = Mutex::new(Vec::new());
    let density = cranpose_ui::current_density();
    let key = (density.to_bits(), accessory_center.is_some());
    {
        let mut requested = REQUESTED.lock().unwrap_or_else(PoisonError::into_inner);
        if requested.contains(&key) {
            return;
        }
        requested.push(key);
    }
    let strained = Size::new(0.1, -0.1);
    let free = [(0.0, strained), (1.0, Size::ZERO), (1.0, strained)].map(|(a, s)| (a, s, None));
    let pressed = accessory_center.into_iter().flat_map(|center| {
        [
            (1.0, Size::ZERO, Some(center)),
            (1.0, strained, Some(center)),
        ]
    });
    let mut warm_ups = Vec::new();
    for (activity, strain, accessory) in free.into_iter().chain(pressed) {
        let (width, height) = tab_lens_base_size(TAB_WIDTH, activity);
        // A lens pressed against the accessory reaches a point into it.
        let center = accessory.map_or((width, height), |(x, y)| {
            (x - (width + BAR_HEIGHT) * 0.5 + 1.0, y)
        });
        let geometry = TabFlightGeometry {
            center,
            base_size: Size::new(width, height),
            strain,
            lens_position: 0.0,
            lens_activity: activity,
            resting_tint: colors.fill,
            accessory_center: accessory,
        };
        let node = TabFlightNode {
            origin: (center.0 - width, center.1 - height),
            size: Size::new(2.0 * width, 2.0 * height),
        };
        let layers = cached_glass_content_layers(
            tab_flight_lens_material(colors.label, activity).resolve(colors),
        );
        // The lens's nodes hold no content, so their render effects never draw.
        for layer in layers(density, tab_flight_dynamics(geometry, node)) {
            if let Some(effect) = &layer.backdrop_effect {
                push_effect_warm_ups(effect, backdrop_target(effect), &mut warm_ups);
            }
        }
    }
    cranpose_ui_graphics::request_shader_warm_ups(warm_ups);
}

// Where the renderer draws a backdrop's last shader over a layer at full
// alpha with no rounded clip (`PendingBackdrop::shader_target`): over the
// page when it batches or reads no substrate, else into a texture.
fn backdrop_target(effect: &RenderEffect) -> ShaderTarget {
    let composites = match effect {
        RenderEffect::Shader { shader } => {
            shader.batched_source() || shader.substrates().is_empty()
        }
        RenderEffect::Chain { first, second } => match second.as_ref() {
            RenderEffect::Shader { shader } => {
                let blurred = match first.as_ref() {
                    RenderEffect::Blur {
                        radius_x, radius_y, ..
                    } => *radius_x > 0.0 || *radius_y > 0.0,
                    _ => false,
                };
                shader.substrates().is_empty() || (blurred && shader.batched_source())
            }
            _ => false,
        },
        _ => false,
    };
    if composites {
        ShaderTarget::Page
    } else {
        ShaderTarget::Layer
    }
}

// Each shader of `effect` at the target it draws to, as the renderer's
// readiness walk asks for them: a chain's first effect into a texture.
fn push_effect_warm_ups(
    effect: &RenderEffect,
    target: ShaderTarget,
    warm_ups: &mut Vec<ShaderWarmUp>,
) {
    match effect {
        RenderEffect::Shader { shader } => warm_ups.push(ShaderWarmUp {
            shader: shader.as_ref().clone(),
            target,
        }),
        RenderEffect::Chain { first, second } => {
            push_effect_warm_ups(first, ShaderTarget::Layer, warm_ups);
            push_effect_warm_ups(second, target, warm_ups);
        }
        RenderEffect::Blur { .. } | RenderEffect::Offset { .. } => {}
    }
}
