//! A one-euro filter on a controller's pointing direction.
//!
//! A resting hand shakes a ray by a few millimeters on a panel a meter away. The filter cuts
//! that shake while the hand rests. Its cutoff rises with the turning speed, so a fast turn
//! follows with little lag.

use std::{f32::consts::TAU, time::Duration};

use crate::Ray;

// The cutoff in hertz while the hand rests, and how far the cutoff rises for each radian per
// second of turn.
const STEADY_CUTOFF: f32 = 1.0;
const STEADY_BETA: f32 = 2.0;
// The cutoff in hertz of the filter's own estimate of the turning speed.
const SPEED_CUTOFF: f32 = 1.0;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AimFilter {
    // The filtered direction and the time of its frame
    last: Option<(Duration, [f32; 3])>,
    // The turning speed in radians per second, itself filtered
    speed: f32,
}

impl AimFilter {
    // The ray at `time` with its direction filtered. The origin stays as it is: a hand's
    // tremor moves a ray's end by its turn far more than by its shift.
    pub(crate) fn apply(&mut self, ray: Ray, time: Duration) -> Ray {
        let raw = ray.direction;
        let dt = self
            .last
            .map_or(0.0, |(last, _)| time.saturating_sub(last).as_secs_f32());
        let direction = match self.last {
            Some((_, last)) if dt > 0.0 => {
                let speed = angle_between(last, raw) / dt;
                self.speed += low_pass(dt, SPEED_CUTOFF) * (speed - self.speed);
                let cutoff = STEADY_CUTOFF + STEADY_BETA * self.speed;
                normalize(lerp(last, raw, low_pass(dt, cutoff))).unwrap_or(raw)
            }
            Some((_, last)) => last,
            None => raw,
        };
        self.last = Some((time, direction));
        Ray {
            origin: ray.origin,
            direction,
        }
    }
}

// The angle between two unit directions in radians.
pub(crate) fn angle_between(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    dot.clamp(-1.0, 1.0).acos()
}

// The share of the way to a new value that a low-pass filter at `cutoff` hertz goes in `dt`
// seconds.
fn low_pass(dt: f32, cutoff: f32) -> f32 {
    let tau = 1.0 / (TAU * cutoff);
    1.0 / (1.0 + tau / dt)
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

fn normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (length > 1e-6).then(|| v.map(|c| c / length))
}
