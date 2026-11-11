use super::*;

#[test]
fn test_empty_tracker_returns_zero() {
    let tracker = VelocityTracker1D::new();
    assert_eq!(tracker.calculate_velocity(), 0.0);
}

#[test]
fn test_single_point_returns_zero() {
    let mut tracker = VelocityTracker1D::new();
    tracker.add_data_point(0, 100.0);
    assert_eq!(tracker.calculate_velocity(), 0.0);
}

#[test]
fn test_constant_velocity() {
    let mut tracker = VelocityTracker1D::new();
    tracker.add_data_point(0, 0.0);
    tracker.add_data_point(10, 100.0);
    tracker.add_data_point(20, 200.0);
    tracker.add_data_point(30, 300.0);

    let velocity = tracker.calculate_velocity();
    assert!(
        (velocity - 10000.0).abs() < 1000.0,
        "Expected ~10000, got {velocity}"
    );
}

#[test]
fn test_reset() {
    let mut tracker = VelocityTracker1D::new();
    tracker.add_data_point(0, 0.0);
    tracker.add_data_point(10, 100.0);

    tracker.reset();

    assert_eq!(tracker.calculate_velocity(), 0.0);
}

#[test]
fn test_negative_velocity() {
    let mut tracker = VelocityTracker1D::new();
    tracker.add_data_point(0, 300.0);
    tracker.add_data_point(10, 200.0);
    tracker.add_data_point(20, 100.0);

    let velocity = tracker.calculate_velocity();
    assert!(velocity < 0.0, "Expected negative velocity, got {velocity}");
}

#[test]
fn test_velocity_capped() {
    let mut tracker = VelocityTracker1D::new();
    tracker.add_data_point(0, 0.0);
    tracker.add_data_point(1, 10_000.0);
    tracker.add_data_point(2, 20_000.0);

    let velocity = tracker.calculate_velocity_with_max(8_000.0);
    assert_eq!(velocity, 8_000.0);

    tracker.reset();
    tracker.add_data_point(0, 20_000.0);
    tracker.add_data_point(1, 10_000.0);
    tracker.add_data_point(2, 0.0);

    let velocity = tracker.calculate_velocity_with_max(8_000.0);
    assert_eq!(velocity, -8_000.0);
}

#[test]
fn test_old_samples_ignored() {
    let mut tracker = VelocityTracker1D::new();
    tracker.add_data_point(0, 0.0);
    tracker.add_data_point(150, 100.0);
    tracker.add_data_point(160, 200.0);
    tracker.add_data_point(170, 300.0);

    let velocity = tracker.calculate_velocity();
    assert!(
        velocity.abs() > 0.0,
        "Should calculate velocity from recent samples"
    );
}

#[test]
fn test_gap_over_stopped_threshold_returns_zero() {
    let mut tracker = VelocityTracker1D::new();
    tracker.add_data_point(0, 0.0);
    tracker.add_data_point(ASSUME_STOPPED_MS + 1, 100.0);

    let velocity = tracker.calculate_velocity();
    assert_eq!(velocity, 0.0);
}

// A track of positions 8 ms apart, the last one first.
fn velocity_of(positions: &[f32]) -> f32 {
    let mut tracker = VelocityTracker1D::new();
    for (index, position) in positions.iter().enumerate() {
        tracker.add_data_point(index as i64 * 8, *position);
    }
    tracker.calculate_velocity()
}

// A finger that lifts often moves a little back first. That move alone must
// not send the fling back faster than the finger ever moved.
#[test]
fn a_small_move_back_at_the_end_does_not_fling_back_fast() {
    // 1,000 units a second, then 8 units back in the last sample.
    let velocity = velocity_of(&[0.0, 8.0, 16.0, 24.0, 32.0, 24.0]);
    assert!(
        velocity.abs() < 400.0,
        "an 8 unit move back after 1,000 units a second gave {velocity}"
    );
    // 250 units a second, then 6 units back: the finger never passed 750.
    let velocity = velocity_of(&[0.0, 2.0, 4.0, 6.0, 8.0, 2.0]);
    assert!(
        velocity.abs() < 750.0,
        "a 6 unit move back after 250 units a second gave {velocity}"
    );
}

#[test]
fn a_pointer_that_stops_in_its_last_sample_slows_down() {
    let moving = velocity_of(&[0.0, 8.0, 16.0, 24.0, 32.0, 40.0]);
    let stopped = velocity_of(&[0.0, 8.0, 16.0, 24.0, 32.0, 32.0]);
    assert!((moving - 1_000.0).abs() < 1.0, "{moving}");
    assert!(stopped > 0.0 && stopped < 600.0, "{stopped}");
}

#[test]
fn samples_at_two_times_take_a_line_s_slope() {
    let mut tracker = VelocityTracker1D::new();
    tracker.add_data_point(0, 0.0);
    tracker.add_data_point(8, 8.0);
    tracker.add_data_point(8, 8.0);
    let velocity = tracker.calculate_velocity();
    assert!((velocity - 1_000.0).abs() < 1.0, "{velocity}");
}
