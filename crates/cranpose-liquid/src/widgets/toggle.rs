//! The iOS 26 switch: a 63×28 capsule track with a wide capsule thumb
//! (~60% of the track). Pressing lifts the whole thumb into a transparent
//! refractive glass capsule that grows past the track edges (the reference
//! "toggle in action" frames): the track color refracts through it with a
//! rainbow rim while the white thumb dissolves into the glass. Releasing
//! lets the lens shrink back slowly; the white thumb rematerializes at the
//! end of the settle. The thumb is swipable; a tap flips.

use crate::material::{
    Glass, GlassDynamics, GlassMorph, GlassShadow, LiquidModifierExt, LiquidShape,
};
use crate::motion::LiquidMotion;
use crate::theme::liquid_colors;
use cranpose_animation::{
    animateColorAsState, animateFloatAsState, spring, AnimationSpec, AnimationType, Easing,
};
use cranpose_core::{mutableStateOf, remember};
use cranpose_macros::composable;
use cranpose_services::{default_haptics, HapticFeedback};
use cranpose_ui::widgets::{Box, BoxSpec};
use cranpose_ui::{Modifier, PointerEventKind, PointerInputScope, Size};
use cranpose_ui_graphics::{Brush, CornerRadii, GraphicsLayer};

pub(crate) const TRACK_WIDTH: f32 = 63.0;
pub(crate) const TRACK_HEIGHT: f32 = 28.0;
const THUMB_WIDTH: f32 = 37.0;
const THUMB_HEIGHT: f32 = 25.0;
const THUMB_MARGIN: f32 = 1.5;
/// The pressed lens capsule rides the full track width and grows past the
/// track edges, matching the reference's refractive chamber.
const LENS_WIDTH: f32 = 58.0;
const LENS_HEIGHT: f32 = 109.0 / 3.0;
const LENS_VERTICAL_OFFSET: f32 = -2.0 / 3.0;
/// The raised lens leans toward the travel side, measured from the thumb
/// center on the reference press and settle frames (~6-8dp in every phase:
/// press leans toward the destination, flight leads the thumb, settle
/// overhangs the arrival end).
const LENS_TRAVEL_LEAN: f32 = 7.0;
/// Glass node span beyond the lens shape (rim glow + wobble live here).
/// The full dome plus the travel lean needs real headroom — 10dp cut the
/// leaning edge flat (live report).
const LENS_PAD: f32 = 18.0;
/// Pointer travel below this is a tap, not a swipe.
const TAP_SLOP: f32 = 4.0;
// The flight itself now carries the raised lens; the reference keeps the
// lens clearly readable ~750 ms after release (toggle-press sheet still
// shows it at 883 ms) before the thumb rematerializes.
const LENS_RELEASE_LINGER_MS: u64 = 400;
const LENS_RELEASE_FADE_MS: u64 = 400;

fn toggle_track_motion() -> AnimationType {
    // The reference track crosses into sage ~17 ms after release and reaches
    // full green ~150 ms later (toggle-press sheet, 60 fps): a fast-start
    // sweep with no delay.
    AnimationType::Tween(AnimationSpec::tween(150, Easing::EaseOut))
}

fn toggle_lens_material() -> Glass {
    Glass::lens()
        .shape(LiquidShape::Capsule)
        .tint(cranpose_ui_graphics::Color::WHITE.with_alpha(0.02))
        // The original wcKSRD blurs inside the lens (its 9x9 loop at half-px
        // steps): content edges under the dome read soft, never cut.
        .blur_radius(2.0)
        // The reference lens transmits the track at full saturation with a
        // clear face and vivid chromatic rim (toggle-press cheatsheet); the
        // milky wash came from over-absorbing the transmitted ray.
        .saturation(1.0)
        // The FULL wcKSRD dome (example/shaders.txt): interior ramps across
        // the whole inradius, so the face magnifies and the rim replays the
        // interior in ONE continuous mapping — no fold band, no zoom term,
        // no scratches on the surface.
        .refraction_depth(1.0)
        // The FULL dome curve: sin(interior * pi/2) — the face magnifies
        // the thumb across the lens so the green fills it to a THIN
        // mirrored rim (toggle-press T266). The shallow 0.25 curve left
        // the face barely magnifying: a wide un-magnified band of the
        // white well drowned the dispersion fringes into a solid bloom.
        .refraction_curve(1.0)
        .transmission_refraction(1.0)
        .meniscus_absorption(0.70)
        .dispersion(0.85)
        // The reference press face is the magnified track, not a lit dome —
        // its luma matches the sampled backdrop (toggle-press detail).
        .highlight(0.04)
        .lift(0.0)
        .shadow_style(GlassShadow::new(
            cranpose_ui_graphics::Color::BLACK.with_alpha(0.14),
            14.0,
            4.0,
            -1.5,
        ))
        .no_clip()
}

