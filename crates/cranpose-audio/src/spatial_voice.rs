//! The mix of a voice that plays from an emitter: each ear reads the clip at its own time, passes
//! the head shadow's filter and takes its own gain.
//!
//! A clip is in memory, so an ear's delay is a second read position earlier in the clip. The voice
//! keeps no delay line. The gain, the delay and the filter's coefficients follow their targets in
//! a straight line across each render call, so a head that turns between two calls does not click.

use crate::{
    mixer::{ClipData, Reading},
    panner::{Placement, shadow_coefficients},
};

// One ear: what the voice's sound passes on its way to that side of the output.
#[derive(Clone, Copy)]
struct Ear {
    gain: f32,
    // In seconds
    delay: f32,
    // b0, b1 and a1 of the head shadow's filter
    coefficients: [f32; 3],
    // The filter's state
    state: f32,
}

// Where an ear is headed in a render call.
#[derive(Clone, Copy)]
struct EarTarget {
    gain: f32,
    delay: f32,
    coefficients: [f32; 3],
}

impl Ear {
    const IDLE: Self = Self {
        gain: 0.0,
        delay: 0.0,
        coefficients: [1.0, 0.0, 0.0],
        state: 0.0,
    };

    fn set(&mut self, target: &EarTarget) {
        self.gain = target.gain;
        self.delay = target.delay;
        self.coefficients = target.coefficients;
    }

    // The change in each frame that takes the ear to `target` in `frames` frames.
    fn steps(&self, target: &EarTarget, frames: usize) -> EarTarget {
        let per_frame = 1.0 / frames as f32;
        EarTarget {
            gain: (target.gain - self.gain) * per_frame,
            delay: (target.delay - self.delay) * per_frame,
            coefficients: std::array::from_fn(|index| {
                (target.coefficients[index] - self.coefficients[index]) * per_frame
            }),
        }
    }

    // What the ear hears of `heard`, the clip at its delay.
    fn hear(&mut self, heard: f32) -> f32 {
        let [b0, b1, a1] = self.coefficients;
        let filtered = b0 * heard + self.state;
        self.state = b1 * heard - a1 * filtered;
        filtered * self.gain
    }

    fn advance(&mut self, steps: &EarTarget) {
        self.gain += steps.gain;
        self.delay += steps.delay;
        for (coefficient, step) in self.coefficients.iter_mut().zip(steps.coefficients) {
            *coefficient += step;
        }
    }
}

// What the voice keeps from one render call to the next.
#[derive(Clone, Copy)]
pub(crate) struct SpatialVoice {
    ears: [Ear; 2],
    // The placement that the last good read of the emitter gave, which a read that overlapped a
    // write leaves in place
    pub(crate) placement: Placement,
    // Whether the voice has rendered. Its first call starts at its placement.
    started: bool,
}

// What a render call needs to know of the voice's clip and of the device.
pub(crate) struct Pass<'a> {
    pub(crate) clip: &'a ClipData,
    // The clip frames that the position advances in one output frame
    pub(crate) step: f64,
    pub(crate) looping: bool,
    pub(crate) device_rate: f32,
    pub(crate) channels: usize,
}

impl SpatialVoice {
    pub(crate) const IDLE: Self = Self {
        ears: [Ear::IDLE; 2],
        placement: Placement::AHEAD,
        started: false,
    };

    // Whether both ears have faded to nothing.
    pub(crate) fn is_silent(&self) -> bool {
        self.ears.iter().all(|ear| ear.gain == 0.0)
    }

    // Adds the voice's sound for the frames of `out` and moves `position` along the clip. `scale`
    // multiplies both gains: the voice's volume, its bus and the pause. Returns `true` when a clip
    // that does not loop ended in these frames.
    pub(crate) fn mix(
        &mut self,
        pass: &Pass<'_>,
        position: &mut f64,
        scale: f32,
        out: &mut [f32],
    ) -> bool {
        let frames = out.len() / pass.channels;
        let targets = [0, 1].map(|ear| EarTarget {
            gain: self.placement.gain[ear] * scale,
            delay: self.placement.delay[ear],
            coefficients: shadow_coefficients(self.placement.shadow[ear], pass.device_rate),
        });
        if !self.started {
            for (ear, target) in self.ears.iter_mut().zip(&targets) {
                ear.set(target);
            }
            self.started = true;
        }
        let steps = [0, 1].map(|ear| self.ears[ear].steps(&targets[ear], frames));
        // The clip frames that one second of an ear's delay is worth
        let seconds_to_frames = f64::from(pass.device_rate) * pass.step;
        let length = pass.clip.frames() as f64;
        let mut ended = false;
        for frame in 0..frames {
            if *position >= length {
                if !pass.looping {
                    ended = true;
                    break;
                }
                *position -= length;
                if *position < 0.0 || *position >= length {
                    *position = 0.0;
                }
            }
            let [left, right] = [0, 1].map(|index| {
                let ear = &mut self.ears[index];
                let at = *position - f64::from(ear.delay) * seconds_to_frames;
                let sound = ear.hear(read_mono(pass.clip, at, pass.looping));
                ear.advance(&steps[index]);
                sound
            });
            let base = frame * pass.channels;
            if pass.channels == 1 {
                out[base] += (left + right) * 0.5;
            } else {
                out[base] += left;
                out[base + 1] += right;
            }
            *position += pass.step;
        }
        // The straight lines reach the targets only up to rounding
        for (ear, target) in self.ears.iter_mut().zip(&targets) {
            ear.set(target);
        }
        ended
    }
}

// The clip at `position` frames, both channels of a stereo clip averaged, with linear interpolation.
// A position before the start is silence, or the end of the clip again for a clip that loops.
fn read_mono(clip: &ClipData, position: f64, looping: bool) -> f32 {
    let frames = clip.frames();
    let position = if position >= 0.0 {
        position
    } else if looping {
        position.rem_euclid(frames as f64)
    } else {
        return 0.0;
    };
    let reading = if looping {
        Reading::looping(position, frames)
    } else {
        Reading::once(position, frames)
    };
    let (a, b) = (
        frame_mono(clip, reading.index),
        frame_mono(clip, reading.next),
    );
    a + (b - a) * reading.fraction
}

fn frame_mono(clip: &ClipData, frame: usize) -> f32 {
    if clip.channels == 2 {
        (clip.samples[frame * 2] + clip.samples[frame * 2 + 1]) * 0.5
    } else {
        clip.samples[frame]
    }
}

#[cfg(test)]
#[path = "tests/spatial_voice_tests.rs"]
mod tests;
