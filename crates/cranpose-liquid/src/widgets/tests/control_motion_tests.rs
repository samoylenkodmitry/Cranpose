use super::*;

#[test]
fn contact_frames_track_native_size_and_white_fill_independently() {
    for (name, kind, rest, held) in [
        (
            "slider",
            ControlContactKind::Thumb,
            (37.0, 24.0),
            (57.35, 37.2),
        ),
        (
            "toggle",
            ControlContactKind::Thumb,
            (37.0, 24.0),
            (58.0, 115.0 / 3.0),
        ),
        (
            "segmented",
            ControlContactKind::Segment,
            (96.0, 28.0),
            (120.0, 44.0),
        ),
    ] {
        for down in [true, false] {
            let runtime =
                cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
            let motion = ControlContactMotion::new(runtime.handle(), kind);
            if !down {
                for channel in motion.channels.borrow_mut().iter_mut() {
                    channel.snapTo(1.0);
                }
            }
            let origin = 1_000_000_000;
            runtime.handle().drain_frame_callbacks(origin);
            motion.pressed(down, Some(origin));
            let (geometry, material) = motion.states();
            let mut size_error = 0.0;
            let mut fill_error = 0.0;
            let mut count = 0;
            for row in include_str!("../../../tests/fixtures/native_control_contact.csv")
                .lines()
                .skip(1)
            {
                let fields = row.split(',').collect::<Vec<_>>();
                if fields[0] != name || (fields[1] == "press") != down {
                    continue;
                }
                let values = fields[2..]
                    .iter()
                    .map(|value| value.parse::<f64>().expect("native sample"))
                    .collect::<Vec<_>>();
                runtime
                    .handle()
                    .drain_frame_callbacks(origin + (values[0].max(0.0) * 1e9) as u64);
                let progress = f64::from(geometry.get());
                for (actual, expected) in [
                    (rest.0 + (held.0 - rest.0) * progress, values[1]),
                    (rest.1 + (held.1 - rest.1) * progress, values[2]),
                ] {
                    size_error += (actual - expected).powi(2);
                }
                fill_error += (f64::from((1.0 - material.get()).clamp(0.0, 1.0))
                    - values[3].clamp(0.0, 1.0))
                .powi(2);
                count += 1;
            }
            assert!(count >= 20);
            let size_rms = (size_error / f64::from(count * 2)).sqrt();
            let fill_rms = (fill_error / f64::from(count)).sqrt();
            assert!(
                size_rms < 1.1,
                "{name} down={down} native size RMS: {size_rms}"
            );
            assert!(
                fill_rms < 0.06,
                "{name} down={down} native fill RMS: {fill_rms}"
            );
        }
    }
}

#[test]
fn releasing_during_inflation_preserves_motion_then_settles_without_a_linger() {
    let runtime = cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
    let motion = ControlContactMotion::new(runtime.handle(), ControlContactKind::Thumb);
    motion.pressed(true, Some(1_000_000_000));
    runtime.handle().drain_frame_callbacks(1_080_000_000);
    let velocity = motion.channels.borrow()[0].velocity();
    motion.pressed(false, Some(1_080_000_000));
    assert_eq!(motion.channels.borrow()[0].velocity(), velocity);
    for frame in 1..=120 {
        runtime
            .handle()
            .drain_frame_callbacks(1_080_000_000 + frame * 16_666_667);
    }
    let (geometry, material) = motion.states();
    assert_eq!(geometry.get(), 0.0);
    assert_eq!(material.get(), 0.0);
}
