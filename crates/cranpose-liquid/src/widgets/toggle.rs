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
use cranpose_ui_graphics::{
    Brush, CornerRadii, GlassProfileCurve, GlassSurfaceProfile, GraphicsLayer,
};

pub(crate) const TRACK_WIDTH: f32 = 63.0;
pub(crate) const TRACK_HEIGHT: f32 = 28.0;
const THUMB_WIDTH: f32 = 37.0;
const THUMB_HEIGHT: f32 = 25.0;
const THUMB_MARGIN: f32 = 1.5;
/// The pressed lens capsule rides the full track width and grows past the
/// track edges, matching the reference's refractive chamber.
const LENS_WIDTH: f32 = 58.0;
const LENS_HEIGHT: f32 = 39.0;
const LENS_VERTICAL_OFFSET: f32 = 0.0;
/// Small outward lean at each resting end. Centering the 58dp lens on the
/// 37dp thumb provides most of the measured overhang; this adds the remaining
/// state-dependent travel without biasing the entire OFF→ON ride rightward.
const LENS_OUTWARD_LEAN: f32 = 7.5;
/// Glass node span beyond the lens shape (rim glow + wobble live here).
const LENS_PAD: f32 = 10.0;
/// Pointer travel below this is a tap, not a swipe.
const TAP_SLOP: f32 = 4.0;
const LENS_RELEASE_LINGER_MS: u64 = 520;
const LENS_RELEASE_FADE_MS: u64 = 140;

fn toggle_surface_profile() -> GlassSurfaceProfile {
    let shared = GlassSurfaceProfile::lens();
    let x_profile = GlassProfileCurve::from_points(&[
        (0.0, 1.00),
        (0.18, 0.995),
        (0.28, 0.96),
        (0.40, 0.55),
        (0.65, 0.12),
        (1.0, 0.00),
    ])
    .expect("toggle X-Z profile is valid");
    let y_profile = GlassProfileCurve::from_points(&[
        (0.0, 1.00),
        (0.58, 0.998),
        (0.74, 0.97),
        (0.86, 0.10),
        (0.94, 0.58),
        (1.0, 0.98),
    ])
    .expect("toggle Y-Z profile is valid");
    GlassSurfaceProfile::new(x_profile, y_profile, 8.0, shared.radial_power())
        .expect("toggle surface profile is coherent")
}

