// One sounding note: a sum of sine partials, each with its own fade, under
// an envelope that opens at the strike and closes at the release.

use std::{
    f32::consts::{FRAC_PI_4, TAU},
    f64::consts::TAU as TAU_F64,
};

use super::timbre::{Recipe, PARTIALS};

// The level a voice counts as silent under: 80 dB below a full strike.
const SILENT: f32 = 1.0e-4;
// The level of one full strike. The soft limit of the synth bends the sum of
// several of them toward full scale.
const STRIKE_LEVEL: f32 = 0.4;
const A4_KEY: f32 = 69.0;
const A4_HERTZ: f32 = 440.0;
// A partial above this share of the sample rate would fold back as a false
// lower tone.
const HIGHEST_PARTIAL: f32 = 0.45;
// The keyboard spans the stereo picture this far to each side of its middle.
const PAN_WIDTH: f32 = 0.6;
const PAN_MIDDLE_KEY: f32 = 64.0;
const PAN_KEYS_PER_SIDE: f32 = 48.0;
// A fade lasts this many times longer two octaves down.
const FADE_OCTAVES: f32 = 24.0;

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Idle,
    Sounding,
    Released,
}

#[derive(Clone, Copy)]
pub(super) struct Voice {
    stage: Stage,
    // Each partial is a point on the unit circle, turned once a frame.
    sin: [f32; PARTIALS],
    cos: [f32; PARTIALS],
    turn_sin: [f32; PARTIALS],
    turn_cos: [f32; PARTIALS],
    level: [f32; PARTIALS],
    fade: [f32; PARTIALS],
    envelope: f32,
    attack_step: f32,
    release: f32,
    left: f32,
    right: f32,
}

// A strike of a key.
#[derive(Clone, Copy)]
pub(super) struct Strike {
    // The MIDI number of the key.
    pub key: f32,
    // How hard the key is struck, from 0 to 1.
    pub velocity: f32,
    // The share of its loudness the strike keeps.
    pub gain: f32,
    // Seconds since the strike: the voice starts this far into its life,
    // its partials faded and turned as far.
    pub age_seconds: f32,
}

fn narrowed(value: f64) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a sine or a cosine, within -1 and 1"
    )]
    let value = value as f32;
    value
}

// The share a level keeps each frame when it falls 60 dB in `seconds`.
fn fall_per_frame(seconds: f32, sample_rate: f32) -> f32 {
    (0.001f32.ln() / (seconds * sample_rate)).exp()
}

impl Voice {
    pub(super) const IDLE: Self = Self {
        stage: Stage::Idle,
        sin: [0.0; PARTIALS],
        cos: [1.0; PARTIALS],
        turn_sin: [0.0; PARTIALS],
        turn_cos: [1.0; PARTIALS],
        level: [0.0; PARTIALS],
        fade: [1.0; PARTIALS],
        envelope: 0.0,
        attack_step: 1.0,
        release: 0.0,
        left: 0.0,
        right: 0.0,
    };

    // Whether a note of `key` still sounds `age_seconds` after its strike.
    pub(super) fn sounds_after(recipe: &Recipe, key: f32, age_seconds: f32) -> bool {
        recipe.fade_seconds.is_none_or(|seconds| {
            let key_fade = 2f32.powf((A4_KEY - key) / FADE_OCTAVES);
            age_seconds < seconds * key_fade * SILENT.ln() / 0.001f32.ln()
        })
    }

