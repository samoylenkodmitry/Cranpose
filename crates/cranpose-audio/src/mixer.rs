#![cfg_attr(
    not(any(
        test,
        all(feature = "aaudio", target_os = "android"),
        all(
            feature = "cpal-backend",
            not(any(target_os = "android", target_arch = "wasm32"))
        )
    )),
    allow(dead_code)
)]

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use crate::ring::{Consumer, Producer};

/// How many clips the engine holds at once. One byte of index, and far more
/// than the couple of dozen cues a game keeps resident.
pub const MAX_CLIPS: usize = 256;

/// How many voices can sound simultaneously. Beyond this the oldest one-shot
/// is stolen, which is what a listener expects when a cue storm arrives.
pub const MAX_VOICES: usize = 32;

pub const BUS_COUNT: usize = 2;

/// How long the output keeps running with nothing to play before the mixer
/// stops it.
///
/// A running output stream is not free even when every sample it carries is
/// zero: on Android it holds an MMAP route open and keeps the always-on audio
/// DSP awake, which measures in tens of milliwatts on a phone and is a large
/// share of a watch's budget. Stopping is therefore worth doing — but every
/// restart is a device round trip (route setup, then the first callback), so
/// stopping too eagerly turns a burst of UI cues into a burst of route changes
/// and risks clipping the front of a sound.
///
/// Two seconds sits above both of the intervals that matter. A player working
/// through a menu taps every few hundred milliseconds and a one-shot cue lasts
/// well under a second, so an active screen never stops the stream; a screen
/// the player has settled on goes quiet two seconds after its last sound and
/// stays that way for as long as they look at it, which is where all of the
/// battery is. Anything shorter buys nothing measurable and starts to thrash.
pub const IDLE_GRACE_SECONDS: f32 = 2.0;

/// What the output device should do once a
/// [`render`](crate::backend::Renderer::render) call returns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderStatus {
    /// Keep the stream running: something is sounding, or work is queued.
    Continue,
    /// Nothing has sounded for [`IDLE_GRACE_SECONDS`] and the command queue is
    /// empty, so the stream should stop. The engine starts it again on the
    /// next play.
    Idle,
}

#[derive(Clone)]
pub struct ClipData {
    pub samples: Arc<[f32]>,
    pub channels: u8,
    pub sample_rate: u32,
}

impl ClipData {
    fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels)
    }
}

impl std::fmt::Debug for ClipData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipData")
            .field("frames", &self.frames())
            .field("channels", &self.channels)
            .field("sample_rate", &self.sample_rate)
            .finish()
    }
}

#[derive(Debug)]
pub enum Command {
    LoadClip {
        slot: u32,
        clip: ClipData,
    },
    UnloadClip {
        slot: u32,
    },
    Play {
        voice: u64,
        slot: u32,
        gain_left: f32,
        gain_right: f32,
        rate: f32,
        bus: u8,
        looping: bool,
    },
    RetuneVoice {
        voice: u64,
        gain_left: f32,
        gain_right: f32,
        rate: f32,
    },
    StopVoice {
        voice: u64,
    },
    StopClip {
        slot: u32,
    },
    StopAll,
    SetMaster(f32),
    SetBusVolume {
        bus: u8,
        volume: f32,
    },
    SetBusEnabled {
        bus: u8,
        enabled: bool,
    },
}

pub struct MixerSeed {
    pub commands: Consumer<Command>,
    pub retired: Producer<ClipData>,
    pub leaked_clips: Arc<AtomicU32>,
    pub underruns: Arc<AtomicU32>,
    pub streaming: Arc<AtomicBool>,
}

#[derive(Clone, Copy)]
struct Voice {
    id: u64,
    slot: usize,
    position: f64,
    step: f64,
    rate: f32,
    gain_left: f32,
    gain_right: f32,
    bus: usize,
    looping: bool,
}

impl Voice {
    const IDLE: Voice = Voice {
        id: 0,
        slot: 0,
        position: 0.0,
        step: 1.0,
        rate: 1.0,
        gain_left: 0.0,
        gain_right: 0.0,
        bus: 0,
        looping: false,
    };
}

pub struct Mixer {
    commands: Consumer<Command>,
    retired: Producer<ClipData>,
    leaked_clips: Arc<AtomicU32>,
    underruns: Arc<AtomicU32>,
    streaming: Arc<AtomicBool>,
    clips: Vec<Option<ClipData>>,
    voices: Vec<Voice>,
    master: f32,
    bus_volume: [f32; BUS_COUNT],
    bus_enabled: [bool; BUS_COUNT],
    device_sample_rate: f32,
    device_channels: usize,
    idle_frames: u64,
    idle_grace_frames: u64,
}

