//! Decay animation specification for fling animations.
//!
//! Port of `UIScrollView`'s deceleration: a fling's velocity decays by a
//! constant fraction every millisecond (`v(t) = v0 * rate^t`), which
//! integrates to a closed-form position and rest offset. `rate` is exactly
//! `UIScrollView.DecelerationRate`, read on-device in
//! `cranpose-ui/src/tests/ios_fling_measurement.rs`; the exponential law
//! itself, the rubber-band resistance curve, and the overscroll bounce spring
//! were all fit against traces recorded from a real `UIScrollView` in the iOS
//! Simulator (see that test module and the PR that introduced it for the
//! recorded traces and residual error against iOS's own
//! `targetContentOffset` predictions).

/// `UIScrollView.DecelerationRate.normal.rawValue`, read at runtime on iOS
/// 26.5 (matches Apple's documented constant).
pub const IOS_DECELERATION_RATE_NORMAL: f32 = 0.998;

/// `UIScrollView.DecelerationRate.fast.rawValue`, read at runtime on iOS
/// 26.5 (matches Apple's documented constant).
pub const IOS_DECELERATION_RATE_FAST: f32 = 0.99;

/// Trait for decay animation specifications.
///
/// A decay animation has no fixed target - it starts with a velocity and
/// decelerates to zero. The final position depends on the initial velocity.
pub trait FloatDecayAnimationSpec {
    /// Velocity threshold below which animation is considered finished.
    fn abs_velocity_threshold(&self) -> f32;

    /// Get position at a given time.
    fn get_value_from_nanos(
        &self,
        play_time_nanos: i64,
        initial_value: f32,
        initial_velocity: f32,
    ) -> f32;

    /// Get velocity at a given time.
    fn get_velocity_from_nanos(
        &self,
        play_time_nanos: i64,
        initial_value: f32,
        initial_velocity: f32,
    ) -> f32;

    /// Get total animation duration in nanoseconds.
    fn get_duration_nanos(&self, initial_value: f32, initial_velocity: f32) -> i64;

    /// Get the target value (final position) of the animation.
    fn get_target_value(&self, initial_value: f32, initial_velocity: f32) -> f32;
}

const REST_VELOCITY_PTS_PER_SEC: f64 = 0.1;

/// Exponential decay animation spec matching `UIScrollView`'s deceleration.
///
/// `initial_velocity` and the values returned by `get_velocity_from_nanos`
/// are in points/sec (Cranpose's velocity-tracker convention throughout the
/// gesture pipeline); internally the law is evaluated in points/ms, which is
/// the unit `UIScrollView.DecelerationRate` and
/// `scrollViewWillEndDragging(_:withVelocity:targetContentOffset:)` use.
#[derive(Debug, Clone, Copy)]
pub struct ExponentialDecaySpec {
    rate: f32,
}

impl ExponentialDecaySpec {
    /// Creates a decay spec for the given per-millisecond decay `rate`
    /// (e.g. [`IOS_DECELERATION_RATE_NORMAL`]).
    pub fn new(rate: f32) -> Self {
        Self { rate }
    }

    fn ln_rate(&self) -> f64 {
        (self.rate as f64).ln()
    }
}

impl Default for ExponentialDecaySpec {
    fn default() -> Self {
        Self::new(IOS_DECELERATION_RATE_NORMAL)
    }
}

impl FloatDecayAnimationSpec for ExponentialDecaySpec {
    fn abs_velocity_threshold(&self) -> f32 {
        REST_VELOCITY_PTS_PER_SEC as f32
    }

    fn get_value_from_nanos(
        &self,
        play_time_nanos: i64,
        initial_value: f32,
        initial_velocity: f32,
    ) -> f32 {
        let t_ms = play_time_nanos as f64 / 1_000_000.0;
        let v0_per_ms = initial_velocity as f64 / 1000.0;
        let ln_rate = self.ln_rate();
        let delta = v0_per_ms * ((self.rate as f64).powf(t_ms) - 1.0) / ln_rate;
        initial_value + delta as f32
    }

    fn get_velocity_from_nanos(
        &self,
        play_time_nanos: i64,
        _initial_value: f32,
        initial_velocity: f32,
    ) -> f32 {
        let t_ms = play_time_nanos as f64 / 1_000_000.0;
        (initial_velocity as f64 * (self.rate as f64).powf(t_ms)) as f32
    }

    fn get_duration_nanos(&self, _initial_value: f32, initial_velocity: f32) -> i64 {
        let v0 = initial_velocity.abs() as f64;
        if v0 <= REST_VELOCITY_PTS_PER_SEC {
            return 0;
        }
        let t_ms = (REST_VELOCITY_PTS_PER_SEC / v0).ln() / self.ln_rate();
        (t_ms * 1_000_000.0) as i64
    }

    fn get_target_value(&self, initial_value: f32, initial_velocity: f32) -> f32 {
        let v0_per_ms = initial_velocity as f64 / 1000.0;
        initial_value - (v0_per_ms / self.ln_rate()) as f32
    }
}

#[cfg(test)]
mod tests {
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
}
