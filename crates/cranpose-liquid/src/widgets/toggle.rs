//! The iOS 26 switch: a 63×28 capsule track with a wide capsule thumb
//! (~60% of the track). Pressing lifts the whole thumb into a transparent
//! magnifying glass capsule that grows PAST the track edges (the reference
//! "toggle in action" frames): the track color refracts through it with a
//! rainbow rim while the white thumb dissolves into the glass. Releasing
//! lets the lens shrink back slowly; the white thumb rematerializes at the
//! end of the settle. The thumb is swipable; a tap flips.

use crate::material::{Glass, GlassDynamics, GlassMorph, LiquidModifierExt, LiquidShape};
use crate::motion::LiquidMotion;
use crate::theme::liquid_colors;
use cranpose_animation::{animateColorAsState, animateFloatAsState, spring};
use cranpose_core::{mutableStateOf, remember};
use cranpose_macros::composable;
use cranpose_services::{default_haptics, HapticFeedback};
use cranpose_ui::widgets::{Box, BoxSpec};
use cranpose_ui::{Modifier, PointerEventKind, PointerInputScope, Size};
use cranpose_ui_graphics::{Brush, CornerRadii, GraphicsLayer};
use std::cell::Cell;
use std::rc::Rc;

pub(crate) const TRACK_WIDTH: f32 = 63.0;
pub(crate) const TRACK_HEIGHT: f32 = 28.0;
const THUMB_WIDTH: f32 = 37.0;
const THUMB_HEIGHT: f32 = 25.0;
const THUMB_MARGIN: f32 = 1.5;
/// The pressed lens capsule (measured from the reference: 58×39dp riding the
/// thumb, hanging ~23% past the track's end at either extreme).
const LENS_WIDTH: f32 = 58.0;
const LENS_HEIGHT: f32 = 39.0;
/// How far the lens center leans past the thumb toward its side of travel
/// (the reference composition puts the magnified end-cap mid-lens with the
/// background visible beyond it).
const LENS_LEAN: f32 = 12.0;
/// Glass node span beyond the lens shape (rim glow + wobble live here).
const LENS_PAD: f32 = 10.0;
/// Pointer travel below this is a tap, not a swipe.
const TAP_SLOP: f32 = 4.0;

