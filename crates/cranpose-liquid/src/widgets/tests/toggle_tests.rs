use super::*;

#[test]
fn toggle_lens_stays_centered_on_the_thumb_at_both_endpoints() {
    let node_width = 130.0;
    for (progress, expected) in [(0.0, 20.5), (1.0, 42.5)] {
        let thumb_x = lens_ride_x(Some(progress), 0.0);
        let center = lens_translation_x(thumb_x, node_width) + node_width * 0.5;
        assert_eq!(center, expected);
    }
    assert_eq!(lens_ride_x(None, 12.5), 12.5);
}

#[test]
fn switch_semantics_exposes_the_committed_value() {
    for checked in [false, true] {
        let mut config = cranpose_ui::SemanticsConfiguration::default();
        switch_semantics(checked)(&mut config);
        assert_eq!(config.toggled, Some(checked.into()));
        assert_eq!(config.role, Some(cranpose_ui::SemanticsWidgetRole::Switch));
        assert!(config.is_clickable);
    }
}

#[test]
fn the_track_recolors_on_the_native_spring_after_a_tap() {
    let runtime = cranpose_core::Runtime::new(Arc::new(cranpose_core::DefaultScheduler));
    let mut fill = Animatable::new(0.0_f32, runtime.handle());
    let origin = 1_000_000_000_u64;
    let release = origin + crate::widgets::native_toggle_tap::RELEASE_NANOS;
    runtime.handle().drain_frame_callbacks(release);
    fill.animate_to_at(1.0, toggle_fill_motion(), release);
    let (mut error, mut count) = (0.0, 0);
    for frame in crate::widgets::native_toggle_tap::frames() {
        let time = origin + frame.time_nanos;
        if time < release {
            continue;
        }
        runtime.handle().drain_frame_callbacks(time);
        error += (f64::from(fill.state().get()) - frame.track_fill).powi(2);
        count += 1;
    }
    assert!(count >= 40);
    let rms = (error / f64::from(count)).sqrt();
    assert!(rms < 0.02, "native track fill RMS: {rms}");
}

#[test]
fn the_track_paints_one_flat_color_at_rest_and_the_strip_while_it_moves() {
    let colors = Arc::<[Color]>::from([
        Color::from_rgb_u8(0, 133, 252),
        Color::from_rgb_u8(198, 198, 201),
    ]);
    assert_eq!(track_brush(0.0, &colors), Brush::solid(colors[1]));
    assert_eq!(track_brush(1.0, &colors), Brush::solid(colors[0]));
    let Brush::LinearGradient { start, end, .. } = track_brush(0.5, &colors) else {
        panic!("a moving fill paints the strip");
    };
    // Halfway, the blend covers the track, its on side leading from the left.
    assert!(start.x < 0.0 && end.x > TRACK_WIDTH);
    assert!((end.x - start.x - 8.0 * TRACK_WIDTH).abs() < 1e-3);
}
