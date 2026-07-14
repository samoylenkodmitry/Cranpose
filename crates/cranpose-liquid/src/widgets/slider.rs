//! A continuous slider: capsule track with a filled portion and a round
//! draggable thumb. Touching the thumb lifts it into a magnifying glass
//! lens (the reference "seek" thumb liquifies under the finger and rides
//! the drag); releasing lets the lens decay back into the white thumb.

use crate::material::{Glass, GlassDynamics, GlassMorph, LiquidModifierExt, LiquidShape};
use cranpose_animation::{animateFloatAsState, spring};
use cranpose_core::{mutableStateOf, remember};
use cranpose_foundation::{PointerEventKind, PointerId};
use cranpose_macros::composable;
use cranpose_ui::widgets::{Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope};
use cranpose_ui::{Modifier, Size};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, GlassSurfaceProfile, GraphicsLayer};
use std::cell::Cell;
use std::rc::Rc;

use crate::theme::liquid_colors;

const TRACK_HEIGHT: f32 = 6.0;
const THUMB_SIZE: f32 = 24.0;
const SLIDER_HEIGHT: f32 = 32.0;
/// The pressed lens circle riding the thumb (the reference seek thumb grows
/// ~1.4× and magnifies the track through itself).
const LENS_SIZE: f32 = 34.0;
/// Glass node span beyond the lens shape (rim glow + bulge live here).
const LENS_PAD: f32 = 10.0;
const SLIDER_STRAIN_GAIN: f32 = 2.0;

fn slider_deformation(pose: crate::dynamics::LiquidPose) -> crate::material::GlassDeformation {
    let stretch = pose
        .stretch
        .powf(SLIDER_STRAIN_GAIN)
        .clamp(crate::dynamics::STRETCH_MIN, crate::dynamics::STRETCH_MAX);
    crate::material::GlassDeformation::incompressible(pose.axis, stretch)
}

