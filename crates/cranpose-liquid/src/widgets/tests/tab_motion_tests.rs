use super::*;

#[test]
fn reversing_motion_preserves_the_stretch_and_rebound() {
    let runtime = cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
    let forward = TabLensShape::new(runtime.handle());
    let reverse = TabLensShape::new(runtime.handle());
    let mut greatest = 0.0f32;
    for frame in 0..240 {
        runtime
            .handle()
            .drain_frame_callbacks(1_000_000_000 + frame * 8_333_333);
        let position = ((frame as f32 - 96.0) * 8.0).clamp(0.0, 285.5);
        let a = forward.sample(position);
        let b = reverse.sample(285.5 - position);
        greatest = greatest.max(a.width.abs());
        assert!(
            (a.width - b.width).abs() < 0.00001 && (a.height - b.height).abs() < 0.00001,
            "direction changed strain at frame {frame}: {a:?}, {b:?}"
        );
    }
    assert!(greatest > 0.05);
}

#[test]
fn contact_light_tracks_native_onset_and_keeps_the_dimmed_release() {
    let runtime = cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
    let motion = TabContactMotion::new(runtime.handle());
    let mut route = usize::MAX;
    let mut error = 0.0;
    let mut samples = 0;
    for row in include_str!("../../tests/fixtures/native_tab_glow.csv")
        .lines()
        .skip(1)
    {
        let fields = row
            .split(',')
            .map(|v| v.parse::<f64>().unwrap())
            .collect::<Vec<_>>();
        let next = fields[0] as usize;
        let origin = (next as u64 + 1) * 10_000_000_000;
        if next != route {
            motion.glow.borrow_mut().snapTo(0.0);
            motion.pressed(true, Some(origin));
            route = next;
        }
        runtime
            .handle()
            .drain_frame_callbacks(origin + (fields[1] * 1e9) as u64);
        error += (f64::from(motion.glow_state().get()) - fields[2]).powi(2);
        samples += 1;
    }
    assert!(samples >= 50);
    assert!((error / f64::from(samples)).sqrt() < 0.03);
    motion.crossed_tab();
    for frame in 1..=120 {
        runtime
            .handle()
            .drain_frame_callbacks(40_250_000_000 + frame * 16_666_667);
    }
    assert_eq!(motion.local_glow_factor_state().get(), 0.5);
    motion.pressed(false, Some(42_250_000_000));
    runtime.handle().drain_frame_callbacks(42_350_000_000);
    assert!((motion.glow_state().get() - 0.64).abs() < 0.03);
    assert_eq!(motion.local_glow_factor_state().get(), 0.5);
}

#[test]
fn contact_clock_integrates_from_input_and_keeps_release_velocity() {
    let runtime = cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
    let motion = TabContactMotion::new(runtime.handle());
    runtime.handle().drain_frame_callbacks(1_000_000_000);
    motion.pressed(true, Some(1_000_000_000));
    runtime.handle().drain_frame_callbacks(1_050_000_000);
    let before = motion.lens_state().value();
    assert!(before > 0.34 && before < 0.35, "contact sample: {before}");
    assert!(motion.bar_state().value() > 0.1);
    let velocity = motion.lens.borrow().velocity();
    motion.pressed(false, Some(1_050_000_000));
    assert_eq!(motion.lens.borrow().velocity(), velocity);
    for frame in 1..=120 {
        runtime
            .handle()
            .drain_frame_callbacks(1_050_000_000 + frame * 16_666_667);
    }
    assert_eq!(motion.lens_state().value(), 0.0);
    assert_eq!(motion.bar_state().value(), 0.0);
}

#[test]
fn contact_without_input_time_uses_the_shared_frame_clock() {
    let runtime = cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
    let motion = TabContactMotion::new(runtime.handle());
    runtime.handle().drain_frame_callbacks(1_000_000_000);
    motion.pressed(true, None);
    runtime.handle().drain_frame_callbacks(1_050_000_000);
    assert!(motion.lens_state().value() > 0.2);
    assert!(motion.bar_state().value() > 0.1);
}

#[test]
fn stopped_shape_returns_to_exact_zero() {
    let runtime = cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
    let shape = TabLensShape::new(runtime.handle());
    shape
        .width
        .strain
        .borrow_mut()
        .animateTo(0.0005, spring(0.4, 35.0));
    for frame in 0..600 {
        runtime
            .handle()
            .drain_frame_callbacks(1_000_000_000 + frame * 16_666_667);
        shape.sample(0.0);
    }
    assert_eq!(shape.width.strain.borrow().target(), 0.0);
    assert_eq!(shape.width.strain.borrow().state().value(), 0.0);
    assert!(!shape.width.strain.borrow().is_running());
}

#[test]
fn shape_response_tracks_native_reversal_microframes() {
    let runtime = cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
    let mut shape = TabLensShape::new(runtime.handle());
    let mut errors = [[0.0f32; 2]; 8];
    let mut counts = [0; 8];
    let mut extrema = [0.0f32; 2];
    let mut route = usize::MAX;
    let mut output = String::from("route,time,x,native_width,native_height,width,height\n");
    for line in include_str!("../../tests/fixtures/native_tab_shape.csv")
        .lines()
        .skip(1)
    {
        let fields: Vec<f64> = line.split(',').map(|v| v.parse().unwrap()).collect();
        let next = fields[0] as usize;
        if route != next {
            route = next;
            shape = TabLensShape::new(runtime.handle());
        }
        runtime
            .handle()
            .drain_frame_callbacks((route as u64 + 1) * 10_000_000_000 + (fields[1] * 1e9) as u64);
        let actual = shape.sample(fields[2] as f32);
        output.push_str(&format!(
            "{route},{},{},{},{},{},{}\n",
            fields[1], fields[2], fields[3], fields[4], actual.width, actual.height
        ));
        for (axis, value) in [actual.width, actual.height].into_iter().enumerate() {
            errors[route][axis] += (value - fields[axis + 3] as f32).powi(2);
        }
        extrema[0] = extrema[0].max(actual.width);
        extrema[1] = extrema[1].min(actual.width);
        counts[route] += 1;
    }
    if let Ok(path) = std::env::var("CRANPOSE_OPTICAL_DEBUG") {
        std::fs::write(std::path::Path::new(&path).join("strain.csv"), output).unwrap();
    }
    assert!(counts.iter().all(|count| *count > 350));
    assert!(
        extrema[0] > 0.12 && extrema[1] < -0.12,
        "missing stretch/rebound: {extrema:?}"
    );
    for (route, axes) in errors.into_iter().enumerate() {
        for (axis, error) in axes.into_iter().enumerate() {
            let rms = (error / counts[route] as f32).sqrt();
            assert!(
                rms < 0.025,
                "native route {route} axis {axis} strain RMS: {rms}"
            );
        }
    }
}