fn toggle_motion_bulge(pose: crate::dynamics::LiquidPose) -> f32 {
    pose.bulge_amplitude.max(3.5 * pose.energy()).min(5.0)
}

fn toggle_lens_release() -> AnimationType {
    AnimationType::Tween(
        AnimationSpec::tween(LENS_RELEASE_FADE_MS, Easing::EaseIn)
            .with_delay(LENS_RELEASE_LINGER_MS),
    )
}

fn track_tint_progress(progress: f32) -> f32 {
    // Direct contact never paints the committed endpoint color. The whole
    // fixed capsule moves through gray/sage together; release owns the final
    // transition into system green.
    let t = ((progress.clamp(0.0, 1.0) - 0.20) / 1.25).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn interpolate_track_color(
    source: cranpose_ui_graphics::Color,
    target: cranpose_ui_graphics::Color,
    progress: f32,
) -> cranpose_ui_graphics::Color {
    let progress = progress.clamp(0.0, 1.0);
    cranpose_ui_graphics::Color::rgba(
        source.r() + (target.r() - source.r()) * progress,
        source.g() + (target.g() - source.g()) * progress,
        source.b() + (target.b() - source.b()) * progress,
        source.a() + (target.a() - source.a()) * progress,
    )
}

fn lens_translation_x(thumb_x: f32, node_width: f32) -> f32 {
    thumb_x + (THUMB_WIDTH - node_width) * 0.5
}

/// The lean's travel side for a fresh press: the only end this switch can
/// head to. Movement and release retarget it through the gesture handler —
/// the fluid motion axis is unusable here because a slow drag stays under
/// its direction threshold and holds whatever the previous flight left.
fn lens_press_travel(checked: bool) -> f32 {
    if checked {
        -1.0
    } else {
        1.0
    }
}

fn lens_ride_x(drag_progress: Option<f32>, thumb_x: f32) -> f32 {
    drag_progress
        .map(|progress| {
            let min_x = THUMB_MARGIN;
            let max_x = TRACK_WIDTH - THUMB_MARGIN - THUMB_WIDTH;
            min_x + (max_x - min_x) * progress.clamp(0.0, 1.0)
        })
        .unwrap_or(thumb_x)
}

/// An on/off switch. `checked` is owned by the caller; `on_change` receives
/// the requested new value. The thumb both taps and swipes.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidToggle(modifier: Modifier, checked: bool, on_change: impl Fn(bool) + 'static) {
    let colors = liquid_colors();

    // Some(progress 0..1) while the finger drags the thumb.
    let drag_progress = remember(|| mutableStateOf(Option::<f32>::None)).with(|s| *s);
    let pressed = remember(|| mutableStateOf(false)).with(|s| *s);
    // ±1: which end the gesture is heading to. Press aims at the flip side,
    // movement follows the finger, release holds the committed side through
    // the linger (the settled lens keeps overhanging its arrival end).
    let travel_dir = remember(|| mutableStateOf(1.0f32)).with(|s| *s);
    let off_track = colors.toggle_off;
    // Mid-drag the complete track interpolates with the finger. Its capsule
    // geometry is fixed; the glass is a separate optical layer above it.
    let base_track = match drag_progress.get() {
        Some(progress) => {
            interpolate_track_color(off_track, colors.toggle_on, track_tint_progress(progress))
        }
        None => {
            if checked {
                colors.toggle_on
            } else {
                off_track
            }
        }
    };
    // The uncovered tail retains the system accent. The lens itself creates
    // the lighter, lower-saturation interior through refraction; applying
    // that tone transform to the whole track washes out the reference color.
    let animated_track = animateColorAsState(base_track, toggle_track_motion(), "toggle-track");
    let track_color = if drag_progress.get().is_some() {
        base_track
    } else {
        animated_track.get()
    };

    let min_x = THUMB_MARGIN;
    let max_x = TRACK_WIDTH - THUMB_MARGIN - THUMB_WIDTH;

    // While dragging, the spring target follows the finger: the thumb trails
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
    let lens_axis = crate::motion::remember_liquid_drag_axis(target_x);
    lens_axis.settle_to(target_x, LiquidMotion::snappy());
    let lens_x = lens_axis.value();

    // Lens presence: springs to 1 fast on press (the glass materializes in
    // ~120ms), decays slowly after release (the reference lens lingers
    // through the settle flight for ~0.6s before the white thumb returns).
    // A quick TAP holds the lens raised through the whole flip flight —
    // the reference shows the full glass riding the thumb to the far end,
    // not a faint trace of a barely-risen press.
    let thumb_in_flight = (thumb_x.get() - target_x).abs() > 1.5;
    let lens_target = if pressed.get() || thumb_in_flight {
        1.0
    } else {
        0.0
    };
    let lens_progress = animateFloatAsState(
        lens_target,
        if pressed.get() || thumb_in_flight {
            spring(0.9, 1400.0)
        } else {
            toggle_lens_release()
        },
        "toggle-lens",
    );

    let on_change = std::rc::Rc::new(on_change);
    let track = Modifier::empty()
        .size(Size::new(TRACK_WIDTH, TRACK_HEIGHT))
        // The controlled value is part of the gesture identity. Once a
        // release commits a new value, the next gesture must capture that
        // value; keeping one coroutine under a constant key leaves `checked`
        // frozen at the first composition and prevents reversing the switch.
        .pointer_input(checked, {
            let on_change = std::rc::Rc::clone(&on_change);
            let lens_axis = std::rc::Rc::clone(&lens_axis);
            move |scope: PointerInputScope| {
                let on_change = std::rc::Rc::clone(&on_change);
                let lens_axis = std::rc::Rc::clone(&lens_axis);
                async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            let mut down_x = 0.0f32;
                            let mut grab_offset = 0.0f32;
                            let mut dragging = false;
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                match event.kind {
                                    PointerEventKind::Down => {
                                        dragging = true;
                                        down_x = event.position.x;
                                        // The finger grabs the thumb WHERE it
                                        // touched it: without this offset the
                                        // first move snaps the thumb center
                                        // under the finger (live report).
                                        grab_offset =
                                            event.position.x - (thumb_x.get() + THUMB_WIDTH * 0.5);
                                        lens_axis.begin(thumb_x.get(), event.time_ms);
                                        pressed.set(true);
                                        travel_dir.set(lens_press_travel(checked));
                                        default_haptics().perform(HapticFeedback::Selection);
                                        event.consume();
                                    }
                                    PointerEventKind::Move if dragging => {
                                        let progress = ((event.position.x
                                            - grab_offset
                                            - THUMB_MARGIN
                                            - THUMB_WIDTH * 0.5)
                                            / (TRACK_WIDTH - 2.0 * THUMB_MARGIN - THUMB_WIDTH))
                                            .clamp(0.0, 1.0);
                                        if let Some(previous) = drag_progress.get() {
                                            if (progress - previous).abs() > 0.005 {
                                                travel_dir.set((progress - previous).signum());
                                            }
                                        }
                                        drag_progress.set(Some(progress));
                                        lens_axis.move_to(
                                            lens_ride_x(Some(progress), thumb_x.get()),
                                            event.time_ms,
                                        );
                                        event.consume();
                                    }
                                    PointerEventKind::Up if dragging => {
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
                                        travel_dir.set(if next { 1.0 } else { -1.0 });
                                        lens_axis.release_to(
                                            if next { max_x } else { min_x },
                                            event.time_ms,
                                            LiquidMotion::glide(),
                                        );
                                        drag_progress.set(None);
                                        if next != checked {
                                            default_haptics().perform(HapticFeedback::ImpactLight);
                                            on_change(next);
                                        }
                                        event.consume();
                                    }
                                    PointerEventKind::Cancel if dragging => {
                                        dragging = false;
                                        pressed.set(false);
                                        travel_dir.set(if checked { 1.0 } else { -1.0 });
                                        lens_axis.release_to(
                                            if checked { max_x } else { min_x },
                                            event.time_ms,
                                            LiquidMotion::snappy(),
                                        );
                                        drag_progress.set(None);
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
            scope.draw_round_rect(
                Brush::solid(track_color),
                CornerRadii::uniform(TRACK_HEIGHT * 0.5),
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
        let deformation_headroom =
            crate::dynamics::STRETCH_MAX.max(1.0 / crate::dynamics::STRETCH_MIN);
        let node_w =
            LENS_WIDTH * deformation_headroom + crate::dynamics::BULGE_MAX + LENS_PAD * 2.0;
        let node_h =
            LENS_HEIGHT * deformation_headroom + crate::dynamics::BULGE_MAX + LENS_PAD * 2.0;
        let lens_for_layer = lens_progress;
        let physics_axis = std::rc::Rc::clone(&lens_axis);
        let lens = Modifier::empty()
            // required_size: the lens node MUST exceed the 63×28 track
            // box — plain size() would be coerced by the parent's
            // constraints, clamping the SDF and slicing the lens.
            .required_size(Size::new(node_w, node_h))
            .offset(0.0, (TRACK_HEIGHT - node_h) * 0.5 + LENS_VERTICAL_OFFSET)
            .graphics_layer(move || GraphicsLayer {
                translation_x: lens_translation_x(lens_x, node_w),
                ..Default::default()
            })
            .glass_effect_with(toggle_lens_material(), move || {
                let grow = lens_for_layer.get().clamp(0.0, 1.2);
                let base_w = THUMB_WIDTH + (LENS_WIDTH - THUMB_WIDTH) * grow;
                let base_h = THUMB_HEIGHT + (LENS_HEIGHT - THUMB_HEIGHT) * grow;
                // Droplet law over the ride position: drag speed
                // stretches the lens along the track, braking swells
                // its leading edge (crate::dynamics).
                let pose = physics_axis.liquid_pose();
                // The lens leans toward its travel side in every raised
                // phase (the reference press leans toward the destination,
                // the settle overhangs the arrival end). The lean lives in
                // the SDF center so the optics tilt while the node holds.
                let lean = travel_dir.get() * LENS_TRAVEL_LEAN * grow.clamp(0.0, 1.0);
                GlassDynamics {
                    activity: Some(grow.clamp(0.0, 1.0)),
                    morph: Some(GlassMorph {
                        node_size: (node_w, node_h),
                        primary: (node_w * 0.5 + lean, node_h * 0.5, base_w, base_h, -1.0),
                        shapes: Vec::new(),
                        glue: 0.0,
                        wobble_amplitude: 0.0,
                        wobble_phase: 0.0,
                        bulge_amplitude: toggle_motion_bulge(pose),
                        bulge_direction: pose.bulge_direction,
                        ellipse_blend: 0.0,
                        deformation: Some(pose.deformation()),
                    }),
                    ..Default::default()
                }
            });
        Box(lens, BoxSpec::default(), || {});
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_geometry_matches_the_reference_proportions() {
        assert_eq!((TRACK_WIDTH, TRACK_HEIGHT), (63.0, 28.0));
        assert_eq!((THUMB_WIDTH, THUMB_HEIGHT), (37.0, 25.0));
        assert_eq!((LENS_WIDTH, LENS_HEIGHT), (58.0, 109.0 / 3.0));
        assert_eq!(
            toggle_lens_material().shape,
            LiquidShape::Capsule,
            "the pressed switch thumb remains a capsule while its optical body inflates"
        );
        assert!((LENS_VERTICAL_OFFSET + 2.0 / 3.0).abs() < 1.0e-6);
        assert!(
            ((LENS_HEIGHT - TRACK_HEIGHT) * 0.5 - LENS_VERTICAL_OFFSET - 29.0 / 6.0).abs() < 1.0e-6
        );
        assert!(
            ((LENS_HEIGHT - TRACK_HEIGHT) * 0.5 + LENS_VERTICAL_OFFSET - 7.0 / 2.0).abs() < 1.0e-6
        );

        let mid = interpolate_track_color(
            cranpose_ui_graphics::Color::from_rgb_u8(187, 186, 188),
            cranpose_ui_graphics::Color::from_rgb_u8(43, 189, 76),
            0.5,
        );
        assert!((mid.r() - 115.0 / 255.0).abs() < 1.0e-6);
        assert!((mid.g() - 187.5 / 255.0).abs() < 1.0e-6);
        assert!((mid.b() - 132.0 / 255.0).abs() < 1.0e-6);
    }

    #[test]
    fn toggle_lens_leans_toward_the_travel_side() {
        // Reference press/settle frames: the lens center sits ~6-8dp toward
        // the side the gesture heads to, in every phase.
        assert_eq!(LENS_TRAVEL_LEAN, 7.0);
        // A fresh press has no motion yet: the only travel side is the
        // opposite end of the track.
        assert_eq!(lens_press_travel(false), 1.0);
        assert_eq!(lens_press_travel(true), -1.0);

        // Mid-drag the leaning lens frees the departed track region: its
        // trailing edge must clear the whole-track interpolation samples
        // (the traveling-fill contract probes 5dp in from the track end).
        let min = THUMB_MARGIN;
        let max = TRACK_WIDTH - THUMB_MARGIN - THUMB_WIDTH;
        let mid_thumb_center = (min + max) * 0.5 + THUMB_WIDTH * 0.5;
        let lens_trailing_edge = mid_thumb_center + LENS_TRAVEL_LEAN - LENS_WIDTH * 0.5;
        assert!(lens_trailing_edge > 5.0);

        // The node itself never leans — the lean lives in the SDF center so
        // the oversized node's padding absorbs it on both sides.
        let node_width = LENS_WIDTH
            * crate::dynamics::STRETCH_MAX.max(1.0 / crate::dynamics::STRETCH_MIN)
            + crate::dynamics::BULGE_MAX
            + LENS_PAD * 2.0;
        let node_left = lens_translation_x(max, node_width);
        let node_center = node_left + node_width * 0.5;
        assert!((node_center - (max + THUMB_WIDTH * 0.5)).abs() < 1.0e-5);
        assert!(node_width * 0.5 - LENS_WIDTH * 0.5 - LENS_TRAVEL_LEAN > 0.0);
    }

    #[test]
    fn toggle_lens_ride_uses_pointer_progress_while_dragging() {
        assert_eq!(lens_ride_x(Some(0.0), 20.0), THUMB_MARGIN);
        assert_eq!(
            lens_ride_x(Some(1.0), THUMB_MARGIN),
            TRACK_WIDTH - THUMB_MARGIN - THUMB_WIDTH
        );
        assert_eq!(lens_ride_x(None, 12.5), 12.5);
    }

    #[test]
    fn toggle_track_tint_waits_for_real_travel() {
        assert_eq!(track_tint_progress(0.20), 0.0);
        assert!(track_tint_progress(0.35) < 0.2);
        assert!((0.30..=0.40).contains(&track_tint_progress(0.70)));
        assert!((0.65..=0.75).contains(&track_tint_progress(1.0)));
    }

    #[test]
    fn toggle_track_color_sweeps_on_the_reference_clock() {
        // Measured on the toggle-press sheet (60 fps): sage appears ~17 ms
        // after release and the sweep completes ~150 ms later — no delay,
        // fast start.
        let AnimationType::Tween(spec) = toggle_track_motion() else {
            panic!("toggle track color needs a bounded transition");
        };
        assert_eq!(spec.delay_millis, 0);
        assert_eq!(spec.duration_millis, 150);
        assert_eq!(spec.easing, Easing::EaseOut);
    }

    #[test]
    fn toggle_silhouette_uses_reciprocal_shader_deformation() {
        let pose = crate::dynamics::LiquidPose {
            stretch: 1.25,
            ortho: 0.8,
            axis: (1.0, 0.0),
            ..Default::default()
        };
        let deformation = pose.deformation();
        assert_eq!(deformation.along(), 1.25);
        assert_eq!(deformation.across(), 0.8);
        assert!((deformation.along() * deformation.across() - 1.0).abs() < 1e-6);
        let cruise = crate::dynamics::LiquidPose {
            speed: 1100.0,
            ..Default::default()
        };
        assert_eq!(toggle_motion_bulge(cruise), 3.5);
    }

    #[test]
    fn released_toggle_holds_the_full_lens_before_fading() {
        let AnimationType::Tween(spec) = toggle_lens_release() else {
            panic!("toggle lens release needs an explicit linger interval");
        };
        assert_eq!(spec.delay_millis, LENS_RELEASE_LINGER_MS);
        assert_eq!(spec.duration_millis, LENS_RELEASE_FADE_MS);
        assert_eq!(spec.easing, Easing::EaseIn);
        // The reference lens stays readable ~750 ms after release
        // (toggle-press sheet still shows it at 883 ms) before the thumb
        // rematerializes.
        assert!((700..=900).contains(&(spec.delay_millis + spec.duration_millis)));
    }
}
