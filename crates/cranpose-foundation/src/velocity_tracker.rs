//! Velocity tracking for fling gesture support.
//!
//! Port of Jetpack Compose's `VelocityTracker1D` with its default strategy
//! for a pointer's positions: a least-squares fit of a curve of degree 2
//! through the last samples, whose slope at the newest sample is the
//! velocity.

const HISTORY_SIZE: usize = 20;

const HORIZON_MS: i64 = 100;

// The fewest samples a velocity is worked out from, as Compose's `Lsq2`.
const MIN_SAMPLES: usize = 3;

// How small, against the size of its terms, a determinant counts as zero.
const SINGULAR: f64 = 1.0e-9;

/// If no movement for this duration, assume the pointer has stopped.
pub const ASSUME_STOPPED_MS: i64 = 40;

#[derive(Clone, Copy, Default)]
struct DataPointAtTime {
    time_ms: i64,
    data_point: f32,
}

/// 1D velocity tracker over a pointer's positions.
///
/// This implements Jetpack Compose's `VelocityTracker1D` with its default
/// `Lsq2` strategy: the velocity is the slope, at the newest sample, of the
/// curve of degree 2 that fits the samples of the last 100 ms best. One
/// sample against the others' direction, as a finger makes when it lifts,
/// bends the curve a little and does not set the velocity by itself.
///
/// # Usage
/// ```ignore
/// let mut tracker = VelocityTracker1D::new();
/// tracker.add_data_point(time_ms, position);
/// // ... more points ...
/// let velocity = tracker.calculate_velocity(); // px/sec
/// ```
#[derive(Clone)]
pub struct VelocityTracker1D {
    samples: [Option<DataPointAtTime>; HISTORY_SIZE],
    index: usize,
}

impl Default for VelocityTracker1D {
    fn default() -> Self {
        Self::new()
    }
}

impl VelocityTracker1D {
    /// Creates a new velocity tracker for absolute position data.
    pub fn new() -> Self {
        Self {
            samples: [None; HISTORY_SIZE],
            index: 0,
        }
    }

    /// Adds the position `data_point` at the given time (milliseconds).
    pub fn add_data_point(&mut self, time_ms: i64, data_point: f32) {
        self.index = (self.index + 1) % HISTORY_SIZE;
        self.samples[self.index] = Some(DataPointAtTime {
            time_ms,
            data_point,
        });
    }

    /// Calculates the velocity in units/second.
    ///
    /// Returns 0.0 with fewer than three samples in the last 100 ms, or when
    /// the pointer rested for more than [`ASSUME_STOPPED_MS`] between two of
    /// them.
    pub fn calculate_velocity(&self) -> f32 {
        let mut data_points = [0.0f32; HISTORY_SIZE];
        let mut times = [0.0f32; HISTORY_SIZE];
        let mut sample_count = 0;

        let Some(newest_sample) = self.samples[self.index] else {
            return 0.0;
        };

        let mut current_index = self.index;
        let mut previous_sample = newest_sample;

        while let Some(sample) = self.samples[current_index] {
            let age = (newest_sample.time_ms - sample.time_ms) as f32;
            let delta = (sample.time_ms - previous_sample.time_ms).abs() as f32;
            previous_sample = sample;

            if age > HORIZON_MS as f32 || delta > ASSUME_STOPPED_MS as f32 {
                break;
            }

            data_points[sample_count] = sample.data_point;
            times[sample_count] = -age;

            current_index = if current_index == 0 {
                HISTORY_SIZE - 1
            } else {
                current_index - 1
            };

            sample_count += 1;
            if sample_count >= HISTORY_SIZE {
                break;
            }
        }

        if sample_count < MIN_SAMPLES {
            return 0.0;
        }

        fitted_velocity(&times[..sample_count], &data_points[..sample_count]) * 1000.0
    }

    /// Calculates the velocity in units/second, capped to `max_velocity`.
    pub fn calculate_velocity_with_max(&self, max_velocity: f32) -> f32 {
        if !max_velocity.is_finite() || max_velocity <= 0.0 {
            return 0.0;
        }

        let velocity = self.calculate_velocity();
        if velocity == 0.0 || velocity.is_nan() {
            return 0.0;
        }

        velocity.clamp(-max_velocity, max_velocity)
    }

    /// The velocity a gesture ends with when its pointer lifts at
    /// `lift_position` at `lift_time_ms`, capped to `max_velocity`: zero when
    /// the pointer rested for more than [`ASSUME_STOPPED_MS`] before it
    /// lifted.
    ///
    /// A lift at the last sample's position is no sample of the motion, as
    /// in Compose: it repeats that position at a later time, which would read
    /// as a pointer that slowed down. A lift somewhere else is the motion's
    /// last sample.
    pub fn release_velocity(
        &mut self,
        lift_time_ms: i64,
        lift_position: f32,
        max_velocity: f32,
    ) -> f32 {
        let Some(newest) = self.samples[self.index] else {
            return 0.0;
        };
        if lift_time_ms - newest.time_ms > ASSUME_STOPPED_MS {
            return 0.0;
        }
        if lift_position != newest.data_point {
            self.add_data_point(lift_time_ms, lift_position);
        }
        self.calculate_velocity_with_max(max_velocity)
    }

    /// Clears all tracked data.
    pub fn reset(&mut self) {
        self.samples = [None; HISTORY_SIZE];
        self.index = 0;
    }
}

// The slope at time zero, in units a millisecond, of the curve of degree 2
// that fits `positions` at `times` best: `times` are at or before zero, the
// newest sample's time. Samples at fewer than three different times bend no
// curve, so those take the best line's slope, and samples at one time have
// no slope.
fn fitted_velocity(times: &[f32], positions: &[f32]) -> f32 {
    // The sums of the normal equations, in f64: a time is up to 100 and its
    // fourth power loses digits in f32.
    let (mut s1, mut s2, mut s3, mut s4) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let (mut p0, mut p1, mut p2) = (0.0f64, 0.0f64, 0.0f64);
    for (&time, &position) in times.iter().zip(positions) {
        let (time, position) = (f64::from(time), f64::from(position));
        let square = time * time;
        s1 += time;
        s2 += square;
        s3 += square * time;
        s4 += square * square;
        p0 += position;
        p1 += position * time;
        p2 += position * square;
    }
    let s0 = times.len() as f64;
    // position = a + b * time + c * time^2; b by Cramer's rule.
    let determinant =
        s0 * (s2 * s4 - s3 * s3) - s1 * (s1 * s4 - s3 * s2) + s2 * (s1 * s3 - s2 * s2);
    let slope = if determinant.abs() > SINGULAR * s0 * s2 * s4 {
        (s0 * (p1 * s4 - s3 * p2) - p0 * (s1 * s4 - s3 * s2) + s2 * (s1 * p2 - p1 * s2))
            / determinant
    } else {
        let spread = s0 * s2 - s1 * s1;
        if spread.abs() <= SINGULAR * s0 * s2 {
            return 0.0;
        }
        (s0 * p1 - s1 * p0) / spread
    };
    // Only the lint budget's run turns this lint on, so an `expect` would
    // go unmet in every other build.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a velocity in units a millisecond is far inside f32"
    )]
    let slope = slope as f32;
    slope
}

#[cfg(test)]
#[path = "tests/velocity_tracker_tests.rs"]
mod tests;
