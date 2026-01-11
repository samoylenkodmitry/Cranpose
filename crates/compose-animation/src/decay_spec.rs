//! Decay animation specification for fling animations.
//!
//! Port of Jetpack Compose's SplineBasedDecay and FlingCalculator.
//! This provides the physics for Android-feel fling scrolling.

use std::sync::LazyLock;

// ============================================================================
// Android Fling Spline
// ============================================================================

/// Tension curve inflection point
const INFLECTION: f32 = 0.35;
const START_TENSION: f32 = 0.5;
const END_TENSION: f32 = 1.0;
const P1: f32 = START_TENSION * INFLECTION;
const P2: f32 = 1.0 - END_TENSION * (1.0 - INFLECTION);

/// Number of samples in the spline lookup tables
const NB_SAMPLES: usize = 100;

/// Precomputed spline data for fast lookups.
#[allow(dead_code)]
struct SplineData {
    positions: [f32; NB_SAMPLES + 1],
    times: [f32; NB_SAMPLES + 1],
}

/// Lazily computed spline tables.
static SPLINE_DATA: LazyLock<SplineData> = LazyLock::new(|| {
    let mut positions = [0.0f32; NB_SAMPLES + 1];
    let mut times = [0.0f32; NB_SAMPLES + 1];

    let mut x_min = 0.0f32;
    let mut y_min = 0.0f32;

    for i in 0..NB_SAMPLES {
        let alpha = i as f32 / NB_SAMPLES as f32;

        // Find x such that bezier(x) = alpha
        let mut x_max = 1.0f32;
        let x;
        let coef;
        loop {
            let x_mid = x_min + (x_max - x_min) / 2.0;
            let c = 3.0 * x_mid * (1.0 - x_mid);
            let tx = c * ((1.0 - x_mid) * P1 + x_mid * P2) + x_mid * x_mid * x_mid;
            if (tx - alpha).abs() < 1e-5 {
                x = x_mid;
                coef = c;
                break;
            }
            if tx > alpha {
                x_max = x_mid;
            } else {
                x_min = x_mid;
            }
        }
        positions[i] = coef * ((1.0 - x) * START_TENSION + x) + x * x * x;

        // Find y for time lookup
        let mut y_max = 1.0f32;
        let y;
        let coef_y;
        loop {
            let y_mid = y_min + (y_max - y_min) / 2.0;
            let c = 3.0 * y_mid * (1.0 - y_mid);
            let dy = c * ((1.0 - y_mid) * START_TENSION + y_mid) + y_mid * y_mid * y_mid;
            if (dy - alpha).abs() < 1e-5 {
                y = y_mid;
                coef_y = c;
                break;
            }
            if dy > alpha {
                y_max = y_mid;
            } else {
                y_min = y_mid;
            }
        }
        times[i] = coef_y * ((1.0 - y) * P1 + y * P2) + y * y * y;
    }

    positions[NB_SAMPLES] = 1.0;
    times[NB_SAMPLES] = 1.0;

    SplineData { positions, times }
});

/// Result of sampling the fling spline.
#[derive(Debug, Clone, Copy)]
pub struct FlingResult {
    /// Distance coefficient (0.0 to 1.0) - fraction of total distance traveled.
    pub distance_coefficient: f32,
    /// Velocity coefficient - instantaneous velocity at this point.
    pub velocity_coefficient: f32,
}

/// Android fling spline implementation.
///
/// This is a port of Android's native fling scroll physics from `android.widget.Scroller`.
/// It provides smooth, natural-feeling fling deceleration.
pub struct AndroidFlingSpline;

impl AndroidFlingSpline {
    /// Sample the spline at a given time (0.0 to 1.0).
    ///
    /// Returns coefficients for distance and velocity at that point in the fling.
    pub fn fling_position(time: f32) -> FlingResult {
        let clamped_time = time.clamp(0.0, 1.0);
        let index = (NB_SAMPLES as f32 * clamped_time) as usize;

        let (distance_coef, velocity_coef) = if index < NB_SAMPLES {
            let t_inf = index as f32 / NB_SAMPLES as f32;
            let t_sup = (index + 1) as f32 / NB_SAMPLES as f32;
            let d_inf = SPLINE_DATA.positions[index];
            let d_sup = SPLINE_DATA.positions[index + 1];
            let vel = (d_sup - d_inf) / (t_sup - t_inf);
            let dist = d_inf + (clamped_time - t_inf) * vel;
            (dist, vel)
        } else {
            (1.0, 0.0)
        };

        FlingResult {
            distance_coefficient: distance_coef,
            velocity_coefficient: velocity_coef,
        }
    }

