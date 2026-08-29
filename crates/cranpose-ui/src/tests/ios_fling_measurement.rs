
use std::{cell::Cell, rc::Rc, sync::Arc};

use cranpose_core::{DefaultScheduler, Runtime};

use crate::{
    fling_animation::{FlingAnimation, SettleAnimation, SpringParams},
    scroll::OverscrollEffect,
};

const NORMAL_RATE_TRACE: (f32, f32, &[(f32, f32)]) = (
    400.0,
    480.833_66,
    &[
        (0.290, 408.0000),
        (100.213, 449.6667),
        (199.568, 484.0000),
        (299.717, 512.3333),
        (400.298, 535.3333),
        (500.281, 554.3333),
        (600.284, 570.0000),
        (700.302, 582.6667),
        (799.691, 593.0000),
        (900.283, 601.6667),
        (1000.261, 608.6667),
        (1100.212, 614.3333),
        (1199.606, 619.0000),
        (1299.828, 622.6667),
        (1400.246, 625.6667),
        (1_500.25, 628.3333),
        (1599.643, 630.3333),
        (1700.271, 632.3333),
        (1850.238, 634.3333),
        (1916.913, 635.0000),
    ],
);

const TRAJECTORY_TOLERANCE_PT: f32 = 10.0;

#[test]
fn fling_animation_trajectory_matches_recorded_ios_trace() {
    let (initial_value, velocity, trace) = NORMAL_RATE_TRACE;
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let handle = runtime.handle();
    let fling = FlingAnimation::new(handle.clone());
    let position = Rc::new(Cell::new(initial_value));
    let position_for_scroll = Rc::clone(&position);

    fling.start_fling(
        initial_value,
        velocity,
        move |delta| {
            position_for_scroll.set(position_for_scroll.get() + delta);
            delta
        },
        || {},
    );
    handle.drain_frame_callbacks(0);

    let mut max_abs_error = 0.0f32;
    for &(t_ms, recorded_y) in trace {
        handle.drain_frame_callbacks((t_ms as f64 * 1_000_000.0) as u64);
        let error = (position.get() - recorded_y).abs();
        max_abs_error = max_abs_error.max(error);
        assert!(
            error < TRAJECTORY_TOLERANCE_PT,
            "at t={t_ms}ms: cranpose={got}, iOS recorded={recorded_y}, error={error}pt exceeds {TRAJECTORY_TOLERANCE_PT}pt",
            got = position.get(),
        );
    }
    assert!(
        max_abs_error > 0.0,
        "trace replay produced zero error at every sample; the trace is not exercising the decay law"
    );
}

const TARGET_OFFSET_SAMPLES: &[(f32, f32, f32, f32)] = &[
    (0.998, 400.00, 0.480_834, 635.333_3),
    (0.998, 1244.67, 0.495_222, 1487.0),
    (0.998, 1755.67, 0.307_760, 1_904.333_4),
    (0.998, 2010.00, 0.262_579, 2136.0),
    (0.998, 2315.67, 0.677_054, 2649.0),
    (0.998, 2912.67, 1.381_048, 3_597.666_7),
    (0.998, 4029.00, 2.530_246, 5288.0),
    (0.998, 5686.67, 5.613_967, 8486.0),
    (0.99, 8662.33, 0.756_662, 8_736.667),
    (0.99, 9000.33, 1.193_017, 9118.0),
    (0.99, 9549.33, 2.347_413, 9782.0),
    (0.99, 10180.67, 5.991_895, 10776.0),
];

const TARGET_OFFSET_TOLERANCE_PT: f32 = 5.2;

#[test]
fn decay_spec_target_matches_recorded_ios_target_content_offset_both_rates() {
    use cranpose_animation::{ExponentialDecaySpec, FloatDecayAnimationSpec};

    for &(rate, offset, velocity_pt_per_ms, ios_target) in TARGET_OFFSET_SAMPLES {
        let spec = ExponentialDecaySpec::new(rate);
        let velocity_pt_per_sec = velocity_pt_per_ms * 1000.0;
        let target = spec.get_target_value(offset, velocity_pt_per_sec);
        let error = (target - ios_target).abs();
        assert!(
            error < TARGET_OFFSET_TOLERANCE_PT,
            "rate={rate} offset={offset} v={velocity_pt_per_sec}pt/s: cranpose target={target}, iOS target={ios_target}, error={error}pt"
        );
    }
}

#[test]
fn fling_rest_position_matches_recorded_ios_target_content_offset() {
    use crate::fling_animation::{MIN_FLING_VELOCITY, fling_rest_position};

    for &(rate, offset, velocity_pt_per_ms, ios_target) in TARGET_OFFSET_SAMPLES {
        if rate != cranpose_animation::IOS_DECELERATION_RATE_NORMAL {
            continue;
        }
        let velocity_pt_per_sec = velocity_pt_per_ms * 1000.0;
        if velocity_pt_per_sec.abs() < MIN_FLING_VELOCITY {
            continue;
        }
        let target = fling_rest_position(offset, velocity_pt_per_sec);
        let error = (target - ios_target).abs();
        assert!(
            error < TARGET_OFFSET_TOLERANCE_PT,
            "offset={offset} v={velocity_pt_per_sec}pt/s: cranpose target={target}, iOS target={ios_target}, error={error}pt"
        );
    }
}

