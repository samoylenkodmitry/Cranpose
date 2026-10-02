use std::{cell::Cell, sync::Arc};

use super::*;

#[test]
fn release_stream_matches_all_native_velocity_and_direction_captures() {
    let mut cases = 0;
    let mut frames = 0;
    for case in 0..6 {
        let runtime = cranpose_core::Runtime::new(Arc::new(cranpose_core::DefaultScheduler));
        let fling = SliderFling::new(0.5, runtime.handle());
        let reported = Cell::new(0.5);
        let mut squared = 0.0_f32;
        let mut maximum = 0.0_f32;
        let mut count = 0;
        for row in include_str!("../../../tests/fixtures/native_slider_release.csv")
            .lines()
            .skip(1)
        {
            let mut fields = row.split(',');
            if fields
                .next()
                .expect("case column")
                .parse::<usize>()
                .expect("native case")
                != case
            {
                continue;
            }
            let time = fields
                .next()
                .expect("time column")
                .parse::<f64>()
                .expect("native frame time");
            let initial = fields
                .next()
                .expect("initial column")
                .parse::<f32>()
                .expect("native held value");
            let velocity = fields
                .next()
                .expect("velocity column")
                .parse::<f32>()
                .expect("native pan velocity");
            let expected = fields
                .next()
                .expect("value column")
                .parse::<f32>()
                .expect("native bound value");
            if count == 0 {
                reported.set(initial);
                fling.synchronize(initial);
                fling.release(initial, velocity, 300.0, Some(1_000_000_000));
            }
            runtime
                .handle()
                .drain_frame_callbacks(1_000_000_000 + (time * 1e9).round() as u64);
            if let Some(next) = fling.sample() {
                fling.publish(next, &|next| reported.set(next));
            }
            fling.synchronize(reported.get());
            let error = (reported.get() - expected).abs() * 263.0;
            squared += error * error;
            maximum = maximum.max(error);
            count += 1;
        }
        let rms = (squared / count as f32).sqrt();
        assert_eq!(
            count, 120,
            "retain every native release frame in case {case}"
        );
        assert!(
            rms < 0.75 && maximum < 2.0,
            "native case {case}: {rms} pt RMS / {maximum} pt maximum"
        );
        assert!(
            !fling.animation.borrow().is_running(),
            "case {case}: settled release must stop scheduling frames"
        );
        frames += count;
        cases += 1;
    }
    assert_eq!((cases, frames), (6, 720));
}

#[test]
fn a_new_grab_or_external_value_cancels_release_notifications() {
    for external in [false, true] {
        let runtime = cranpose_core::Runtime::new(Arc::new(cranpose_core::DefaultScheduler));
        let fling = SliderFling::new(0.5, runtime.handle());
        fling.release(0.5, 400.0, 300.0, Some(1_000_000_000));
        runtime.handle().drain_frame_callbacks(1_050_000_000);
        let before = fling.sample().expect("fling is moving");
        assert!(before > 0.5);
        if external {
            fling.synchronize(0.2);
        } else {
            fling.cancel();
        }
        runtime.handle().drain_frame_callbacks(2_000_000_000);
        assert_eq!(fling.sample(), None);
        assert!(!fling.animation.borrow().is_running());
        fling.publish(0.6, &|_| panic!("cancelled release must not call back"));
    }
}

#[test]
fn a_settled_release_returns_control_to_an_unchanged_caller_value() {
    let runtime = cranpose_core::Runtime::new(Arc::new(cranpose_core::DefaultScheduler));
    let fling = SliderFling::new(0.5, runtime.handle());
    fling.release(0.5, 400.0, 300.0, Some(1_000_000_000));
    runtime.handle().drain_frame_callbacks(1_050_000_000);
    let next = fling.sample().expect("release is moving");
    fling.publish(next, &|_| {});
    fling.synchronize(0.5);
    runtime.handle().drain_frame_callbacks(3_000_000_000);
    fling.publish(fling.sample().expect("last release value"), &|_| {});
    assert_eq!(fling.sample(), None, "the caller still owns the value");
    assert!(!fling.animation.borrow().is_running());
}