    /// Compute deceleration rate for a given velocity and friction.
    pub fn deceleration(velocity: f32, friction: f32) -> f64 {
        (INFLECTION as f64 * velocity.abs() as f64 / friction as f64).ln()
    }
}

// ============================================================================
// Fling Calculator
// ============================================================================

/// Earth's gravity in SI units (m/s²)
const GRAVITY_EARTH: f32 = 9.80665;
/// Inches per meter (for density conversion)
const INCHES_PER_METER: f32 = 39.37;
/// Deceleration rate constant (from Android Scroller)
const DECELERATION_RATE: f32 = 2.358_201_6; // (ln(0.78) / ln(0.9)).abs()

/// Computes physical deceleration based on density and friction.
fn compute_deceleration(friction: f32, density: f32) -> f32 {
    GRAVITY_EARTH * INCHES_PER_METER * density * 160.0 * friction
}

/// Information about a fling animation.
#[derive(Debug, Clone, Copy)]
pub struct FlingInfo {
    /// Initial velocity in px/sec.
    pub initial_velocity: f32,
    /// Total distance that will be traveled.
    pub distance: f32,
    /// Total duration in milliseconds.
    pub duration: i64,
}

impl FlingInfo {
    /// Get position at a given time (in milliseconds).
    pub fn position(&self, time_ms: i64) -> f32 {
        let spline_pos = if self.duration > 0 {
            time_ms as f32 / self.duration as f32
        } else {
            1.0
        };
        self.distance
            * self.initial_velocity.signum()
            * AndroidFlingSpline::fling_position(spline_pos).distance_coefficient
    }

    /// Get velocity at a given time (in milliseconds), in px/sec.
    pub fn velocity(&self, time_ms: i64) -> f32 {
        let spline_pos = if self.duration > 0 {
            time_ms as f32 / self.duration as f32
        } else {
            1.0
        };
        AndroidFlingSpline::fling_position(spline_pos).velocity_coefficient
            * self.initial_velocity.signum()
            * self.distance
            / self.duration as f32
            * 1000.0
    }

    /// Check if the fling is finished at the given time.
    pub fn is_finished(&self, time_ms: i64) -> bool {
        time_ms >= self.duration
    }
}

/// Calculator for Android-feel fling animations.
///
/// This uses the Android Scroller physics to compute natural fling behavior
/// based on physical constants and screen density.
#[derive(Debug, Clone, Copy)]
pub struct FlingCalculator {
    friction: f32,
    magic_physical_coefficient: f32,
}

impl FlingCalculator {
    /// Default friction value (matches Android default)
    pub const DEFAULT_FRICTION: f32 = 0.015;

    /// Create a new fling calculator.
    ///
    /// # Arguments
    /// * `friction` - Scroll friction coefficient (higher = faster deceleration)
    /// * `density` - Screen density in dp (e.g., 1.0 for mdpi, 2.0 for xhdpi)
    pub fn new(friction: f32, density: f32) -> Self {
        Self {
            friction,
            magic_physical_coefficient: compute_deceleration(0.84, density),
        }
    }

    /// Create a calculator with default friction for the given density.
    pub fn with_density(density: f32) -> Self {
        Self::new(Self::DEFAULT_FRICTION, density)
    }

    fn spline_deceleration(&self, velocity: f32) -> f64 {
        AndroidFlingSpline::deceleration(velocity, self.friction * self.magic_physical_coefficient)
    }

    /// Compute the duration of a fling in milliseconds.
    pub fn fling_duration(&self, velocity: f32) -> i64 {
        let l = self.spline_deceleration(velocity);
        let decel_minus_one = DECELERATION_RATE as f64 - 1.0;
        (1000.0 * (l / decel_minus_one).exp()) as i64
    }

    /// Compute the total distance a fling will travel.
    pub fn fling_distance(&self, velocity: f32) -> f32 {
        let l = self.spline_deceleration(velocity);
        let decel_minus_one = DECELERATION_RATE as f64 - 1.0;
        self.friction
            * self.magic_physical_coefficient
            * (DECELERATION_RATE as f64 / decel_minus_one * l).exp() as f32
    }