impl Mixer {
    pub fn new(seed: MixerSeed, sample_rate: f32, channels: usize) -> Mixer {
        let mut clips = Vec::with_capacity(MAX_CLIPS);
        clips.resize_with(MAX_CLIPS, || None);
        Mixer {
            commands: seed.commands,
            retired: seed.retired,
            leaked_clips: seed.leaked_clips,
            underruns: seed.underruns,
            streaming: seed.streaming,
            clips,
            voices: vec![Voice::IDLE; MAX_VOICES],
            master: 1.0,
            bus_volume: [1.0; BUS_COUNT],
            bus_enabled: [true; BUS_COUNT],
            device_sample_rate: sample_rate.max(1.0),
            device_channels: channels.max(1),
            idle_frames: 0,
            idle_grace_frames: grace_frames(sample_rate),
        }
    }

    pub fn set_device_format(&mut self, sample_rate: f32, channels: usize) {
        let sample_rate = sample_rate.max(1.0);
        let channels = channels.max(1);
        if sample_rate == self.device_sample_rate && channels == self.device_channels {
            return;
        }
        self.device_sample_rate = sample_rate;
        self.device_channels = channels;
        self.idle_grace_frames = grace_frames(sample_rate);
        for index in 0..self.voices.len() {
            if self.voices[index].id == 0 {
                continue;
            }
            let slot = self.voices[index].slot;
            let rate = self.voices[index].rate;
            let clip_rate = self.clips[slot].as_ref().map_or(0, |clip| clip.sample_rate);
            self.voices[index].step = step_for(rate, clip_rate, sample_rate);
        }
    }

    #[allow(dead_code)]
    pub fn device_sample_rate(&self) -> f32 {
        self.device_sample_rate
    }

