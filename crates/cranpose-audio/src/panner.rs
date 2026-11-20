//! Cranpose's own panner: where a point source sounds in each ear, from where it stands relative
//! to the listener's head.
//!
//! The model has four parts. Each is a plain formula, and none needs a measured head response.
//!
//! - The distance gain comes from the source's [`DistanceAttenuation`] and applies to both ears.
//! - The level difference between the ears grows with the sine of the angle between the source and
//!   the head's forward axis, as the sideways part of the direction to the source. The ear that
//!   is farther from the source is quieter by up to [`MAX_LEVEL_DIFFERENCE_DB`].
//! - The time difference is the Woodworth formula for a spherical head: the far ear hears the
//!   sound `head_radius / speed_of_sound * (angle + sin(angle))` later, 656 microseconds at most.
//! - The head shadow is the one-pole, one-zero filter of Brown and Duda (1998). Each ear's
//!   filter keeps the low frequencies and lifts or cuts the high ones, by how far the source is
//!   from that ear's axis. A source at the ear gains 6 dB above 2 kHz, and one behind the head
//!   loses 20 dB.
//!
//! The model has no elevation cue and no front and back cue beyond what the shadow of the head
//! gives: a source above the head and one below it sound alike, and so do a source in front and
//! one behind. A measured head response would tell them apart. This one runs on every platform
//! with a few multiplications per voice and ear.

use cranpose_services::{AudioListener, DistanceAttenuation};

// The head of an adult, as a sphere: its radius in meters and the speed of sound in air in meters
// per second.
const HEAD_RADIUS: f32 = 0.0875;
const SPEED_OF_SOUND: f32 = 343.0;

// How much quieter the far ear is than the near one for a source on the head's axis, in dB. Real
// heads differ by a few dB at low frequencies. The loudspeakers of a headset sit at the ears and
// need a level cue that a head shadow alone does not give, so this one is a little larger.
pub(crate) const MAX_LEVEL_DIFFERENCE_DB: f32 = 6.0;

// The lowest gain factor of the head shadow's high frequencies, at the angle from the ear's axis
// where a source is farthest from the ear, and that angle, in radians (150 degrees).
const SHADOW_MIN_ALPHA: f32 = 0.1;
const SHADOW_MIN_ANGLE: f32 = 150.0 * std::f32::consts::PI / 180.0;

// A source closer to the head than this has no direction, so it sounds in the middle. In meters.
const NO_DIRECTION: f32 = 1.0e-3;

// The time constant of the head shadow's filter, in seconds: half the time that sound takes to
// travel the head's radius.
pub(crate) const SHADOW_TIME_CONSTANT: f32 = HEAD_RADIUS / (2.0 * SPEED_OF_SOUND);

// How a source sounds in each ear. Index 0 is the left ear and 1 the right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placement {
    // The gain of each ear, the distance gain included
    pub(crate) gain: [f32; 2],
    // How long each ear hears the sound after the near ear, in seconds. One of the two is zero.
    pub(crate) delay: [f32; 2],
    // The high frequency factor of each ear's head shadow filter: above 1 lifts them, below 1 cuts
    // them
    pub(crate) shadow: [f32; 2],
}

impl Placement {
    // A source straight ahead of the head at the distance where the gain is 1.
    pub(crate) const AHEAD: Self = Self {
        gain: [1.0; 2],
        delay: [0.0; 2],
        shadow: [1.0; 2],
    };
}

// Where a source at `source`, in meters in the listener's frame of reference, sounds in each ear
// of `listener`.
pub(crate) fn place(
    listener: &AudioListener,
    source: [f32; 3],
    attenuation: DistanceAttenuation,
) -> Placement {
    let local = listener.local(source);
    let distance = (local[0] * local[0] + local[1] * local[1] + local[2] * local[2]).sqrt();
    // The sideways part of the direction: 1 straight at the right ear, -1 at the left
    let sideways = if distance > NO_DIRECTION {
        (local[0] / distance).clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let distance_gain = attenuation.gain(distance);
    let far = 10.0_f32.powf(-MAX_LEVEL_DIFFERENCE_DB * sideways.abs() / 20.0);
    let (left, right) = if sideways < 0.0 {
        (1.0, far)
    } else {
        (far, 1.0)
    };
    // Positive: the right ear hears the sound first
    let lead = HEAD_RADIUS / SPEED_OF_SOUND * (sideways.asin() + sideways);
    Placement {
        gain: [left * distance_gain, right * distance_gain],
        delay: [lead.max(0.0), (-lead).max(0.0)],
        shadow: [shadow_factor(-sideways), shadow_factor(sideways)],
    }
}

// The factor alpha of Brown and Duda's head shadow for an ear whose axis makes the angle with
// cosine `cosine` with the direction to the source.
fn shadow_factor(cosine: f32) -> f32 {
    let angle = cosine.clamp(-1.0, 1.0).acos();
    (1.0 + SHADOW_MIN_ALPHA / 2.0)
        + (1.0 - SHADOW_MIN_ALPHA / 2.0) * (angle / SHADOW_MIN_ANGLE * std::f32::consts::PI).cos()
}

// The coefficients of the head shadow's digital filter at `sample_rate`: b0, b1 and a1 of
// y[n] = b0 x[n] + b1 x[n-1] - a1 y[n-1]. The analog filter is (1 + alpha s T) / (1 + s T) for the
// time constant T, and the bilinear transform makes it digital. Its gain at zero frequency is 1.
pub(crate) fn shadow_coefficients(factor: f32, sample_rate: f32) -> [f32; 3] {
    let pole = 2.0 * SHADOW_TIME_CONSTANT * sample_rate;
    let scale = 1.0 / (1.0 + pole);
    [
        (1.0 + factor * pole) * scale,
        (1.0 - factor * pole) * scale,
        (1.0 - pole) * scale,
    ]
}

#[cfg(test)]
#[path = "tests/panner_tests.rs"]
mod tests;