fn toggle_lens_material() -> Glass {
    Glass::lens()
        .shape(LiquidShape::Capsule)
        .tint(cranpose_ui_graphics::Color::WHITE.with_alpha(0.02))
        .blur_radius(1.5)
        .saturation(1.0)
        .chromatic_aberration(1.8)
        .dispersion_axes(0.10, 1.69)
        .displacement(24.0)
        .highlight(0.40)
        .surface_profile(toggle_surface_profile())
        .sheen(0.12)
        .lift(-0.045)
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
        AnimationSpec::tween(LENS_RELEASE_FADE_MS, Easing::EaseOut)
            .with_delay(LENS_RELEASE_LINGER_MS),
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TrackFillGeometry {
    radius: f32,
    width: f32,
    right_cap_center: f32,
}

fn track_fill_geometry(thumb_x: f32) -> TrackFillGeometry {
    let radius = TRACK_HEIGHT * 0.5;
    let max_x = TRACK_WIDTH - THUMB_MARGIN - THUMB_WIDTH;
    let progress = ((thumb_x - THUMB_MARGIN) / (max_x - THUMB_MARGIN)).clamp(0.0, 1.0);
    let travelling_width = thumb_x + THUMB_WIDTH * 0.5 + radius;
    let width = if progress >= 0.98 {
        TRACK_WIDTH
    } else {
        travelling_width.clamp(TRACK_HEIGHT, TRACK_WIDTH)
    };
    TrackFillGeometry {
        radius,
        width,
        right_cap_center: width - radius,
    }
}

fn track_tint_progress(progress: f32) -> f32 {
    let t = ((progress.clamp(0.0, 1.0) - 0.20) / 0.65).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lens_translation_x(thumb_x: f32, node_width: f32, outward_lean: f32) -> f32 {
    let min_x = THUMB_MARGIN;
    let max_x = TRACK_WIDTH - THUMB_MARGIN - THUMB_WIDTH;
    let side = if max_x > min_x {
        ((thumb_x - min_x) / (max_x - min_x) * 2.0 - 1.0).clamp(-1.0, 1.0)
    } else {
        0.0
    };
    thumb_x + (THUMB_WIDTH - node_width) * 0.5 + side * outward_lean
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
    // OFF track: sampled from the reference video's resting switch —
    // srgb(187,186,188), a solid medium gray, darker than iOS systemGray4.
    let off_track = cranpose_ui_graphics::Color::from_rgb_u8(187, 186, 188);
    // Mid-drag the track color follows the finger (the reference shows the
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
    let base_track = match drag_progress.get() {
        Some(progress) => drag_mix(off_track, colors.success, track_tint_progress(progress)),
        None => {
            if checked {
                colors.success
            } else {
                off_track
            }
        }
    };
    // The uncovered tail retains the system accent. The lens itself creates
    // the lighter, lower-saturation interior through refraction; applying
    // that tone transform to the whole track washes out the reference color.
    let animated_track = animateColorAsState(base_track, LiquidMotion::smooth(), "toggle-track");
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
    let settling = !lens_axis.is_dragging() && (lens_x - target_x).abs() > 0.75;
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
                            let mut dragging = false;
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                match event.kind {
                                    PointerEventKind::Down => {
                                        dragging = true;
                                        down_x = event.position.x;
                                        lens_axis.begin(thumb_x.get(), event.time_ms);
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
                                        lens_axis.release_to(
                                            if next { max_x } else { min_x },
                                            event.time_ms,
                                            LiquidMotion::snappy(),
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
            // The color commits with the thumb: everything the thumb has
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
            let fill = track_fill_geometry(lens_ride_x(drag_progress.get(), thumb_x.get()));
            let split = fill.width.min(size.width);
            let radius = fill.radius;
            let color = track_color;
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
                    width: (split - 2.0 * radius).max(0.0),
                    height: size.height,
                },
                Brush::solid(color),
            );
            scope.draw_circle(
                Brush::solid(color),
                cranpose_ui_graphics::Point::new(fill.right_cap_center, radius),
                radius,
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
                .graphics_layer(move || {
                    // The lens leans past the thumb toward its side of
                    // travel (the reference lens overhangs the track end).
                    GraphicsLayer {
                        translation_x: lens_translation_x(
                            lens_x,
                            node_w,
                            if drag_progress.get().is_none() {
                                LENS_OUTWARD_LEAN
                            } else {
                                0.0
                            },
                        ),
                        alpha: (lens_for_layer.get() * 2.5).clamp(0.0, 1.0),
                        ..Default::default()
                    }
                })
                .glass_effect_with(toggle_lens_material(), move || {
                    let grow = lens_for_layer.get().clamp(0.0, 1.2);
                    let base_w = THUMB_WIDTH + (LENS_WIDTH - THUMB_WIDTH) * grow;
                    let base_h = THUMB_HEIGHT + (LENS_HEIGHT - THUMB_HEIGHT) * grow;
                    // Droplet law over the ride position: drag speed
                    // stretches the lens along the track, braking swells
                    // its leading edge (crate::dynamics).
                    let pose = physics_axis.liquid_pose();
                    GlassDynamics {
                        morph: Some(GlassMorph {
                            node_size: (node_w, node_h),
                            primary: (node_w * 0.5, node_h * 0.5, base_w, base_h, -1.0),
                            shapes: Vec::new(),
                            glue: 0.0,
                            wobble_amplitude: 0.0,
                            wobble_phase: 0.0,
                            bulge_amplitude: toggle_motion_bulge(pose),
                            bulge_direction: pose.bulge_direction,
                            ellipse_blend: 0.0,
                            deformation: Some(pose.deformation()),
                        }),
                        surface_depth_boost: 0.12 * pose.energy(),
                        ..Default::default()
                    }
                });
            Box(lens, BoxSpec::default(), || {});
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_geometry_matches_the_reference_proportions() {
        assert_eq!((TRACK_WIDTH, TRACK_HEIGHT), (63.0, 28.0));
        assert_eq!((THUMB_WIDTH, THUMB_HEIGHT), (37.0, 25.0));
        assert_eq!((LENS_WIDTH, LENS_HEIGHT), (58.0, 39.0));
        assert_eq!(
            toggle_lens_material().shape,
            LiquidShape::Capsule,
            "the pressed switch thumb remains a capsule while its optical body inflates"
        );
        assert_eq!(
            LENS_VERTICAL_OFFSET, 0.0,
            "the measured body is centered on the track; optical extent comes from refraction"
        );
        assert_eq!(
            (LENS_HEIGHT - TRACK_HEIGHT) * 0.5 - LENS_VERTICAL_OFFSET,
            5.5
        );
        assert_eq!(
            (LENS_HEIGHT - TRACK_HEIGHT) * 0.5 + LENS_VERTICAL_OFFSET,
            5.5
        );

        let fill = track_fill_geometry(TRACK_WIDTH - THUMB_MARGIN - THUMB_WIDTH);
        assert_eq!(fill.width, TRACK_WIDTH);
        assert_eq!(
            fill.right_cap_center,
            TRACK_WIDTH - TRACK_HEIGHT * 0.5,
            "the active track must end in the rounded cap magnified by the lens"
        );
        let thumb_mid = (THUMB_MARGIN + TRACK_WIDTH - THUMB_MARGIN - THUMB_WIDTH) * 0.5;
        let travelling = track_fill_geometry(thumb_mid);
        assert!(
            (travelling.right_cap_center - (thumb_mid + THUMB_WIDTH * 0.5)).abs() < 1e-4,
            "the traveling color boundary must follow the thumb center"
        );
    }

    #[test]
    fn toggle_lens_has_the_reference_rim_energy() {
        let glass = toggle_lens_material();
        let x_profile = glass.surface_profile.x_profile();
        let y_profile = glass.surface_profile.y_profile();
        let (_, side_wall_slope) = x_profile.evaluate(0.35);
        let (_, side_slope) = x_profile.evaluate(0.82);
        let (inner_height, inner_slope) = y_profile.evaluate(0.70);
        let (shoulder_height, meniscus_slope) = y_profile.evaluate(0.80);
        let (trough_height, _) = y_profile.evaluate(0.87);
        let (_, return_slope) = y_profile.evaluate(0.90);
        let (_, outer_slope) = y_profile.evaluate(0.97);
        assert!(
            inner_height >= 0.95 && inner_slope.abs() <= 0.35,
            "the short-axis dome must stay optically flat until the outer meniscus"
        );
        assert!(
            meniscus_slope <= -4.0,
            "the outer meniscus must bend sharply toward the exterior"
        );
        assert!(
            (0.40..=0.65).contains(&shoulder_height) && trough_height < 0.18,
            "the short-axis trough must sit in the measured outer interval: shoulder={shoulder_height}, trough={trough_height}"
        );
        assert!(
            return_slope >= 2.0,
            "the trough must return inward backdrop content into the outer caustic"
        );
        assert!(
            outer_slope > 0.0,
            "the return must reach the silhouette without a second bevel"
        );
        for radius in [0.87, 0.93, 0.98] {
            let (_, slope) = y_profile.evaluate(radius);
            assert!(
                slope >= 0.0,
                "the Y-Z surface must contain one trough and no repeated bevel: radius={radius}, slope={slope}"
            );
        }
        assert!(
            side_slope <= 0.0,
            "the X-Z wall is monotone; a signed horizontal return paints a four-sided rainbow ring"
        );
        assert!(
            side_wall_slope <= -3.5,
            "the domed side wall must displace the track boundary into the white chamber"
        );
        assert!(
            (1.7..=1.9).contains(&glass.chromatic_aberration),
            "toggle dispersion must match the measured cyan/yellow caustic coverage: {}",
            glass.chromatic_aberration
        );
        assert!(
            (0.08..=0.12).contains(&glass.dispersion_axes.0)
                && (1.64..=1.74).contains(&glass.dispersion_axes.1),
            "the compact lens separates wavelengths along its principal profile curvature: {:?}",
            glass.dispersion_axes
        );
        assert!(
            glass
                .blur_radius
                .is_some_and(|radius| (1.0..=2.0).contains(&radius)),
            "the source needs a small optical prefilter so the signed folds merge into one caustic"
        );
        assert!(
            (0.35..=0.45).contains(&glass.highlight),
            "toggle meniscus bands must stay visible without becoming a white ring"
        );
        assert_eq!(glass.surface_profile, toggle_surface_profile());
        assert!((7.8..=8.2).contains(&glass.surface_profile.depth()));
        assert!(
            (22.0..=26.0).contains(&glass.displacement),
            "toggle rim must bend the track edge without duplicating it across the chamber"
        );
        assert!(
            glass
                .lift
                .is_some_and(|lift| (-0.06..=-0.03).contains(&lift)),
            "the saturated track must remain unchanged through the clear face"
        );
        assert!(glass.tint.is_some_and(|tint| tint.a() <= 0.03));
        assert!(glass
            .saturation
            .is_some_and(|saturation| (0.98..=1.02).contains(&saturation)));
        assert!(
            glass
                .sheen
                .is_some_and(|sheen| (0.08..=0.16).contains(&sheen)),
            "toggle needs separated caustic lobes instead of a continuous white ring"
        );
        assert_eq!(
            glass.lift,
            Some(-0.045),
            "the recessed face must preserve the target track saturation"
        );
        let shadow = glass.shadow_style.expect("toggle shadow override");
        assert!((shadow.color.r() - shadow.color.g()).abs() < 1e-6);
        assert!((12.0..=16.0).contains(&shadow.radius) && shadow.offset_y <= 4.0);
    }

    #[test]
    fn toggle_x_profile_folds_the_on_track_endpoint_into_the_face() {
        let max_thumb_x = TRACK_WIDTH - THUMB_MARGIN - THUMB_WIDTH;
        let lens_center = max_thumb_x + THUMB_WIDTH * 0.5 + LENS_OUTWARD_LEAN;
        let endpoint_radius = (TRACK_WIDTH - lens_center).abs() / (LENS_WIDTH * 0.5);
        let glass = toggle_lens_material();
        let (_, endpoint_slope) = glass.surface_profile.x_profile().evaluate(endpoint_radius);
        let bend = 1.0 - 1.0 / 1.5;
        let endpoint_deflection = endpoint_slope.abs() * glass.surface_profile.depth()
            / (LENS_WIDTH * 0.5)
            * bend
            * glass.displacement;
        assert!(
            endpoint_slope < 0.0 && endpoint_deflection >= 4.5,
            "the physical X-Z shoulder must fold the raw ON endpoint inward: radius={endpoint_radius}, slope={endpoint_slope}, deflection={endpoint_deflection}"
        );
    }

    #[test]
    fn toggle_x_profile_does_not_flatten_after_the_inward_fold() {
        let profile = toggle_surface_profile();
        let x_profile = profile.x_profile();
        for (radius, maximum_slope) in [(0.45, -1.5), (0.55, -1.5), (0.65, -0.5)] {
            let (_, slope) = x_profile.evaluate(radius);
            assert!(
                slope <= maximum_slope,
                "the X-Z dome must keep bending through the raw endpoint: radius={radius}, slope={slope}"
            );
        }
    }

    #[test]
    fn toggle_lens_overhangs_toward_each_resting_side() {
        let node_width =
            LENS_WIDTH * crate::dynamics::STRETCH_MAX + crate::dynamics::BULGE_MAX + LENS_PAD * 2.0;
        let min = THUMB_MARGIN;
        let max = TRACK_WIDTH - THUMB_MARGIN - THUMB_WIDTH;
        let primary_left = (node_width - LENS_WIDTH) * 0.5;
        let off_left = lens_translation_x(min, node_width, LENS_OUTWARD_LEAN) + primary_left;
        let on_right =
            lens_translation_x(max, node_width, LENS_OUTWARD_LEAN) + primary_left + LENS_WIDTH;
        assert!(
            (-17.0..=-16.0).contains(&off_left),
            "OFF glass must match the symmetric track overhang, got {off_left}"
        );
        assert!(
            (TRACK_WIDTH + 16.0..=TRACK_WIDTH + 17.0).contains(&on_right),
            "ON glass must match the measured right-track overhang, got {on_right}"
        );
        let off_center = off_left + LENS_WIDTH * 0.5;
        let on_center = on_right - LENS_WIDTH * 0.5;
        assert!((37.0..=39.0).contains(&(on_center - off_center)));
        let direct_left = lens_translation_x(min, node_width, 0.0) + primary_left;
        assert!((direct_left + 9.0).abs() < 1e-4);
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
        assert!(track_tint_progress(0.70) > 0.8);
        assert_eq!(track_tint_progress(1.0), 1.0);
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
        assert_eq!(spec.delay_millis, 520);
        assert_eq!(spec.duration_millis, LENS_RELEASE_FADE_MS);
        assert_eq!(LENS_RELEASE_LINGER_MS, 520);
    }
}
