//! Velocity tracking for fling gesture support.
//!
//! Port of Jetpack Compose's VelocityTracker1D using the Impulse strategy.
//! This calculates velocity based on kinetic energy principles.

const HISTORY_SIZE: usize = 20;

const HORIZON_MS: i64 = 100;

/// If no movement for this duration, assume the pointer has stopped.
pub const ASSUME_STOPPED_MS: i64 = 40;

#[derive(Clone, Copy, Default)]
struct DataPointAtTime {
    time_ms: i64,
    data_point: f32,
}

/// 1D velocity tracker using impulse-based velocity calculation.
///
/// This implements the same algorithm as Jetpack Compose's VelocityTracker1D
/// with the Impulse strategy, which calculates velocity based on the
/// kinetic energy imparted by the touch gestures.
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
    is_differential: bool,
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
            is_differential: false,
        }
    }

    /// Adds a data point at the given time (milliseconds).
    ///
    /// For absolute tracking, `data_point` is the position.
    /// For differential tracking, `data_point` is the change since last point.
    pub fn add_data_point(&mut self, time_ms: i64, data_point: f32) {
        self.index = (self.index + 1) % HISTORY_SIZE;
        self.samples[self.index] = Some(DataPointAtTime {
            time_ms,
            data_point,
        });
    }

    /// Calculates the velocity in units/second.
    ///
    /// Returns 0.0 if there aren't enough samples or if the pointer hasn't moved.
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
            previous_sample = if self.is_differential {
                sample
            } else {
                newest_sample
            };

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

        if sample_count < 2 {
            return 0.0;
        }

        let velocity_per_ms =
            calculate_impulse_velocity(&data_points, &times, sample_count, self.is_differential);

        velocity_per_ms * 1000.0
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

    /// Clears all tracked data.
    pub fn reset(&mut self) {
        self.samples = [None; HISTORY_SIZE];
        self.index = 0;
    }
}

fn calculate_impulse_velocity(
    data_points: &[f32; HISTORY_SIZE],
    times: &[f32; HISTORY_SIZE],
    sample_count: usize,
    is_differential: bool,
) -> f32 {
    if sample_count < 2 {
        return 0.0;
    }

    let mut work = 0.0f32;
    let start = sample_count - 1;
    let mut next_time = times[start];

    for i in (1..=start).rev() {
        let current_time = next_time;
        next_time = times[i - 1];
        if current_time == next_time {
            continue;
        }

        let data_points_delta = if is_differential {
            -data_points[i - 1]
        } else {
            data_points[i] - data_points[i - 1]
        };
        let v_curr = data_points_delta / (current_time - next_time);
        let v_prev = kinetic_energy_to_velocity(work);
        work += (v_curr - v_prev) * v_curr.abs();
        if i == start {
            work *= 0.5;
        }
    }

    kinetic_energy_to_velocity(work)
}

#[inline]
fn kinetic_energy_to_velocity(kinetic_energy: f32) -> f32 {
    kinetic_energy.signum() * (2.0 * kinetic_energy.abs()).sqrt()
}

#[cfg(test)]
#[path = "tests/velocity_tracker_tests.rs"]
mod tests;
