//! Velocity tracking for fling gesture support.
//!
//! The velocity a touch ends with is the one an iOS scroll view gives the
//! same touch samples: a weighted mean of the last three velocities from one
//! sample to the next, the oldest of them weighing most. The weights are the
//! ones Flutter's `IOSScrollViewFlingVelocityTracker` fits to
//! `scrollViewWillEndDragging(_:withVelocity:targetContentOffset:)`.

const HISTORY_SIZE: usize = 20;

// The weights of the velocities between the last four samples, the newest
// pair first. The last move before a lift is often a twitch of the finger,
// so it weighs least.
const PAIR_WEIGHTS: [f32; 3] = [0.05, 0.35, 0.6];

/// If no movement for this duration, assume the pointer has stopped.
pub const ASSUME_STOPPED_MS: i64 = 40;

#[derive(Clone, Copy, Default)]
struct DataPointAtTime {
    time_ms: i64,
    data_point: f32,
}

/// 1D velocity tracker over a pointer's positions.
///
/// The velocity is a weighted mean of the last three velocities from one
/// sample to the next: 0.6 for the oldest, 0.35 for the middle one and 0.05
/// for the newest, as an iOS scroll view weighs them. One move against the
/// others' direction, as a finger makes when it lifts, changes the result by
/// a twentieth of its own speed.
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
    /// A pair of samples that is missing, or at one time, counts as no
    /// motion. So does a pair more than [`ASSUME_STOPPED_MS`] apart, and every
    /// pair before it: the pointer rested there, and what it did before the
    /// rest is no part of this motion.
    pub fn calculate_velocity(&self) -> f32 {
        let mut velocity = 0.0;
        let mut newer_index = self.index;
        for weight in PAIR_WEIGHTS {
            let older_index = (newer_index + HISTORY_SIZE - 1) % HISTORY_SIZE;
            let (Some(newer), Some(older)) = (self.samples[newer_index], self.samples[older_index])
            else {
                break;
            };
            let elapsed_ms = newer.time_ms - older.time_ms;
            if !(0..=ASSUME_STOPPED_MS).contains(&elapsed_ms) {
                break;
            }
            if elapsed_ms > 0 {
                let per_ms = (newer.data_point - older.data_point) / elapsed_ms as f32;
                velocity += weight * per_ms * 1000.0;
            }
            newer_index = older_index;
        }
        velocity
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
    /// A lift at the last sample's position is no sample of the motion: it
    /// repeats that position at a later time, which would read as a pointer
    /// that slowed down. A lift somewhere else is the motion's last sample.
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

#[cfg(test)]
#[path = "tests/velocity_tracker_tests.rs"]
mod tests;