    #[allow(dead_code)]
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|voice| voice.id != 0).count()
    }

    pub fn render(&mut self, out: &mut [f32]) -> RenderStatus {
        self.drain_commands();

        for sample in out.iter_mut() {
            *sample = 0.0;
        }

        let channels = self.device_channels;
        if channels == 0 || out.is_empty() {
            return RenderStatus::Continue;
        }
        let out_frames = out.len() / channels;
        if out_frames == 0 {
            self.underruns.fetch_add(1, Ordering::Relaxed);
            return RenderStatus::Continue;
        }

        let master = self.master;
        let bus_gain = [
            if self.bus_enabled[0] {
                self.bus_volume[0] * master
            } else {
                0.0
            },
            if self.bus_enabled[1] {
                self.bus_volume[1] * master
            } else {
                0.0
            },
        ];

        let mut sounding = 0usize;
        let clips = &self.clips;
        for voice in &mut self.voices {
            if voice.id == 0 {
                continue;
            }
            let Some(clip) = clips[voice.slot].as_ref() else {
                voice.id = 0;
                continue;
            };
            let frames = clip.frames();
            if frames == 0 {
                voice.id = 0;
                continue;
            }
            let stereo_clip = clip.channels == 2;
            let gain = bus_gain[voice.bus];
            let gain_left = voice.gain_left * gain;
            let gain_right = voice.gain_right * gain;
            let mut position = voice.position;
            let step = voice.step;
            let length = frames as f64;
            let audible = voice.looping || position < length;

            for frame in 0..out_frames {
                if position >= length {
                    if voice.looping {
                        position -= length;
                        if position < 0.0 || position >= length {
                            position = 0.0;
                        }
                    } else {
                        voice.id = 0;
                        break;
                    }
                }

                let index = position as usize;
                let index = if index < frames { index } else { frames - 1 };
                let fraction = (position - index as f64) as f32;
                let next = if index + 1 < frames {
                    index + 1
                } else if voice.looping {
                    0
                } else {
                    index
                };

                let (left, right) = if stereo_clip {
                    let a_left = clip.samples[index * 2];
                    let a_right = clip.samples[index * 2 + 1];
                    let b_left = clip.samples[next * 2];
                    let b_right = clip.samples[next * 2 + 1];
                    (
                        a_left + (b_left - a_left) * fraction,
                        a_right + (b_right - a_right) * fraction,
                    )
                } else {
                    let a = clip.samples[index];
                    let b = clip.samples[next];
                    let sample = a + (b - a) * fraction;
                    (sample, sample)
                };

                let base = frame * channels;
                if channels == 1 {
                    out[base] += (left * gain_left + right * gain_right) * 0.5;
                } else {
                    out[base] += left * gain_left;
                    out[base + 1] += right * gain_right;
                }

                position += step;
            }

            voice.position = position;
            if audible {
                sounding += 1;
            }
        }

        for sample in out.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }

        self.settle(sounding, out_frames)
    }

    fn settle(&mut self, sounding: usize, frames: usize) -> RenderStatus {
        if sounding > 0 {
            self.idle_frames = 0;
            return RenderStatus::Continue;
        }
        self.idle_frames = self.idle_frames.saturating_add(frames as u64);
        if self.idle_frames < self.idle_grace_frames {
            return RenderStatus::Continue;
        }

        self.streaming.store(false, Ordering::SeqCst);
        std::sync::atomic::fence(Ordering::SeqCst);
        if !self.commands.is_empty() {
            self.streaming.store(true, Ordering::SeqCst);
            self.idle_frames = 0;
            return RenderStatus::Continue;
        }
        RenderStatus::Idle
    }

    fn drain_commands(&mut self) {
        while let Some(command) = self.commands.pop() {
            self.apply(command);
        }
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::LoadClip { slot, clip } => {
                let slot = slot as usize;
                if slot >= self.clips.len() {
                    self.retire(clip);
                    return;
                }
                self.silence_slot(slot);
                if let Some(previous) = self.clips[slot].replace(clip) {
                    self.retire(previous);
                }
            }
            Command::UnloadClip { slot } => {
                let slot = slot as usize;
                if slot >= self.clips.len() {
                    return;
                }
                self.silence_slot(slot);
                if let Some(previous) = self.clips[slot].take() {
                    self.retire(previous);
                }
            }
            Command::Play {
                voice,
                slot,
                gain_left,
                gain_right,
                rate,
                bus,
                looping,
            } => {
                let slot = slot as usize;
                let bus = usize::from(bus).min(BUS_COUNT - 1);
                let Some(clip) = self.clips.get(slot).and_then(|clip| clip.as_ref()) else {
                    return;
                };
                let step = step_for(rate, clip.sample_rate, self.device_sample_rate);
                let index = self.claim_voice();
                self.voices[index] = Voice {
                    id: voice,
                    slot,
                    position: 0.0,
                    step,
                    rate,
                    gain_left,
                    gain_right,
                    bus,
                    looping,
                };
            }
            Command::RetuneVoice {
                voice,
                gain_left,
                gain_right,
                rate,
            } => {
                for index in 0..self.voices.len() {
                    if self.voices[index].id != voice {
                        continue;
                    }
                    let slot = self.voices[index].slot;
                    let clip_rate = self.clips[slot].as_ref().map_or(0, |clip| clip.sample_rate);
                    self.voices[index].gain_left = gain_left;
                    self.voices[index].gain_right = gain_right;
                    self.voices[index].rate = rate;
                    self.voices[index].step = step_for(rate, clip_rate, self.device_sample_rate);
                }
            }
            Command::StopVoice { voice } => {
                for slot in &mut self.voices {
                    if slot.id == voice {
                        slot.id = 0;
                    }
                }
            }
            Command::StopClip { slot } => {
                self.silence_slot(slot as usize);
            }
            Command::StopAll => {
                for voice in &mut self.voices {
                    voice.id = 0;
                }
            }
            Command::SetMaster(volume) => self.master = sane_gain(volume),
            Command::SetBusVolume { bus, volume } => {
                if let Some(entry) = self.bus_volume.get_mut(usize::from(bus)) {
                    *entry = sane_gain(volume);
                }
            }
            Command::SetBusEnabled { bus, enabled } => {
                if let Some(entry) = self.bus_enabled.get_mut(usize::from(bus)) {
                    *entry = enabled;
                }
            }
        }
    }

    fn silence_slot(&mut self, slot: usize) {
        for voice in &mut self.voices {
            if voice.id != 0 && voice.slot == slot {
                voice.id = 0;
            }
        }
    }

    fn claim_voice(&mut self) -> usize {
        let mut oldest_one_shot: Option<(usize, u64)> = None;
        let mut oldest_any: Option<(usize, u64)> = None;
        for (index, voice) in self.voices.iter().enumerate() {
            if voice.id == 0 {
                return index;
            }
            if oldest_any.is_none_or(|(_, id)| voice.id < id) {
                oldest_any = Some((index, voice.id));
            }
            if !voice.looping && oldest_one_shot.is_none_or(|(_, id)| voice.id < id) {
                oldest_one_shot = Some((index, voice.id));
            }
        }
        oldest_one_shot.or(oldest_any).map_or(0, |(index, _)| index)
    }

    fn retire(&mut self, clip: ClipData) {
        if let Err(clip) = self.retired.push(clip) {
            std::mem::forget(clip);
            self.leaked_clips.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn grace_frames(sample_rate: f32) -> u64 {
    (f64::from(sample_rate.max(1.0)) * f64::from(IDLE_GRACE_SECONDS)) as u64
}

fn sane_gain(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 4.0)
    } else {
        1.0
    }
}

fn step_for(rate: f32, clip_sample_rate: u32, device_sample_rate: f32) -> f64 {
    if clip_sample_rate == 0 || device_sample_rate <= 0.0 {
        return 1.0;
    }
    let rate = if rate.is_finite() {
        rate.clamp(0.05, 8.0)
    } else {
        1.0
    };
    f64::from(rate) * f64::from(clip_sample_rate) / f64::from(device_sample_rate)
}

#[cfg(test)]
#[path = "tests/mixer_tests.rs"]
mod tests;
