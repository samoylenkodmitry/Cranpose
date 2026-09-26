use std::{cell::Cell, rc::Rc, sync::Arc};

use cranpose_core::{DefaultScheduler, Runtime};

use super::*;

#[test]
fn test_min_velocity_threshold() {
    assert_eq!(MIN_FLING_VELOCITY, 300.0);
}

#[test]
fn settle_animation_springs_to_target_and_ends() {
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let handle = runtime.handle();
    let settle = SettleAnimation::new(handle.clone(), SpringParams::SETTLE_POLICY);
    let position = Rc::new(Cell::new(30.0f32));
    let ended = Rc::new(Cell::new(false));
    let position_for_scroll = Rc::clone(&position);
    let ended_for_end = Rc::clone(&ended);
    settle.start_settle(
        30.0,
        0.0,
        52.0,
        move |delta| {
            position_for_scroll.set(position_for_scroll.get() + delta);
            delta
        },
        move |_| ended_for_end.set(true),
    );
    for frame in 0..240u64 {
        handle.drain_frame_callbacks(frame * 16_000_000);
        if ended.get() {
            break;
        }
    }
    assert!(ended.get(), "settle animation must finish");
    assert!(
        (position.get() - 52.0).abs() < 0.2,
        "settle must land on the target, got {}",
        position.get()
    );
}

#[test]
fn settle_animation_ends_on_a_target_two_million_pixels_down() {
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let handle = runtime.handle();
    let settle = SettleAnimation::new(handle.clone(), SpringParams::SETTLE_POLICY);
    let travelled = Rc::new(Cell::new(0.0f64));
    let ended = Rc::new(Cell::new(false));
    let travelled_for_scroll = Rc::clone(&travelled);
    let ended_for_end = Rc::clone(&ended);
    settle.start_settle(
        1_999_000.0,
        0.0,
        2_000_000.0,
        move |delta| {
            travelled_for_scroll.set(travelled_for_scroll.get() + f64::from(delta));
            delta
        },
        move |_| ended_for_end.set(true),
    );
    for frame in 1..=240u64 {
        handle.drain_frame_callbacks(frame * 16_000_000);
        if ended.get() {
            break;
        }
    }
    assert!(ended.get(), "a settle far down a list must finish");
    assert!(
        (travelled.get() - 1_000.0).abs() < 0.2,
        "the settle must scroll the thousand pixels to its target, scrolled {}",
        travelled.get()
    );
}

#[test]
fn settle_reports_reduced_velocity_when_crossing_boundary() {
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let handle = runtime.handle();
    let settle = SettleAnimation::new(handle.clone(), SpringParams::SETTLE_POLICY);
    let position = Rc::new(Cell::new(30.0f32));
    let ended = Rc::new(Cell::new(None::<(f32, bool)>));
    let position_for_scroll = Rc::clone(&position);
    let ended_for_end = Rc::clone(&ended);
    settle.start_settle(
        30.0,
        -1_200.0,
        0.0,
        move |delta| {
            let previous = position_for_scroll.get();
            let next = (previous + delta).max(0.0);
            position_for_scroll.set(next);
            next - previous
        },
        move |end| ended_for_end.set(Some((end.velocity, end.hit_boundary))),
    );
    for frame in 0..240u64 {
        handle.drain_frame_callbacks(frame * 16_000_000);
        if ended.get().is_some() {
            break;
        }
    }

    let (velocity, hit_boundary) = ended.get().expect("settle must finish");
    assert!(hit_boundary);
    assert!(velocity < 0.0 && velocity.abs() < 1_200.0);
    assert_eq!(position.get(), 0.0);
}

#[test]
fn fling_rest_position_is_beyond_start_in_fling_direction() {
    let rest = fling_rest_position(100.0, 900.0);
    assert!(rest > 100.0, "rest {rest} must be past the start");
    assert_eq!(fling_rest_position(100.0, 0.0), 100.0);
}

#[test]
fn test_on_end_called_when_boundary_hit() {
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let handle = runtime.handle();
    let fling = FlingAnimation::new(handle.clone());
    let finished = Rc::new(Cell::new(false));
    let finished_flag = Rc::clone(&finished);

    fling.start_fling(0.0, 10_000.0, |_| 0.0, move || finished_flag.set(true));

    handle.drain_frame_callbacks(0);
    handle.drain_frame_callbacks(16_000_000);

    assert!(finished.get());
}