    // Starts the voice as `strike` says, at the rate of the device.
    pub(super) fn strike(&mut self, recipe: &Recipe, strike: Strike, sample_rate: f32) {
        let Strike {
            key,
            velocity,
            gain,
            age_seconds,
        } = strike;
        let age_frames = age_seconds.max(0.0) * sample_rate;
        let frequency = A4_HERTZ * 2f32.powf((key - A4_KEY) / 12.0);
        let key_fade = 2f32.powf((A4_KEY - key) / FADE_OCTAVES);
        let mut power = 0.0;
        for index in 0..PARTIALS {
            let number = (index + 1) as f32;
            let above_first = index as f32 / (PARTIALS - 1) as f32;
            let partial = frequency
                * recipe.ratios[index]
                * (1.0 + recipe.stiffness * number * number).sqrt();
            let angle = TAU * partial / sample_rate;
            // The turns of the partial since the strike, as an angle
            // within one turn.
            let turned = (f64::from(angle) * f64::from(age_frames)).rem_euclid(TAU_F64);
            self.sin[index] = narrowed(turned.sin());
            self.cos[index] = narrowed(turned.cos());
            self.turn_sin[index] = angle.sin();
            self.turn_cos[index] = angle.cos();
            self.fade[index] = recipe.fade_seconds.map_or(1.0, |seconds| {
                let spread = 1.0 + recipe.fade_spread * index as f32;
                fall_per_frame(seconds * key_fade / spread, sample_rate)
            });
            self.level[index] = if partial < HIGHEST_PARTIAL * sample_rate {
                recipe.levels[index]
                    * velocity.powf(recipe.brightness * above_first)
                    * self.fade[index].powf(age_frames)
            } else {
                0.0
            };
            power += recipe.levels[index] * recipe.levels[index];
        }
        let loudness = velocity.powf(1.5) * gain * STRIKE_LEVEL / power.sqrt();
        let side = ((key - PAN_MIDDLE_KEY) / PAN_KEYS_PER_SIDE).clamp(-1.0, 1.0) * PAN_WIDTH;
        let angle = (side + 1.0) * FRAC_PI_4;
        self.left = angle.cos() * loudness;
        self.right = angle.sin() * loudness;
        self.envelope = 0.0;
        self.attack_step = 1.0 / (recipe.attack_seconds * sample_rate).max(1.0);
        self.release = fall_per_frame(recipe.release_seconds, sample_rate);
        self.stage = Stage::Sounding;
    }

    pub(super) fn release(&mut self) {
        if self.stage == Stage::Sounding {
            self.stage = Stage::Released;
        }
    }

    pub(super) fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    pub(super) fn is_held(&self) -> bool {
        self.stage == Stage::Sounding
    }

    // How loud the voice is now, for choosing the one to give up.
    pub(super) fn loudness(&self) -> f32 {
        let partials = self.level.iter().fold(0.0, |sum, level| sum + level);
        partials * self.envelope * (self.left + self.right)
    }

    // Adds the next frames of the voice to `out`, left and right.
    pub(super) fn render(&mut self, out: &mut [[f32; 2]]) {
        if self.stage == Stage::Idle {
            return;
        }
        for frame in out {
            let mut sum = 0.0;
            for index in 0..PARTIALS {
                let sin =
                    self.sin[index] * self.turn_cos[index] + self.cos[index] * self.turn_sin[index];
                let cos =
                    self.cos[index] * self.turn_cos[index] - self.sin[index] * self.turn_sin[index];
                self.sin[index] = sin;
                self.cos[index] = cos;
                self.level[index] *= self.fade[index];
                sum += sin * self.level[index];
            }
            self.envelope = match self.stage {
                Stage::Released => self.envelope * self.release,
                Stage::Sounding | Stage::Idle => (self.envelope + self.attack_step).min(1.0),
            };
            let sample = sum * self.envelope;
            frame[0] += sample * self.left;
            frame[1] += sample * self.right;
        }
        // Rounding moves each point off the unit circle a little every turn.
        for index in 0..PARTIALS {
            let length = self.sin[index] * self.sin[index] + self.cos[index] * self.cos[index];
            let correction = 1.5 - 0.5 * length;
            self.sin[index] *= correction;
            self.cos[index] *= correction;
        }
        let loudest = self
            .level
            .iter()
            .fold(0.0f32, |most, level| most.max(*level));
        let faded = loudest * self.envelope < SILENT;
        if faded && (self.stage == Stage::Released || self.envelope >= 1.0) {
            self.stage = Stage::Idle;
        }
    }
}
