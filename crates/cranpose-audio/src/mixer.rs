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
            let clip_rate = self.clips[slot]
                .as_ref()
                .map(|clip| clip.sample_rate)
                .unwrap_or(0);
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
        for voice in self.voices.iter_mut() {
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
                    let clip_rate = self.clips[slot]
                        .as_ref()
                        .map(|clip| clip.sample_rate)
                        .unwrap_or(0);
                    self.voices[index].gain_left = gain_left;
                    self.voices[index].gain_right = gain_right;
                    self.voices[index].rate = rate;
                    self.voices[index].step = step_for(rate, clip_rate, self.device_sample_rate);
                }
            }
            Command::StopVoice { voice } => {
                for slot in self.voices.iter_mut() {
                    if slot.id == voice {
                        slot.id = 0;
                    }
                }
            }
            Command::StopClip { slot } => {
                self.silence_slot(slot as usize);
            }
            Command::StopAll => {
                for voice in self.voices.iter_mut() {
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
        for voice in self.voices.iter_mut() {
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
        oldest_one_shot
            .or(oldest_any)
            .map(|(index, _)| index)
            .unwrap_or(0)
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
mod tests {
    use super::*;
    use crate::ring;

    struct Harness {
        commands: Producer<Command>,
        retired: Consumer<ClipData>,
        mixer: Mixer,
        leaked: Arc<AtomicU32>,
        streaming: Arc<AtomicBool>,
    }

    impl Harness {
        fn run(&mut self, frames: usize) -> RenderStatus {
            let burst = 128;
            let channels = self.mixer.device_channels;
            let mut out = vec![0.0f32; burst * channels];
            let mut status = RenderStatus::Continue;
            let mut remaining = frames;
            while remaining > 0 {
                let take = remaining.min(burst);
                status = self.mixer.render(&mut out[..take * channels]);
                remaining -= take;
            }
            status
        }
    }

    fn harness(sample_rate: f32, channels: usize) -> Harness {
        let (command_tx, command_rx) = ring::channel::<Command>(64);
        let (retired_tx, retired_rx) = ring::channel::<ClipData>(64);
        let leaked = Arc::new(AtomicU32::new(0));
        let streaming = Arc::new(AtomicBool::new(true));
        let seed = MixerSeed {
            commands: command_rx,
            retired: retired_tx,
            leaked_clips: Arc::clone(&leaked),
            underruns: Arc::new(AtomicU32::new(0)),
            streaming: Arc::clone(&streaming),
        };
        Harness {
            commands: command_tx,
            retired: retired_rx,
            mixer: Mixer::new(seed, sample_rate, channels),
            leaked,
            streaming,
        }
    }

    fn clip(samples: Vec<f32>, channels: u8, sample_rate: u32) -> ClipData {
        ClipData {
            samples: samples.into(),
            channels,
            sample_rate,
        }
    }

    fn play(voice: u64, slot: u32) -> Command {
        Command::Play {
            voice,
            slot,
            gain_left: 1.0,
            gain_right: 1.0,
            rate: 1.0,
            bus: 0,
            looping: false,
        }
    }

    #[test]
    fn renders_silence_without_voices() {
        let mut h = harness(48_000.0, 2);
        let mut out = vec![1.0f32; 8];
        h.mixer.render(&mut out);
        assert!(out.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn plays_a_one_shot_and_frees_the_voice() {
        let mut h = harness(48_000.0, 2);
        h.commands
            .push(Command::LoadClip {
                slot: 0,
                clip: clip(vec![1.0, 1.0], 1, 48_000),
            })
            .expect("queued");
        h.commands.push(play(1, 0)).expect("queued");

        let mut out = vec![0.0f32; 8];
        h.mixer.render(&mut out);
        assert_eq!(h.mixer.active_voices(), 0, "a two-frame clip ends at once");
        assert!(out[0] > 0.0 && out[1] > 0.0);
        assert_eq!(out[6], 0.0, "past the end of the clip is silent");
    }

    #[test]
    fn overlapping_voices_sum() {
        let mut h = harness(48_000.0, 2);
        h.commands
            .push(Command::LoadClip {
                slot: 0,
                clip: clip(vec![0.25; 64], 1, 48_000),
            })
            .expect("queued");
        h.commands.push(play(1, 0)).expect("queued");
        h.commands.push(play(2, 0)).expect("queued");
        h.commands.push(play(3, 0)).expect("queued");

        let mut out = vec![0.0f32; 8];
        h.mixer.render(&mut out);
        assert_eq!(h.mixer.active_voices(), 3);
        assert!(out[0] > 0.5, "three voices sum, got {}", out[0]);
    }

    #[test]
    fn output_is_clamped_to_the_nominal_range() {
        let mut h = harness(48_000.0, 2);
        h.commands
            .push(Command::LoadClip {
                slot: 0,
                clip: clip(vec![1.0; 64], 1, 48_000),
            })
            .expect("queued");
        for voice in 1..=8 {
            h.commands.push(play(voice, 0)).expect("queued");
        }
        let mut out = vec![0.0f32; 8];
        h.mixer.render(&mut out);
        assert!(out.iter().all(|s| (-1.0..=1.0).contains(s)));
        assert!((out[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn rate_shifts_the_read_position() {
        let mut h = harness(48_000.0, 1);
        let ramp: Vec<f32> = (0..64).map(|i| i as f32 / 64.0).collect();
        h.commands
            .push(Command::LoadClip {
                slot: 0,
                clip: clip(ramp, 1, 48_000),
            })
            .expect("queued");
        h.commands
            .push(Command::Play {
                voice: 1,
                slot: 0,
                gain_left: 1.0,
                gain_right: 1.0,
                rate: 2.0,
                bus: 0,
                looping: false,
            })
            .expect("queued");

        let mut out = vec![0.0f32; 4];
        h.mixer.render(&mut out);
        for (frame, sample) in out.iter().enumerate() {
            let expected = (2 * frame) as f32 / 64.0;
            assert!(
                (sample - expected).abs() < 1e-6,
                "frame {frame}: expected {expected}, got {sample}"
            );
        }
    }

    #[test]
    fn clip_sample_rate_is_resampled_to_the_device_rate() {
        let mut h = harness(48_000.0, 1);
        h.commands
            .push(Command::LoadClip {
                slot: 0,
                clip: clip(vec![0.5; 1024], 1, 24_000),
            })
            .expect("queued");
        h.commands.push(play(1, 0)).expect("queued");
        let mut out = vec![0.0f32; 8];
        h.mixer.render(&mut out);
        assert_eq!(h.mixer.active_voices(), 1);
        for _ in 0..255 {
            h.mixer.render(&mut out);
        }
        assert_eq!(
            h.mixer.active_voices(),
            1,
            "still playing after 2048 frames"
        );
    }

    #[test]
    fn looping_voice_keeps_going_until_stopped() {
        let mut h = harness(48_000.0, 2);
        h.commands
            .push(Command::LoadClip {
                slot: 0,
                clip: clip(vec![0.5, 0.5], 1, 48_000),
            })
            .expect("queued");
        h.commands
            .push(Command::Play {
                voice: 9,
                slot: 0,
                gain_left: 1.0,
                gain_right: 1.0,
                rate: 1.0,
                bus: 1,
                looping: true,
            })
            .expect("queued");

        let mut out = vec![0.0f32; 64];
        h.mixer.render(&mut out);
        assert_eq!(h.mixer.active_voices(), 1);
        assert!(out[40] != 0.0, "the loop refills the whole buffer");

        h.commands
            .push(Command::StopVoice { voice: 9 })
            .expect("queued");
        h.mixer.render(&mut out);
        assert_eq!(h.mixer.active_voices(), 0);
    }

    #[test]
    fn muting_a_bus_silences_only_that_bus() {
        let mut h = harness(48_000.0, 2);
        h.commands
            .push(Command::LoadClip {
                slot: 0,
                clip: clip(vec![1.0; 64], 1, 48_000),
            })
            .expect("queued");
        h.commands
            .push(Command::Play {
                voice: 1,
                slot: 0,
                gain_left: 0.5,
                gain_right: 0.5,
                rate: 1.0,
                bus: 1,
                looping: true,
            })
            .expect("queued");
        h.commands
            .push(Command::SetBusEnabled {
                bus: 1,
                enabled: false,
            })
            .expect("queued");

        let mut out = vec![0.0f32; 16];
        h.mixer.render(&mut out);
        assert!(out.iter().all(|sample| *sample == 0.0));
        assert_eq!(h.mixer.active_voices(), 1, "muting does not stop the voice");

        h.commands
            .push(Command::SetBusEnabled {
                bus: 1,
                enabled: true,
            })
            .expect("queued");
        h.mixer.render(&mut out);
        assert!(out[0] > 0.0, "unmuting resumes mid-track");
    }

    #[test]
    fn master_volume_scales_every_bus() {
        let mut h = harness(48_000.0, 2);
        h.commands
            .push(Command::LoadClip {
                slot: 0,
                clip: clip(vec![1.0; 64], 1, 48_000),
            })
            .expect("queued");
        h.commands
            .push(Command::Play {
                voice: 1,
                slot: 0,
                gain_left: 0.5,
                gain_right: 0.5,
                rate: 1.0,
                bus: 0,
                looping: true,
            })
            .expect("queued");
        h.commands.push(Command::SetMaster(0.0)).expect("queued");
        let mut out = vec![0.0f32; 16];
        h.mixer.render(&mut out);
        assert!(out.iter().all(|sample| *sample == 0.0));

        h.commands.push(Command::SetMaster(1.0)).expect("queued");
        h.mixer.render(&mut out);
        assert!((out[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn stop_clip_silences_every_voice_of_that_clip() {
        let mut h = harness(48_000.0, 2);
        h.commands
            .push(Command::LoadClip {
                slot: 0,
                clip: clip(vec![1.0; 64], 1, 48_000),
            })
            .expect("queued");
        h.commands
            .push(Command::LoadClip {
                slot: 1,
                clip: clip(vec![1.0; 64], 1, 48_000),
            })
            .expect("queued");
        h.commands.push(play(1, 0)).expect("queued");
        h.commands.push(play(2, 0)).expect("queued");
        h.commands.push(play(3, 1)).expect("queued");
        let mut out = vec![0.0f32; 8];
        h.mixer.render(&mut out);
        assert_eq!(h.mixer.active_voices(), 3);

        h.commands
            .push(Command::StopClip { slot: 0 })
            .expect("queued");
        h.mixer.render(&mut out);
        assert_eq!(h.mixer.active_voices(), 1);

        h.commands.push(Command::StopAll).expect("queued");
        h.mixer.render(&mut out);
        assert_eq!(h.mixer.active_voices(), 0);
    }

    #[test]
    fn voice_stealing_prefers_one_shots_over_loops() {
        let mut h = harness(48_000.0, 2);
        h.commands
            .push(Command::LoadClip {
                slot: 0,
                clip: clip(vec![1.0; 4096], 1, 48_000),
            })
            .expect("queued");
        h.commands
            .push(Command::Play {
                voice: 1,
                slot: 0,
                gain_left: 1.0,
                gain_right: 1.0,
                rate: 1.0,
                bus: 0,
                looping: true,
            })
            .expect("queued");
        let mut out = vec![0.0f32; 8];
        for voice in 2..=(MAX_VOICES as u64) {
            h.commands.push(play(voice, 0)).expect("queued");
        }
        h.mixer.render(&mut out);
        assert_eq!(h.mixer.active_voices(), MAX_VOICES);

        h.commands
            .push(play(MAX_VOICES as u64 + 1, 0))
            .expect("queued");
        h.mixer.render(&mut out);
        assert_eq!(h.mixer.active_voices(), MAX_VOICES);
        assert!(
            h.mixer.voices.iter().any(|voice| voice.id == 1),
            "the looping voice is not stolen while one-shots remain"
        );
    }

    #[test]
    fn unloading_a_clip_returns_it_to_the_ui_thread() {
        let mut h = harness(48_000.0, 2);
        h.commands
            .push(Command::LoadClip {
                slot: 3,
                clip: clip(vec![1.0; 8], 1, 48_000),
            })
            .expect("queued");
        h.commands.push(play(5, 3)).expect("queued");
        let mut out = vec![0.0f32; 4];
        h.mixer.render(&mut out);
        assert_eq!(h.mixer.active_voices(), 1);

        h.commands
            .push(Command::UnloadClip { slot: 3 })
            .expect("queued");
        h.mixer.render(&mut out);
        assert_eq!(h.mixer.active_voices(), 0);
        assert!(h.retired.pop().is_some(), "the clip came back for dropping");
        assert_eq!(h.leaked.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn replacing_a_slot_returns_the_previous_clip() {
        let mut h = harness(48_000.0, 2);
        for _ in 0..2 {
            h.commands
                .push(Command::LoadClip {
                    slot: 1,
                    clip: clip(vec![1.0; 8], 1, 48_000),
                })
                .expect("queued");
        }
        let mut out = vec![0.0f32; 4];
        h.mixer.render(&mut out);
        assert!(h.retired.pop().is_some());
        assert!(h.retired.pop().is_none());
    }

    #[test]
    fn out_of_range_slots_are_ignored() {
        let mut h = harness(48_000.0, 2);
        h.commands
            .push(Command::LoadClip {
                slot: MAX_CLIPS as u32 + 5,
                clip: clip(vec![1.0; 8], 1, 48_000),
            })
            .expect("queued");
        h.commands
            .push(Command::UnloadClip {
                slot: MAX_CLIPS as u32 + 5,
            })
            .expect("queued");
        h.commands
            .push(play(1, MAX_CLIPS as u32 + 5))
            .expect("queued");
        let mut out = vec![0.0f32; 4];
        h.mixer.render(&mut out);
        assert_eq!(h.mixer.active_voices(), 0);
        assert!(
            h.retired.pop().is_some(),
            "the rejected clip is handed back"
        );
    }

    #[test]
    fn retune_changes_gain_and_rate_of_a_running_voice() {
        let mut h = harness(48_000.0, 2);
        h.commands
            .push(Command::LoadClip {
                slot: 0,
                clip: clip(vec![1.0; 4096], 1, 48_000),
            })
            .expect("queued");
        h.commands
            .push(Command::Play {
                voice: 4,
                slot: 0,
                gain_left: 1.0,
                gain_right: 1.0,
                rate: 1.0,
                bus: 0,
                looping: true,
            })
            .expect("queued");
        let mut out = vec![0.0f32; 8];
        h.mixer.render(&mut out);
        assert!((out[0] - 1.0).abs() < 1e-6);

        h.commands
            .push(Command::RetuneVoice {
                voice: 4,
                gain_left: 0.25,
                gain_right: 0.25,
                rate: 2.0,
            })
            .expect("queued");
        h.mixer.render(&mut out);
        assert!((out[0] - 0.25).abs() < 1e-6);
        let voice = h.mixer.voices.iter().find(|v| v.id == 4).expect("running");
        assert!((voice.step - 2.0).abs() < 1e-9);
    }

    #[test]
    fn device_format_change_keeps_voice_pitch() {
        let mut h = harness(48_000.0, 2);
        h.commands
            .push(Command::LoadClip {
                slot: 0,
                clip: clip(vec![1.0; 4096], 1, 48_000),
            })
            .expect("queued");
        h.commands
            .push(Command::Play {
                voice: 1,
                slot: 0,
                gain_left: 1.0,
                gain_right: 1.0,
                rate: 1.0,
                bus: 0,
                looping: true,
            })
            .expect("queued");
        let mut out = vec![0.0f32; 8];
        h.mixer.render(&mut out);
        h.mixer.set_device_format(24_000.0, 2);
        assert_eq!(h.mixer.device_sample_rate(), 24_000.0);
        let voice = h.mixer.voices.iter().find(|v| v.id == 1).expect("running");
        assert!((voice.step - 2.0).abs() < 1e-9);
    }

    #[test]
    fn nan_gains_and_rates_do_not_wedge_the_mixer() {
        let mut h = harness(48_000.0, 2);
        h.commands
            .push(Command::LoadClip {
                slot: 0,
                clip: clip(vec![1.0; 64], 1, 48_000),
            })
            .expect("queued");
        h.commands
            .push(Command::SetMaster(f32::NAN))
            .expect("queued");
        h.commands
            .push(Command::SetBusVolume {
                bus: 0,
                volume: f32::INFINITY,
            })
            .expect("queued");
        h.commands
            .push(Command::Play {
                voice: 1,
                slot: 0,
                gain_left: 0.5,
                gain_right: 0.5,
                rate: f32::NAN,
                bus: 0,
                looping: true,
            })
            .expect("queued");
        let mut out = vec![0.0f32; 16];
        h.mixer.render(&mut out);
        assert!(out.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn silence_gives_the_device_up_after_the_grace_period() {
        let mut h = harness(48_000.0, 2);
        let grace = grace_frames(48_000.0) as usize;
        assert_eq!(h.run(grace - 128), RenderStatus::Continue);
        assert!(h.streaming.load(Ordering::SeqCst), "still inside the grace");
        assert_eq!(h.run(128), RenderStatus::Idle);
        assert!(!h.streaming.load(Ordering::SeqCst));
    }

    #[test]
    fn the_grace_period_starts_when_the_last_voice_ends() {
        let mut h = harness(48_000.0, 2);
        h.commands
            .push(Command::LoadClip {
                slot: 0,
                clip: clip(vec![0.5; 48_000], 1, 48_000),
            })
            .expect("queued");
        h.commands.push(play(1, 0)).expect("queued");

        let grace = grace_frames(48_000.0) as usize;
        assert_eq!(h.run(48_000 + grace - 128), RenderStatus::Continue);
        assert_eq!(h.run(128), RenderStatus::Idle);
    }

    #[test]
    fn a_looping_voice_holds_the_device_open_indefinitely() {
        let mut h = harness(48_000.0, 2);
        h.commands
            .push(Command::LoadClip {
                slot: 0,
                clip: clip(vec![0.5; 64], 1, 48_000),
            })
            .expect("queued");
        h.commands
            .push(Command::Play {
                voice: 1,
                slot: 0,
                gain_left: 1.0,
                gain_right: 1.0,
                rate: 1.0,
                bus: 0,
                looping: true,
            })
            .expect("queued");

        let grace = grace_frames(48_000.0) as usize;
        assert_eq!(h.run(grace * 2), RenderStatus::Continue);
        assert!(h.streaming.load(Ordering::SeqCst));
    }

    #[test]
    fn a_muted_voice_still_counts_as_a_reason_to_run() {
        let mut h = harness(48_000.0, 2);
        h.commands
            .push(Command::LoadClip {
                slot: 0,
                clip: clip(vec![0.5; 64], 1, 48_000),
            })
            .expect("queued");
        h.commands
            .push(Command::Play {
                voice: 1,
                slot: 0,
                gain_left: 1.0,
                gain_right: 1.0,
                rate: 1.0,
                bus: 1,
                looping: true,
            })
            .expect("queued");
        h.commands
            .push(Command::SetBusEnabled {
                bus: 1,
                enabled: false,
            })
            .expect("queued");

        let grace = grace_frames(48_000.0) as usize;
        assert_eq!(h.run(grace + 128), RenderStatus::Continue);
    }

    #[test]
    fn a_command_landing_while_the_stream_stops_keeps_it_alive() {
        let mut h = harness(48_000.0, 2);
        assert_eq!(h.run(grace_frames(48_000.0) as usize), RenderStatus::Idle);
        assert!(!h.streaming.load(Ordering::SeqCst));

        h.commands.push(Command::StopAll).expect("queued");
        assert_eq!(h.mixer.settle(0, 128), RenderStatus::Continue);
        assert!(h.streaming.load(Ordering::SeqCst));
    }

    #[test]
    fn the_grace_period_is_a_duration_not_a_callback_count() {
        let mut h = harness(24_000.0, 2);
        let grace = grace_frames(24_000.0) as usize;
        assert_eq!(grace * 2, grace_frames(48_000.0) as usize);
        assert_eq!(h.run(grace - 128), RenderStatus::Continue);
        assert_eq!(h.run(128), RenderStatus::Idle);
    }

    #[test]
    fn a_device_rate_change_rescales_the_grace_period() {
        let mut h = harness(48_000.0, 2);
        h.mixer.set_device_format(24_000.0, 2);
        assert_eq!(h.run(grace_frames(24_000.0) as usize), RenderStatus::Idle);
    }

    #[test]
    fn mono_device_downmixes_both_channels() {
        let mut h = harness(48_000.0, 1);
        h.commands
            .push(Command::LoadClip {
                slot: 0,
                clip: clip(vec![1.0, -1.0, 1.0, -1.0], 2, 48_000),
            })
            .expect("queued");
        h.commands.push(play(1, 0)).expect("queued");
        let mut out = vec![0.0f32; 2];
        h.mixer.render(&mut out);
        assert!(
            out[0].abs() < 1e-6,
            "opposite channels cancel in the downmix"
        );
    }
}
