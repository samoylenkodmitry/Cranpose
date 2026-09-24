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

    let velocity = tracker.calculate_velocity_with_max(8_000.0);
    assert_eq!(velocity, 8_000.0);

    tracker.reset();
    tracker.add_data_point(0, 10_000.0);
    tracker.add_data_point(1, 0.0);

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
