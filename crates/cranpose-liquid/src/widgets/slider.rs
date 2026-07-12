//! A continuous slider: capsule track with a filled portion and a round
//! draggable thumb. Touching the thumb lifts it into a magnifying glass
//! lens (the reference "seek" thumb liquifies under the finger and rides
//! the drag); releasing lets the lens decay back into the white thumb.

use crate::material::{Glass, GlassDynamics, GlassMorph, LiquidModifierExt, LiquidShape};
use cranpose_animation::{animateFloatAsState, spring};
use cranpose_core::{mutableStateOf, remember};
use cranpose_foundation::PointerEventKind;
use cranpose_macros::composable;
use cranpose_ui::widgets::{Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope};
use cranpose_ui::{Modifier, Size};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, GraphicsLayer};
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

/// A 0..=1 slider. The caller owns `value`; `on_change` streams new values
/// while dragging or on tap.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidSlider(modifier: Modifier, value: f32, on_change: impl Fn(f32) + 'static) {
    let colors = liquid_colors();
    let value = value.clamp(0.0, 1.0);
    let on_change = Rc::new(on_change);
    let pressed = remember(|| mutableStateOf(false)).with(|s| *s);

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
            BoxWithConstraints(Modifier::empty(), move |scope| {
                let width = scope.constraints().max_width.max(THUMB_SIZE);
                let usable = (width - THUMB_SIZE).max(1.0);
                let thumb_x = usable * value;

                // Interactive surface spanning the whole control: tap or drag
                // anywhere on the track.
                let on_drag = Rc::clone(&on_change);
                let dragging = Rc::new(Cell::new(false));
                let surface = Modifier::empty()
                    .size(Size::new(width, SLIDER_HEIGHT))
                    .pointer_input((), {
                        move |pointer_scope| {
                            let on_drag = Rc::clone(&on_drag);
                            let dragging = Rc::clone(&dragging);
                            async move {
                                pointer_scope
                                    .await_pointer_event_scope(|await_scope| async move {
                                        loop {
                                            let event = await_scope.await_pointer_event().await;
                                            if event.id != 0 {
                                                continue;
                                            }
                                            let fraction = ((event.position.x - THUMB_SIZE * 0.5)
                                                / usable)
                                                .clamp(0.0, 1.0);
                                            match event.kind {
                                                PointerEventKind::Down => {
                                                    dragging.set(true);
                                                    pressed.set(true);
                                                    event.consume();
                                                    on_drag(fraction);
                                                }
                                                PointerEventKind::Move if dragging.get() => {
                                                    event.consume();
                                                    on_drag(fraction);
                                                }
                                                PointerEventKind::Up | PointerEventKind::Cancel => {
                                                    dragging.set(false);
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
                        let dynamics = crate::dynamics::remember_liquid_dynamics();
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
                                Glass::lens().shape(LiquidShape::Circle).no_clip(),
                                move || {
                                    let grow = lens_for_layer.get().clamp(0.0, 1.2);
                                    let d = THUMB_SIZE + (LENS_SIZE - THUMB_SIZE) * grow;
                                    // Droplet law over the thumb ride
                                    // (crate::dynamics): drag speed stretches
                                    // the circle into a travel-axis oval,
                                    // braking swells its leading edge.
                                    let pose = dynamics.update((thumb_x, 0.0));
                                    let (w, h) = pose.size(d, d);
                                    GlassDynamics {
                                        morph: Some(GlassMorph {
                                            node_size: (node, node),
                                            primary: (node * 0.5, node * 0.5, w, h, -1.0),
                                            shapes: Vec::new(),
                                            glue: 0.0,
                                            wobble_amplitude: 0.0,
                                            wobble_phase: 0.0,
                                            bulge_amplitude: pose.bulge_amplitude.min(4.0),
                                            bulge_direction: pose.bulge_direction,
                                        }),
                                        magnify_boost: 0.2,
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