/// An on/off switch. `checked` is owned by the caller; `on_change` receives
/// the requested new value. The thumb both taps and swipes.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidToggle(modifier: Modifier, checked: bool, on_change: impl Fn(bool) + 'static) {
    let colors = liquid_colors();

    // Some(progress 0..1) while the finger drags the thumb.
    let drag_progress = remember(|| mutableStateOf(Option::<f32>::None)).with(|s| *s);
    let pressed = remember(|| mutableStateOf(false)).with(|s| *s);

    // OFF track: sampled from the reference video's resting switch —
    // srgb(187,186,188), a solid medium gray, darker than iOS systemGray4.
    let off_track = cranpose_ui_graphics::Color::from_rgb_u8(187, 186, 188);
    // Mid-drag the track color FOLLOWS the finger (the reference shows the
    // interpolated sage through the lens while dragging — the color commits
    // with the drag, not after it).
    let drag_mix = |a: cranpose_ui_graphics::Color, b: cranpose_ui_graphics::Color, t: f32| {
        cranpose_ui_graphics::Color::rgba(
            a.r() + (b.r() - a.r()) * t,
            a.g() + (b.g() - a.g()) * t,
            a.b() + (b.b() - a.b()) * t,
            1.0,
        )
    };
    // The whole control lifts while touched: the reference pressed track
    // reads lightened/desaturated (green 52,199,89 → ~86,189,111).
    let base_track = match drag_progress.get() {
        Some(progress) => drag_mix(off_track, colors.success, progress),
        None => {
            if checked {
                colors.success
            } else {
                off_track
            }
        }
    };
    let lifted_track = cranpose_ui_graphics::Color::rgba(
        base_track.r() + (1.0 - base_track.r()) * 0.18,
        base_track.g() + (1.0 - base_track.g()) * 0.10,
        base_track.b() + (1.0 - base_track.b()) * 0.18,
        1.0,
    );
    let track_color = animateColorAsState(
        if pressed.get() {
            lifted_track
        } else {
            base_track
        },
        LiquidMotion::smooth(),
        "toggle-track",
    );

    let min_x = THUMB_MARGIN;
    let max_x = TRACK_WIDTH - THUMB_MARGIN - THUMB_WIDTH;

    // While dragging, the spring TARGET follows the finger — the thumb trails
    // it with a droplet lag; on release it continues into the settle from the
    // same spring state, velocity preserved.
    let target_x = match drag_progress.get() {
        Some(progress) => min_x + (max_x - min_x) * progress,
        None => {
            if checked {
                max_x
            } else {
                min_x
            }
        }
    };
    let thumb_x = animateFloatAsState(target_x, LiquidMotion::snappy(), "toggle-thumb-x");

    // Lens presence: springs to 1 fast on press (the glass materializes in
    // ~120ms), decays slowly after release (the reference lens lingers
    // through the settle flight for ~0.6s before the white thumb returns).
    let settling = (thumb_x.get() - target_x).abs() > 0.75;
    let lens_target = if pressed.get() || (drag_progress.get().is_none() && settling) {
        1.0
    } else {
        0.0
    };
    let lens_progress = animateFloatAsState(
        lens_target,
        if pressed.get() {
            spring(0.9, 1400.0)
        } else {
            spring(1.0, 170.0)
        },
        "toggle-lens",
    );

    let on_change = std::rc::Rc::new(on_change);
    let track = Modifier::empty()
        .size(Size::new(TRACK_WIDTH, TRACK_HEIGHT))
        .pointer_input((), {
            let on_change = std::rc::Rc::clone(&on_change);
            move |scope: PointerInputScope| {
                let on_change = std::rc::Rc::clone(&on_change);
                async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            let mut down_x = 0.0f32;
                            let mut dragging = false;
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                match event.kind {
                                    PointerEventKind::Down => {
                                        dragging = true;
                                        down_x = event.position.x;
                                        pressed.set(true);
                                        default_haptics().perform(HapticFeedback::Selection);
                                        event.consume();
                                    }
                                    PointerEventKind::Move if dragging => {
                                        let progress =
                                            ((event.position.x - THUMB_MARGIN - THUMB_WIDTH * 0.5)
                                                / (TRACK_WIDTH - 2.0 * THUMB_MARGIN - THUMB_WIDTH))
                                                .clamp(0.0, 1.0);
                                        drag_progress.set(Some(progress));
                                        event.consume();
                                    }
                                    PointerEventKind::Up | PointerEventKind::Cancel if dragging => {
                                        dragging = false;
                                        pressed.set(false);
                                        let travelled =
                                            (event.position.x - down_x).abs() > TAP_SLOP;
                                        let next = if travelled {
                                            drag_progress
                                                .get()
                                                .map(|p| p >= 0.5)
                                                .unwrap_or(!checked)
                                        } else {
                                            !checked
                                        };
                                        drag_progress.set(None);
                                        if next != checked {
                                            default_haptics().perform(HapticFeedback::ImpactLight);
                                            on_change(next);
                                        }
                                        event.consume();
                                    }
                                    _ => {}
                                }
                            }
                        })
                        .await;
                }
            }
        })
        .draw_behind(move |scope| {
            // The color commits WITH the thumb: everything the thumb has
            // passed carries the target color, the tail beyond it keeps the
            // source (the reference mid-drag shows a gray tail sticking out
            // past the lens, not a uniformly blended track).
            let size = scope.size();
            scope.draw_round_rect(
                Brush::solid(off_track),
                CornerRadii::uniform(TRACK_HEIGHT * 0.5),
            );
            // Fill reaches the thumb's trailing edge: full green at rest-ON,
            // gray tail beyond the thumb mid-drag (the reference read).
            let split =
                (thumb_x.get() + THUMB_WIDTH + THUMB_MARGIN).clamp(TRACK_HEIGHT, size.width);
            let radius = TRACK_HEIGHT * 0.5;
            let color = track_color.get();
            // Rounded left cap + squared body up to the split (no positioned
            // round-rect primitive; a circle + rect compose the same shape).
            scope.draw_circle(
                Brush::solid(color),
                cranpose_ui_graphics::Point::new(radius, radius),
                radius,
            );
            scope.draw_rect_at(
                cranpose_ui_graphics::Rect {
                    x: radius,
                    y: 0.0,
                    width: (split - radius).max(0.0),
                    height: size.height,
                },
                Brush::solid(color),
            );
        });

    Box(track.then(modifier), BoxSpec::default(), move || {
        let thumb_x_for_layer = thumb_x;
        let lens_for_thumb = lens_progress;
        // Resting thumb: a plain white capsule. It dissolves into the glass
        // as soon as the lens is up and only returns near the settle's end.
        let thumb = Modifier::empty()
            .size(Size::new(THUMB_WIDTH, THUMB_HEIGHT))
            .offset(0.0, (TRACK_HEIGHT - THUMB_HEIGHT) * 0.5)
            .graphics_layer(move || {
                // The thumb only rematerializes once the lens is nearly gone
                // (one object melting into another, never two rims at once).
                let lens = lens_for_thumb.get();
                let alpha = ((0.30 - lens) / 0.22).clamp(0.0, 1.0);
                GraphicsLayer {
                    translation_x: thumb_x_for_layer.get(),
                    alpha,
                    ..Default::default()
                }
            })
            // A soft whisper lifting the thumb off the track (the reference
            // thumb floats; nothing dark).
            .drop_shadow(
                cranpose_ui_graphics::LayerShape::Rounded(
                    cranpose_ui_graphics::RoundedCornerShape::uniform(THUMB_HEIGHT * 0.5),
                ),
                |scope| {
                    scope.radius = 2.0;
                    scope.offset.y = 0.5;
                    scope.color = cranpose_ui_graphics::Color::BLACK.with_alpha(0.10);
                },
            )
            .draw_behind(move |scope| {
                scope.draw_round_rect(
                    Brush::solid(cranpose_ui_graphics::Color::WHITE),
                    CornerRadii::uniform(THUMB_HEIGHT * 0.5),
                );
            });
        Box(thumb, BoxSpec::default(), || {});

        // The interaction lens: one glass node riding the thumb; its SDF
        // capsule inflates from thumb-size to the full lens with a viscous
        // bulge along the drag direction. The morph (not a layer scale)
        // grows it so refraction, rim and wobble stay physically coherent.
        if lens_progress.get() > 0.01 {
            let node_w = LENS_WIDTH + LENS_PAD * 2.0;
            let node_h = LENS_HEIGHT + LENS_PAD * 2.0;
            let thumb_x_for_lens = thumb_x;
            let lens_for_layer = lens_progress;
            let last_x = remember(|| Rc::new(Cell::new(f32::NAN))).with(Rc::clone);
            let lens = Modifier::empty()
                // required_size: the lens node MUST exceed the 63×28 track
                // box — plain size() would be coerced by the parent's
                // constraints, clamping the SDF and slicing the lens.
                .required_size(Size::new(node_w, node_h))
                .offset(0.0, (TRACK_HEIGHT - node_h) * 0.5)
                .graphics_layer(move || {
                    // The lens leans past the thumb toward its side of
                    // travel (the reference lens overhangs the track end).
                    let x = thumb_x_for_lens.get();
                    let side = ((x - THUMB_MARGIN)
                        / (TRACK_WIDTH - 2.0 * THUMB_MARGIN - THUMB_WIDTH))
                        .clamp(0.0, 1.0)
                        * 2.0
                        - 1.0;
                    GraphicsLayer {
                        translation_x: x + (THUMB_WIDTH - node_w) * 0.5 + side * LENS_LEAN,
                        alpha: (lens_for_layer.get() * 2.5).clamp(0.0, 1.0),
                        ..Default::default()
                    }
                })
                .glass_effect_with(
                    Glass::lens().shape(LiquidShape::Capsule).no_clip(),
                    move || {
                        let grow = lens_for_layer.get().clamp(0.0, 1.2);
                        let w = THUMB_WIDTH + (LENS_WIDTH - THUMB_WIDTH) * grow;
                        let h = THUMB_HEIGHT + (LENS_HEIGHT - THUMB_HEIGHT) * grow;
                        // Finite-difference velocity of the ride position
                        // feeds the leading-edge bulge: dragging the lens
                        // swells its front like a pulled droplet.
                        let x = thumb_x_for_lens.get();
                        let prev = last_x.replace(x);
                        let vx = if prev.is_nan() { 0.0 } else { x - prev };
                        let bulge = (vx.abs() * 1.6).min(5.0);
                        let dir = if vx >= 0.0 { 0.0 } else { std::f32::consts::PI };
                        GlassDynamics {
                            morph: Some(GlassMorph {
                                node_size: (node_w, node_h),
                                primary: (node_w * 0.5, node_h * 0.5, w, h, -1.0),
                                shapes: Vec::new(),
                                glue: 0.0,
                                wobble_amplitude: 0.0,
                                wobble_phase: 0.0,
                                bulge_amplitude: bulge,
                                bulge_direction: dir,
                            }),
                            // The reference toggle lens fills with the
                            // magnified track — the base 1.35 isn't enough
                            // for the 39dp lens over the 28dp track.
                            magnify_boost: 0.25,
                            ..Default::default()
                        }
                    },
                );
            Box(lens, BoxSpec::default(), || {});
        }
    });
}
