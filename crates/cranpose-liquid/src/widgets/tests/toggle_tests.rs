use super::*;

#[test]
fn toggle_geometry_matches_the_reference_proportions() {
    assert_eq!((TRACK_WIDTH, TRACK_HEIGHT), (63.0, 28.0));
    assert_eq!((THUMB_WIDTH, THUMB_HEIGHT), (37.0, 25.0));
    assert_eq!((LENS_WIDTH, LENS_HEIGHT), (54.0, 36.0));
    const { assert!(LENS_WIDTH > THUMB_WIDTH) };
    const { assert!(LENS_HEIGHT > TRACK_HEIGHT) };
    assert_eq!(
        toggle_lens_material().shape,
        LiquidShape::Capsule,
        "the pressed switch thumb remains a capsule while its optical body inflates"
    );
    assert!(LENS_VERTICAL_OFFSET.abs() < 1.0e-6);

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
    assert_eq!(LENS_TRAVEL_LEAN, 7.0);
    assert_eq!(lens_press_travel(false), 1.0);
    assert_eq!(lens_press_travel(true), -1.0);

    let min = THUMB_MARGIN;
    let max = TRACK_WIDTH - THUMB_MARGIN - THUMB_WIDTH;
    let mid_thumb_center = (min + max) * 0.5 + THUMB_WIDTH * 0.5;
    let lens_trailing_edge = mid_thumb_center + LENS_TRAVEL_LEAN - LENS_WIDTH * 0.5;
    assert!(lens_trailing_edge > 5.0);

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
    let AnimationType::Tween(spec) = toggle_track_motion() else {
        panic!("toggle track color needs a bounded transition");
    };
    assert_eq!(spec.delay_millis, 0);
    assert_eq!(spec.duration_millis, 260);
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
    assert!((700..=900).contains(&(spec.delay_millis + spec.duration_millis)));
}
