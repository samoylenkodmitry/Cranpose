use super::*;

#[test]
fn ios_deceleration_rates_match_apple_documented_constants() {
    assert_eq!(IOS_DECELERATION_RATE_NORMAL, 0.998);
    assert_eq!(IOS_DECELERATION_RATE_FAST, 0.99);
}

#[test]
fn value_at_zero_time_is_initial_value() {
    let spec = ExponentialDecaySpec::new(IOS_DECELERATION_RATE_NORMAL);
    assert_eq!(spec.get_value_from_nanos(0, 100.0, 900.0), 100.0);
}

#[test]
fn velocity_at_zero_time_is_initial_velocity() {
    let spec = ExponentialDecaySpec::new(IOS_DECELERATION_RATE_NORMAL);
    assert!((spec.get_velocity_from_nanos(0, 0.0, 900.0) - 900.0).abs() < 1e-3);
}

#[test]
fn value_converges_to_target_by_the_reported_duration() {
    let spec = ExponentialDecaySpec::new(IOS_DECELERATION_RATE_NORMAL);
    let initial_value = 100.0;
    let velocity = 5000.0;
    let duration = spec.get_duration_nanos(initial_value, velocity);
    let target = spec.get_target_value(initial_value, velocity);
    let pos_end = spec.get_value_from_nanos(duration, initial_value, velocity);
    assert!(
        (pos_end - target).abs() < 1.0,
        "end position {pos_end} should be near target {target}"
    );
}

#[test]
fn negative_velocity_moves_target_backward() {
    let spec = ExponentialDecaySpec::new(IOS_DECELERATION_RATE_NORMAL);
    let target = spec.get_target_value(0.0, -5000.0);
    assert!(target < 0.0, "target {target} must be negative");
}

#[test]
fn fast_rate_decays_faster_than_normal_rate_for_the_same_velocity() {
    let normal = ExponentialDecaySpec::new(IOS_DECELERATION_RATE_NORMAL);
    let fast = ExponentialDecaySpec::new(IOS_DECELERATION_RATE_FAST);
    let velocity = 3000.0;
    assert!(
        fast.get_target_value(0.0, velocity) < normal.get_target_value(0.0, velocity),
        "fast deceleration must travel a shorter distance than normal"
    );
    assert!(fast.get_duration_nanos(0.0, velocity) < normal.get_duration_nanos(0.0, velocity));
}

#[test]
fn target_matches_recorded_ios_target_content_offset_within_measured_tolerance() {
    let spec = ExponentialDecaySpec::new(IOS_DECELERATION_RATE_NORMAL);
    let target = spec.get_target_value(400.0, 480.8);
    assert!(
        (target - 635.33).abs() < 5.2,
        "target {target} outside the measured 5.2pt tolerance"
    );
}