    /// Get complete fling information for a given initial velocity.
    pub fn fling_info(&self, velocity: f32) -> FlingInfo {
        FlingInfo {
            initial_velocity: velocity,
            distance: self.fling_distance(velocity),
            duration: self.fling_duration(velocity),
        }
    }
}

// ============================================================================
// Decay Animation Spec
// ============================================================================

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

/// Spline-based decay animation spec matching Android fling behavior.
#[derive(Debug, Clone, Copy)]
pub struct SplineBasedDecaySpec {
    calculator: FlingCalculator,
}

impl SplineBasedDecaySpec {
    /// Create a new spline-based decay spec for the given density.
    pub fn new(density: f32) -> Self {
        Self {
            calculator: FlingCalculator::with_density(density),
        }
    }

    /// Create a spec with a custom FlingCalculator.
    pub fn with_calculator(calculator: FlingCalculator) -> Self {
        Self { calculator }
    }
}

impl FloatDecayAnimationSpec for SplineBasedDecaySpec {
    fn abs_velocity_threshold(&self) -> f32 {
        0.0
    }

    fn get_value_from_nanos(
        &self,
        play_time_nanos: i64,
        initial_value: f32,
        initial_velocity: f32,
    ) -> f32 {
        let time_ms = play_time_nanos / 1_000_000;
        let info = self.calculator.fling_info(initial_velocity);
        initial_value + info.position(time_ms)
    }

    fn get_velocity_from_nanos(
        &self,
        play_time_nanos: i64,
        _initial_value: f32,
        initial_velocity: f32,
    ) -> f32 {
        let time_ms = play_time_nanos / 1_000_000;
        let info = self.calculator.fling_info(initial_velocity);
        info.velocity(time_ms)
    }

    fn get_duration_nanos(&self, _initial_value: f32, initial_velocity: f32) -> i64 {
        let duration_ms = self.calculator.fling_duration(initial_velocity);
        duration_ms * 1_000_000
    }

    fn get_target_value(&self, initial_value: f32, initial_velocity: f32) -> f32 {
        let distance = self.calculator.fling_distance(initial_velocity);
        initial_value + distance * initial_velocity.signum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spline_endpoints() {
        let start = AndroidFlingSpline::fling_position(0.0);
        assert!((start.distance_coefficient - 0.0).abs() < 0.01);

        let end = AndroidFlingSpline::fling_position(1.0);
        assert!((end.distance_coefficient - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_spline_monotonic() {
        let mut prev = 0.0;
        for i in 0..=100 {
            let t = i as f32 / 100.0;
            let result = AndroidFlingSpline::fling_position(t);
            assert!(
                result.distance_coefficient >= prev,
                "Spline should be monotonically increasing"
            );
            prev = result.distance_coefficient;
        }
    }

    #[test]
    fn test_fling_calculator() {
        let calc = FlingCalculator::with_density(2.0); // xhdpi

        // A typical fling velocity
        let velocity = 5000.0; // px/sec
        let duration = calc.fling_duration(velocity);
        let distance = calc.fling_distance(velocity);

        assert!(duration > 0, "Duration should be positive");
        assert!(distance > 0.0, "Distance should be positive");

        // Higher velocity should mean longer duration and distance
        let high_velocity = 10000.0;
        assert!(calc.fling_duration(high_velocity) > duration);
        assert!(calc.fling_distance(high_velocity) > distance);
    }

    #[test]
    fn test_decay_spec() {
        let spec = SplineBasedDecaySpec::new(2.0);

        let initial_value = 100.0;
        let velocity = 5000.0;

        // At t=0, position should be initial value
        let pos_0 = spec.get_value_from_nanos(0, initial_value, velocity);
        assert!((pos_0 - initial_value).abs() < 1.0);

        // At end, position should be at target
        let duration = spec.get_duration_nanos(initial_value, velocity);
        let target = spec.get_target_value(initial_value, velocity);
        let pos_end = spec.get_value_from_nanos(duration, initial_value, velocity);
        assert!(
            (pos_end - target).abs() < 10.0,
            "End position {} should be near target {}",
            pos_end,
            target
        );
    }

    #[test]
    fn test_negative_velocity() {
        let calc = FlingCalculator::with_density(2.0);

        let velocity = -5000.0;
        let info = calc.fling_info(velocity);

        // Position should move in negative direction
        let pos_mid = info.position(info.duration / 2);
        assert!(pos_mid < 0.0, "Should move in negative direction");
    }
}