const RUBBER_BAND_DIMENSION_PT: f32 = 681.666_7;
const RUBBER_BAND_TRACE: &[(f32, f32)] = &[
    (15.00, 8.0000),
    (30.00, 16.0000),
    (45.00, 24.0000),
    (60.00, 31.3333),
    (75.00, 39.0000),
    (90.00, 46.0000),
    (105.00, 53.3333),
    (120.00, 60.3333),
    (135.00, 67.0000),
    (150.00, 73.6667),
    (165.00, 80.0000),
    (180.00, 86.3333),
    (195.00, 92.6667),
    (210.00, 98.6667),
    (225.00, 104.6667),
    (240.00, 110.6667),
    (255.00, 116.3333),
    (270.00, 122.0000),
    (285.00, 127.3333),
    (300.00, 133.0000),
    (315.00, 138.0000),
    (330.00, 143.3333),
    (345.00, 148.3333),
    (360.00, 153.3333),
    (375.00, 158.3333),
    (390.00, 163.0000),
    (405.00, 168.0000),
    (420.00, 172.6667),
    (435.00, 177.0000),
];

const RUBBER_BAND_TOLERANCE_PT: f32 = 0.5;

#[test]
fn overscroll_rubber_band_matches_recorded_ios_drag_trace() {
    let effect = OverscrollEffect::new();
    effect.set_dimension(RUBBER_BAND_DIMENSION_PT);

    let mut previous_x = 0.0f32;
    let mut max_abs_error = 0.0f32;
    for &(cumulative_x, recorded_overscroll) in RUBBER_BAND_TRACE {
        effect.apply_drag_delta(cumulative_x - previous_x);
        previous_x = cumulative_x;
        let error = (effect.offset() - recorded_overscroll).abs();
        max_abs_error = max_abs_error.max(error);
        assert!(
            error < RUBBER_BAND_TOLERANCE_PT,
            "raw pull={cumulative_x}: cranpose offset={got}, iOS recorded={recorded_overscroll}, error={error}pt",
            got = effect.offset(),
        );
    }
    assert!(max_abs_error > 0.05, "trace is not exercising resistance");
}

const BOUNCE_START_OFFSET: f32 = -177.0;
const BOUNCE_TRACE: &[(f32, f32)] = &[
    (16.235, -157.3333),
    (33.471, -138.6667),
    (49.541, -121.6667),
    (66.795, -106.3333),
    (83.471, -93.0000),
    (100.122, -81.0000),
    (116.477, -70.6667),
    (133.140, -61.3333),
    (150.131, -53.3333),
    (166.799, -46.0000),
    (183.446, -40.0000),
    (199.516, -34.3333),
    (216.769, -29.6667),
    (233.445, -25.6667),
    (249.503, -22.0000),
    (266.760, -19.0000),
    (283.421, -16.3333),
    (300.095, -14.0000),
    (316.170, -12.0000),
    (332.841, -10.3333),
    (349.720, -9.0000),
    (366.760, -7.6667),
    (383.434, -6.6667),
    (400.098, -5.6667),
    (416.246, -4.6667),
    (433.447, -4.0000),
    (449.933, -3.6667),
    (466.345, -3.0000),
    (482.853, -2.6667),
    (499.651, -2.3333),
    (516.798, -2.0000),
    (532.878, -1.6667),
    (549.539, -1.3333),
    (566.376, -1.0000),
    (616.799, -0.6667),
    (666.808, -0.3333),
];

const BOUNCE_TOLERANCE_PT: f32 = 5.0;

#[test]
fn overscroll_bounce_back_matches_recorded_ios_release_trace() {
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let handle = runtime.handle();
    let settle = SettleAnimation::new(handle.clone(), SpringParams::OVERSCROLL_BOUNCE);
    let position = Rc::new(Cell::new(BOUNCE_START_OFFSET));
    let position_for_scroll = Rc::clone(&position);

    settle.start_settle(
        BOUNCE_START_OFFSET,
        0.0,
        0.0,
        move |delta| {
            position_for_scroll.set(position_for_scroll.get() + delta);
            delta
        },
        |_| {},
    );
    handle.drain_frame_callbacks(0);

    let mut max_abs_error = 0.0f32;
    for &(t_ms, recorded_y) in BOUNCE_TRACE {
        handle.drain_frame_callbacks((t_ms as f64 * 1_000_000.0) as u64);
        let error = (position.get() - recorded_y).abs();
        max_abs_error = max_abs_error.max(error);
        assert!(
            error < BOUNCE_TOLERANCE_PT,
            "at t={t_ms}ms: cranpose={got}, iOS recorded={recorded_y}, error={error}pt exceeds {BOUNCE_TOLERANCE_PT}pt",
            got = position.get(),
        );
    }
    assert!(max_abs_error > 0.05, "trace is not exercising the spring");
}

#[test]
fn measured_decay_rates_match_apple_documented_constants() {
    assert_eq!(cranpose_animation::IOS_DECELERATION_RATE_NORMAL, 0.998);
    assert_eq!(cranpose_animation::IOS_DECELERATION_RATE_FAST, 0.99);
}