/// A 0..=1 slider. The caller owns `value`; `on_change` streams new values
/// while dragging or on tap.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidSlider(modifier: Modifier, value: f32, on_change: impl Fn(f32) + 'static) {
    let colors = liquid_colors();
    let value = value.clamp(0.0, 1.0);
    let on_change = Rc::new(on_change);
    let pressed = remember(|| mutableStateOf(false)).with(|s| *s);
    let active_pointer = remember(|| Rc::new(Cell::new(Option::<PointerId>::None))).with(Rc::clone);

    // Lens presence: fast on press, slow decay after release (the lens
    // lingers briefly, like the toggle's).
    let lens_progress = animateFloatAsState(
        if pressed.get() { 1.0 } else { 0.0 },
        if pressed.get() {
            spring(0.9, 1400.0)
        } else {
            spring(1.0, 170.0)
        },
        "slider-lens",
    );

    Box(
        Modifier::empty().height(SLIDER_HEIGHT).then(modifier),
        BoxSpec::default(),
        move || {
            let on_change = Rc::clone(&on_change);
            let active_pointer = Rc::clone(&active_pointer);
            BoxWithConstraints(Modifier::empty(), move |scope| {
                let width = scope.constraints().max_width.max(THUMB_SIZE);
                let usable = (width - THUMB_SIZE).max(1.0);
                let controlled_x = usable * value;
                let lens_axis = crate::motion::remember_liquid_drag_axis(controlled_x);
                lens_axis.settle_to(controlled_x, crate::motion::LiquidMotion::snappy());
                let thumb_x = lens_axis.value();
                let liquid_pose = lens_axis.liquid_pose();

                // Interactive surface spanning the whole control: tap or drag
                // anywhere on the track.
                let on_drag = Rc::clone(&on_change);
                let active_pointer = Rc::clone(&active_pointer);
                let gesture_axis = Rc::clone(&lens_axis);
                let surface = Modifier::empty()
                    .size(Size::new(width, SLIDER_HEIGHT))
                    .pointer_input((width.to_bits(), value.to_bits()), {
                        move |pointer_scope| {
                            let on_drag = Rc::clone(&on_drag);
                            let active_pointer = Rc::clone(&active_pointer);
                            let lens_axis = Rc::clone(&gesture_axis);
                            async move {
                                pointer_scope
                                    .await_pointer_event_scope(|await_scope| async move {
                                        loop {
                                            let event = await_scope.await_pointer_event().await;
                                            let fraction = ((event.position.x - THUMB_SIZE * 0.5)
                                                / usable)
                                                .clamp(0.0, 1.0);
                                            match event.kind {
                                                PointerEventKind::Down
                                                    if active_pointer.get().is_none() =>
                                                {
                                                    active_pointer.set(Some(event.id));
                                                    lens_axis
                                                        .begin(usable * fraction, event.time_ms);
                                                    pressed.set(true);
                                                    event.consume();
                                                    on_drag(fraction);
                                                }
                                                PointerEventKind::Move
                                                    if active_pointer.get() == Some(event.id) =>
                                                {
                                                    lens_axis
                                                        .move_to(usable * fraction, event.time_ms);
                                                    event.consume();
                                                    on_drag(fraction);
                                                }
                                                PointerEventKind::Up
                                                    if active_pointer.get() == Some(event.id) =>
                                                {
                                                    lens_axis.release_to(
                                                        usable * fraction,
                                                        event.time_ms,
                                                        crate::motion::LiquidMotion::snappy(),
                                                    );
                                                    on_drag(fraction);
                                                    event.consume();
                                                    active_pointer.set(None);
                                                    pressed.set(false);
                                                }
                                                PointerEventKind::Cancel
                                                    if active_pointer.get() == Some(event.id) =>
                                                {
                                                    lens_axis.release_to(
                                                        controlled_x,
                                                        event.time_ms,
                                                        crate::motion::LiquidMotion::snappy(),
                                                    );
                                                    event.consume();
                                                    active_pointer.set(None);
                                                    pressed.set(false);
                                                }
                                                _ => {}
                                            }
                                        }
                                    })
                                    .await;
                            }
                        }
                    });

                Box(surface, BoxSpec::default(), move || {
                    // Track.
                    let track_fill = colors.fill;
                    let track = Modifier::empty()
                        .size(Size::new(width, TRACK_HEIGHT))
                        .offset(0.0, (SLIDER_HEIGHT - TRACK_HEIGHT) * 0.5)
                        .draw_behind(move |scope| {
                            scope.draw_round_rect(
                                Brush::solid(track_fill),
                                CornerRadii::uniform(TRACK_HEIGHT * 0.5),
                            );
                        });
                    Box(track, BoxSpec::default(), || {});

                    // Filled portion up to the thumb.
                    let accent = colors.accent;
                    let filled_width = (thumb_x + THUMB_SIZE * 0.5).max(TRACK_HEIGHT);
                    let filled = Modifier::empty()
                        .size(Size::new(filled_width, TRACK_HEIGHT))
                        .offset(0.0, (SLIDER_HEIGHT - TRACK_HEIGHT) * 0.5)
                        .draw_behind(move |scope| {
                            scope.draw_round_rect(
                                Brush::solid(accent),
                                CornerRadii::uniform(TRACK_HEIGHT * 0.5),
                            );
                        });
                    Box(filled, BoxSpec::default(), || {});

                    // Thumb: a plain white circle that dissolves into the
                    // glass as soon as the lens is up (the lens takes over
                    // the thumb's role while touched).
                    let lens_for_thumb = lens_progress;
                    let thumb = Modifier::empty()
                        .size(Size::new(THUMB_SIZE, THUMB_SIZE))
                        .offset(0.0, (SLIDER_HEIGHT - THUMB_SIZE) * 0.5)
                        .graphics_layer(move || {
                            let lens = lens_for_thumb.get();
                            GraphicsLayer {
                                translation_x: thumb_x,
                                alpha: ((0.30 - lens) / 0.22).clamp(0.0, 1.0),
                                ..Default::default()
                            }
                        })
                        .drop_shadow(
                            cranpose_ui_graphics::LayerShape::Rounded(
                                cranpose_ui_graphics::RoundedCornerShape::uniform(THUMB_SIZE * 0.5),
                            ),
                            |scope| {
                                scope.radius = 6.0;
                                scope.offset.y = 2.0;
                                scope.color = Color::BLACK.with_alpha(0.16);
                            },
                        )
                        .draw_behind(move |scope| {
                            scope.draw_round_rect(
                                Brush::solid(Color::WHITE),
                                CornerRadii::uniform(THUMB_SIZE * 0.5),
                            );
                        });
                    Box(thumb, BoxSpec::default(), || {});

                    // The interaction lens riding the thumb: the SDF circle
                    // inflates from thumb-size to the lens with a viscous
                    // bulge along the travel direction.
                    if lens_progress.get() > 0.01 {
                        let node = LENS_SIZE + LENS_PAD * 2.0;
                        let lens_for_layer = lens_progress;
                        let lens = Modifier::empty()
                            // required_size: the lens node exceeds the 32dp
                            // control; the fixed-height host keeps layout put.
                            .required_size(Size::new(node, node))
                            .offset(0.0, (SLIDER_HEIGHT - node) * 0.5)
                            .graphics_layer(move || GraphicsLayer {
                                translation_x: thumb_x + (THUMB_SIZE - node) * 0.5,
                                alpha: (lens_for_layer.get() * 2.5).clamp(0.0, 1.0),
                                ..Default::default()
                            })
                            .glass_effect_with(
                                Glass::lens()
                                    .shape(LiquidShape::Circle)
                                    .tint(Color::WHITE.with_alpha(0.05))
                                    .surface_profile(
                                        GlassSurfaceProfile::lens()
                                            .with_depth(7.2)
                                            .expect("slider surface depth is valid"),
                                    )
                                    .highlight(0.52)
                                    .chromatic_aberration(2.5)
                                    .displacement(26.0)
                                    .no_clip(),
                                move || {
                                    let grow = lens_for_layer.get().clamp(0.0, 1.2);
                                    let d = THUMB_SIZE + (LENS_SIZE - THUMB_SIZE) * grow;
                                    // Droplet law over the thumb ride
                                    // (crate::dynamics): drag speed stretches
                                    // the circle into a travel-axis oval,
                                    // braking swells its leading edge.
                                    let pose = liquid_pose;
                                    GlassDynamics {
                                        morph: Some(GlassMorph {
                                            node_size: (node, node),
                                            primary: (node * 0.5, node * 0.5, d, d, -1.0),
                                            shapes: Vec::new(),
                                            glue: 0.0,
                                            wobble_amplitude: 0.0,
                                            wobble_phase: 0.0,
                                            bulge_amplitude: pose
                                                .bulge_amplitude
                                                .max(2.5 * pose.energy())
                                                .min(4.0),
                                            bulge_direction: pose.bulge_direction,
                                            ellipse_blend: 0.0,
                                            deformation: Some(slider_deformation(pose)),
                                        }),
                                        surface_depth_boost: 0.2,
                                        ..Default::default()
                                    }
                                },
                            );
                        Box(lens, BoxSpec::default(), || {});
                    }
                });
            });
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_strain_is_amplified_but_remains_incompressible() {
        let pose = crate::dynamics::LiquidPose {
            stretch: 1.05,
            ortho: 1.0 / 1.05,
            ..Default::default()
        };
        let deformation = slider_deformation(pose);
        assert!(deformation.along() > 1.09);
        assert!((deformation.along() * deformation.across() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn slider_strain_preserves_acceleration_axis_compression() {
        let pose = crate::dynamics::LiquidPose {
            stretch: 0.95,
            ortho: 1.0 / 0.95,
            ..Default::default()
        };
        let deformation = slider_deformation(pose);
        assert!(deformation.along() < 0.91);
        assert!(deformation.across() > 1.10);
    }
}
