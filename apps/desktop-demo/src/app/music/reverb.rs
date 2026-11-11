// The room around the instrument: four fed-back delays in parallel and two
// all-pass delays in a row for each ear (Schroeder's reverberator with the
// damped feedback of Freeverb).

use cranpose_ui_graphics::whole;

const COMBS: usize = 4;
const ALL_PASSES: usize = 2;
// Delay lengths in frames at `TUNED_RATE`, chosen with no common factors so
// the echoes do not pile up on one pitch.
const COMB_FRAMES: [usize; COMBS] = [1116, 1188, 1277, 1356];
const ALL_PASS_FRAMES: [usize; ALL_PASSES] = [556, 441];
// The right ear's delays are this much longer, which widens the room.
const SPREAD_FRAMES: usize = 23;
const TUNED_RATE: usize = 44_100;
const HIGHEST_RATE: usize = 192_000;
const ROOM: f32 = 0.84;
const DAMPING: f32 = 0.25;
const ALL_PASS_FEEDBACK: f32 = 0.5;
const SEND: f32 = 0.03;
const WET: f32 = 1.4;

struct Delay {
    frames: Vec<f32>,
    length: usize,
    at: usize,
    damped: f32,
}

impl Delay {
    // Holds room for the delay at the highest rate, so a change of rate
    // never allocates on the audio thread.
    fn new(tuned_frames: usize) -> Self {
        let most = (tuned_frames * HIGHEST_RATE).div_ceil(TUNED_RATE);
        Self {
            frames: vec![0.0; most],
            length: tuned_frames,
            at: 0,
            damped: 0.0,
        }
    }

    fn set_sample_rate(&mut self, tuned_frames: usize, sample_rate: usize) {
        let frames = (tuned_frames * sample_rate + TUNED_RATE / 2) / TUNED_RATE;
        self.length = frames.clamp(1, self.frames.len());
        self.at = 0;
        self.damped = 0.0;
        self.frames.fill(0.0);
    }

    fn comb(&mut self, input: f32) -> f32 {
        let output = self.frames[self.at];
        self.damped = output * (1.0 - DAMPING) + self.damped * DAMPING;
        self.frames[self.at] = input + self.damped * ROOM;
        self.step();
        output
    }

    fn all_pass(&mut self, input: f32) -> f32 {
        let delayed = self.frames[self.at];
        self.frames[self.at] = input + delayed * ALL_PASS_FEEDBACK;
        self.step();
        delayed - input
    }

    fn step(&mut self) {
        self.at += 1;
        if self.at == self.length {
            self.at = 0;
        }
    }
}

struct Ear {
    spread: usize,
    combs: [Delay; COMBS],
    all_passes: [Delay; ALL_PASSES],
}

impl Ear {
    fn new(spread: usize) -> Self {
        Self {
            spread,
            combs: COMB_FRAMES.map(|frames| Delay::new(frames + spread)),
            all_passes: ALL_PASS_FRAMES.map(|frames| Delay::new(frames + spread)),
        }
    }

    fn set_sample_rate(&mut self, sample_rate: usize) {
        for (delay, frames) in self.combs.iter_mut().zip(COMB_FRAMES) {
            delay.set_sample_rate(frames + self.spread, sample_rate);
        }
        for (delay, frames) in self.all_passes.iter_mut().zip(ALL_PASS_FRAMES) {
            delay.set_sample_rate(frames + self.spread, sample_rate);
        }
    }

    fn echo(&mut self, input: f32) -> f32 {
        let mut sum = 0.0;
        for comb in &mut self.combs {
            sum += comb.comb(input);
        }
        for all_pass in &mut self.all_passes {
            sum = all_pass.all_pass(sum);
        }
        sum
    }
}

pub(super) struct Reverb {
    ears: [Ear; 2],
}

impl Reverb {
    pub(super) fn new() -> Self {
        Self {
            ears: [Ear::new(0), Ear::new(SPREAD_FRAMES)],
        }
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        let hertz = usize::try_from(whole::to_i32(sample_rate)).unwrap_or(0);
        for ear in &mut self.ears {
            ear.set_sample_rate(hertz);
        }
    }

    // Adds the room's answer to each frame of `frames`, left and right.
    pub(super) fn render(&mut self, frames: &mut [[f32; 2]]) {
        for frame in frames {
            let input = (frame[0] + frame[1]) * SEND;
            frame[0] += self.ears[0].echo(input) * WET;
            frame[1] += self.ears[1].echo(input) * WET;
        }
    }
}
