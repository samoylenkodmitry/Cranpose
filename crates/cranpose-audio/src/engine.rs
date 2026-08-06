//! The UI-thread half of the engine: the [`AudioPlayer`] an app actually calls.
//!
//! Every method here is bounded work on the calling thread — decode, a little
//! arithmetic, one queue push — and never waits on the audio thread. Handle
//! bookkeeping (clip slots, voice ids) lives here so the mixer never has to
//! search for a free identifier inside its real-time budget.

use crate::backend::{self, AudioSink};
use crate::mixer::{ClipData, Command, MixerSeed, BUS_COUNT, MAX_CLIPS};
use crate::ring;
use cranpose_services::{
    AudioBus, AudioClip, AudioError, AudioPlayer, PlaybackParams, SoundId, VoiceId,
};
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// How many commands can be in flight. The audio thread drains the whole queue
/// every callback (a few milliseconds), so this is deep enough that a frame
/// firing every cue it owns at once still fits.
const COMMAND_CAPACITY: usize = 512;

/// Retired clips are produced at most one per command, and the UI thread drains
/// them on every call, so matching the command depth makes overflow impossible
/// short of an app that stops calling the engine entirely.
const RETIRE_CAPACITY: usize = COMMAND_CAPACITY;

type SinkOpener = Box<dyn Fn(MixerSeed) -> Result<Box<dyn AudioSink>, AudioError>>;

/// A mixing audio player backed by a platform output device.
///
/// The device is opened lazily, on the first call that needs sound, so an app
/// that installs the engine but never plays anything costs no audio thread and
/// no battery.
pub struct AudioEngine {
    commands: RefCell<ring::Producer<Command>>,
    retired: RefCell<ring::Consumer<ClipData>>,
    seed: RefCell<Option<MixerSeed>>,
    sink: RefCell<Option<Box<dyn AudioSink>>>,
    open_sink: SinkOpener,
    free_slots: RefCell<Vec<u32>>,
    next_voice: Cell<u64>,
    master: Cell<f32>,
    bus_volume: Cell<[f32; BUS_COUNT]>,
    bus_enabled: Cell<[bool; BUS_COUNT]>,
    last_error: RefCell<Option<AudioError>>,
    device_unavailable: Cell<bool>,
    suspended: Cell<bool>,
    leaked_clips: Arc<AtomicU32>,
    underruns: Arc<AtomicU32>,
}

impl AudioEngine {
    /// Creates an engine that opens the platform output device on first use.
    pub fn new() -> AudioEngine {
        AudioEngine::with_sink_opener(Box::new(backend::open))
    }

    /// Creates an engine over a caller-supplied device opener. The platform
    /// backends and the crate's own tests both go through this.
    pub fn with_sink_opener(open_sink: SinkOpener) -> AudioEngine {
        let (command_tx, command_rx) = ring::channel::<Command>(COMMAND_CAPACITY);
        let (retired_tx, retired_rx) = ring::channel::<ClipData>(RETIRE_CAPACITY);
        let leaked_clips = Arc::new(AtomicU32::new(0));
        let underruns = Arc::new(AtomicU32::new(0));
        AudioEngine {
            commands: RefCell::new(command_tx),
            retired: RefCell::new(retired_rx),
            seed: RefCell::new(Some(MixerSeed {
                commands: command_rx,
                retired: retired_tx,
                leaked_clips: Arc::clone(&leaked_clips),
                underruns: Arc::clone(&underruns),
            })),
            sink: RefCell::new(None),
            open_sink,
            free_slots: RefCell::new((0..MAX_CLIPS as u32).rev().collect()),
            next_voice: Cell::new(0),
            master: Cell::new(1.0),
            bus_volume: Cell::new([1.0; BUS_COUNT]),
            bus_enabled: Cell::new([true; BUS_COUNT]),
            last_error: RefCell::new(None),
            device_unavailable: Cell::new(false),
            suspended: Cell::new(false),
            leaked_clips,
            underruns,
        }
    }

    /// The most recent failure, if the device refused to open or a call was
    /// rejected. Cleared by reading it.
    pub fn take_last_error(&self) -> Option<AudioError> {
        self.last_error.borrow_mut().take()
    }

    /// How many clips the mixer could not hand back for dropping. Any value
    /// above zero means the app stopped calling the engine while clips were
    /// being replaced; it is reported rather than hidden.
    pub fn leaked_clips(&self) -> u32 {
        self.leaked_clips.load(Ordering::Relaxed)
    }

    /// How many times the device asked for a buffer the mixer could not fill.
    pub fn underruns(&self) -> u32 {
        self.underruns.load(Ordering::Relaxed)
    }

    /// Whether the output device is open.
    pub fn is_running(&self) -> bool {
        self.sink.borrow().is_some()
    }

    /// Opens the output device if it is not open yet. Returns whether a device
    /// is running afterwards.
    fn ensure_running(&self) -> bool {
        if self.sink.borrow().is_some() {
            return true;
        }
        if self.device_unavailable.get() {
            return false;
        }
        let Some(seed) = self.seed.borrow_mut().take() else {
            self.device_unavailable.set(true);
            return false;
        };
        match (self.open_sink)(seed) {
            Ok(sink) => {
                *self.sink.borrow_mut() = Some(sink);
                // Settings written before the device existed have to reach the
                // fresh mixer, which starts from its own defaults.
                let master = self.master.get();
                let volumes = self.bus_volume.get();
                let enabled = self.bus_enabled.get();
                self.send(Command::SetMaster(master));
                for bus in 0..BUS_COUNT {
                    self.send(Command::SetBusVolume {
                        bus: bus as u8,
                        volume: volumes[bus],
                    });
                    self.send(Command::SetBusEnabled {
                        bus: bus as u8,
                        enabled: enabled[bus],
                    });
                }
                true
            }
            Err(error) => {
                log::warn!("cranpose audio device unavailable: {error}");
                *self.last_error.borrow_mut() = Some(error);
                self.device_unavailable.set(true);
                false
            }
        }
    }

    /// Enqueues one command, dropping it if the queue is full rather than
    /// blocking the UI thread on the audio thread.
    fn send(&self, command: Command) {
        self.drain_retired();
        if self.device_unavailable.get() {
            return;
        }
        if self.commands.borrow_mut().push(command).is_err() {
            log::debug!("cranpose audio command queue is full; dropped one request");
        }
    }

    /// Drops clips the mixer handed back. Called from every engine entry point,
    /// which is what keeps the return ring from ever filling.
    fn drain_retired(&self) {
        let mut retired = self.retired.borrow_mut();
        while let Some(clip) = retired.pop() {
            drop(clip);
        }
    }

    fn allocate_voice(&self) -> u64 {
        let next = self.next_voice.get().wrapping_add(1).max(1);
        self.next_voice.set(next);
        next
    }

    fn slot_of(id: SoundId) -> Option<u32> {
        id.raw().checked_sub(1).filter(|slot| (*slot as usize) < MAX_CLIPS)
    }

    fn start_voice(&self, id: SoundId, params: PlaybackParams, looping: bool) -> VoiceId {
        let Some(slot) = Self::slot_of(id) else {
            return VoiceId::NONE;
        };
        if !self.ensure_running() {
            return VoiceId::NONE;
        }
        let params = params.sanitized();
        let (gain_left, gain_right) = params.gains();
        let voice = self.allocate_voice();
        self.send(Command::Play {
            voice,
            slot,
            gain_left,
            gain_right,
            rate: params.rate,
            bus: params.bus.index() as u8,
            looping,
        });
        VoiceId::from_raw(voice)
    }
}

impl Default for AudioEngine {
    fn default() -> AudioEngine {
        AudioEngine::new()
    }
}

impl AudioPlayer for AudioEngine {
    fn load_clip(&self, clip: AudioClip) -> Result<SoundId, AudioError> {
        self.drain_retired();
        if !self.ensure_running() {
            return Err(self
                .last_error
                .borrow()
                .clone()
                .unwrap_or(AudioError::Unsupported));
        }
        let slot = self
            .free_slots
            .borrow_mut()
            .pop()
            .ok_or(AudioError::ClipTableFull {
                capacity: MAX_CLIPS,
            })?;
        self.send(Command::LoadClip {
            slot,
            clip: ClipData {
                samples: clip.shared_samples(),
                channels: clip.channels().min(2) as u8,
                sample_rate: clip.sample_rate(),
            },
        });
        Ok(SoundId::from_raw(slot + 1))
    }

    fn unload(&self, id: SoundId) {
        let Some(slot) = Self::slot_of(id) else {
            return;
        };
        self.send(Command::UnloadClip { slot });
        let mut free = self.free_slots.borrow_mut();
        if !free.contains(&slot) {
            free.push(slot);
        }
    }

    fn play(&self, id: SoundId, params: PlaybackParams) {
        self.start_voice(id, params, false);
    }

    fn play_loop(&self, id: SoundId, params: PlaybackParams) -> VoiceId {
        self.start_voice(id, params, true)
    }

    fn stop(&self, id: SoundId) {
        if let Some(slot) = Self::slot_of(id) {
            self.send(Command::StopClip { slot });
        }
    }

    fn stop_voice(&self, voice: VoiceId) {
        if voice.is_valid() {
            self.send(Command::StopVoice { voice: voice.raw() });
        }
    }

    fn stop_all(&self) {
        self.send(Command::StopAll);
    }

    fn set_voice_params(&self, voice: VoiceId, params: PlaybackParams) {
        if !voice.is_valid() {
            return;
        }
        let params = params.sanitized();
        let (gain_left, gain_right) = params.gains();
        self.send(Command::RetuneVoice {
            voice: voice.raw(),
            gain_left,
            gain_right,
            rate: params.rate,
        });
    }

    fn set_master_volume(&self, volume: f32) {
        let volume = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.master.set(volume);
        self.send(Command::SetMaster(volume));
    }

    fn master_volume(&self) -> f32 {
        self.master.get()
    }

    fn set_bus_volume(&self, bus: AudioBus, volume: f32) {
        let volume = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let mut volumes = self.bus_volume.get();
        volumes[bus.index()] = volume;
        self.bus_volume.set(volumes);
        self.send(Command::SetBusVolume {
            bus: bus.index() as u8,
            volume,
        });
    }

    fn bus_volume(&self, bus: AudioBus) -> f32 {
        self.bus_volume.get()[bus.index()]
    }

    fn set_bus_enabled(&self, bus: AudioBus, enabled: bool) {
        let mut flags = self.bus_enabled.get();
        flags[bus.index()] = enabled;
        self.bus_enabled.set(flags);
        self.send(Command::SetBusEnabled {
            bus: bus.index() as u8,
            enabled,
        });
    }

    fn bus_enabled(&self, bus: AudioBus) -> bool {
        self.bus_enabled.get()[bus.index()]
    }

    fn suspend(&self) {
        if let Some(sink) = self.sink.borrow().as_ref() {
            sink.suspend();
            self.suspended.set(true);
        }
        self.drain_retired();
    }

    fn resume(&self) {
        if let Some(sink) = self.sink.borrow().as_ref() {
            sink.resume();
            self.suspended.set(false);
        }
        self.drain_retired();
    }

    fn is_available(&self) -> bool {
        backend::is_compiled() && !self.device_unavailable.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::{Mixer, MAX_VOICES};
    use std::rc::Rc;

    /// A sink that keeps the mixer where the test can drive it by hand.
    struct TestSink {
        mixer: Rc<RefCell<Option<Mixer>>>,
        suspended: Rc<Cell<bool>>,
    }

    impl AudioSink for TestSink {
        fn suspend(&self) {
            self.suspended.set(true);
        }
        fn resume(&self) {
            self.suspended.set(false);
        }
    }

    struct Rig {
        engine: AudioEngine,
        mixer: Rc<RefCell<Option<Mixer>>>,
        suspended: Rc<Cell<bool>>,
    }

    impl Rig {
        fn new() -> Rig {
            Rig::with_failure(false)
        }

        fn with_failure(fail: bool) -> Rig {
            let mixer: Rc<RefCell<Option<Mixer>>> = Rc::new(RefCell::new(None));
            let suspended = Rc::new(Cell::new(false));
            let mixer_for_opener = Rc::clone(&mixer);
            let suspended_for_opener = Rc::clone(&suspended);
            let engine = AudioEngine::with_sink_opener(Box::new(move |seed| {
                if fail {
                    return Err(AudioError::Backend("no device in this test".into()));
                }
                *mixer_for_opener.borrow_mut() = Some(Mixer::new(seed, 48_000.0, 2));
                Ok(Box::new(TestSink {
                    mixer: Rc::clone(&mixer_for_opener),
                    suspended: Rc::clone(&suspended_for_opener),
                }))
            }));
            Rig {
                engine,
                mixer,
                suspended,
            }
        }

        fn render(&self, frames: usize) -> Vec<f32> {
            let mut out = vec![0.0f32; frames * 2];
            self.mixer
                .borrow_mut()
                .as_mut()
                .expect("device opened")
                .render(&mut out);
            out
        }

        fn active_voices(&self) -> usize {
            self.mixer
                .borrow()
                .as_ref()
                .expect("device opened")
                .active_voices()
        }
    }

    fn tone(frames: usize) -> AudioClip {
        AudioClip::from_samples(vec![0.5; frames], 1, 48_000).expect("valid clip")
    }

    #[test]
    fn device_opens_lazily_on_first_load() {
        let rig = Rig::new();
        assert!(!rig.engine.is_running());
        let id = rig.engine.load_clip(tone(64)).expect("loads");
        assert!(rig.engine.is_running());
        assert!(id.is_valid());
    }

    #[test]
    fn play_reaches_the_mixer() {
        let rig = Rig::new();
        let id = rig.engine.load_clip(tone(4096)).expect("loads");
        rig.engine.play(id, PlaybackParams::new());
        let out = rig.render(16);
        assert!(out[0] > 0.0);
        assert_eq!(rig.active_voices(), 1);
    }

    #[test]
    fn rapid_retriggering_layers_voices() {
        let rig = Rig::new();
        let id = rig.engine.load_clip(tone(4096)).expect("loads");
        for _ in 0..5 {
            rig.engine.play(id, PlaybackParams::new().volume(0.1));
        }
        rig.render(8);
        assert_eq!(rig.active_voices(), 5);
    }

    #[test]
    fn voice_table_saturates_instead_of_growing() {
        let rig = Rig::new();
        let id = rig.engine.load_clip(tone(1 << 16)).expect("loads");
        for _ in 0..(MAX_VOICES * 3) {
            rig.engine.play(id, PlaybackParams::new().volume(0.01));
        }
        rig.render(8);
        assert_eq!(rig.active_voices(), MAX_VOICES);
    }

    #[test]
    fn looping_voice_can_be_stopped_by_handle() {
        let rig = Rig::new();
        let id = rig.engine.load_clip(tone(64)).expect("loads");
        let voice = rig.engine.play_loop(id, PlaybackParams::new());
        assert!(voice.is_valid());
        rig.render(8);
        assert_eq!(rig.active_voices(), 1);
        rig.engine.stop_voice(voice);
        rig.render(8);
        assert_eq!(rig.active_voices(), 0);
    }

    #[test]
    fn stop_silences_every_voice_of_a_clip() {
        let rig = Rig::new();
        let id = rig.engine.load_clip(tone(4096)).expect("loads");
        rig.engine.play(id, PlaybackParams::new());
        rig.engine.play(id, PlaybackParams::new());
        rig.render(8);
        assert_eq!(rig.active_voices(), 2);
        rig.engine.stop(id);
        rig.render(8);
        assert_eq!(rig.active_voices(), 0);
    }

    #[test]
    fn bus_toggles_survive_the_device_opening_later() {
        let rig = Rig::new();
        rig.engine.set_bus_enabled(AudioBus::Music, false);
        rig.engine.set_master_volume(0.5);
        assert!(!rig.engine.bus_enabled(AudioBus::Music));
        assert_eq!(rig.engine.master_volume(), 0.5);

        let id = rig.engine.load_clip(tone(4096)).expect("loads");
        rig.engine
            .play(id, PlaybackParams::new().bus(AudioBus::Music));
        let out = rig.render(8);
        assert!(
            out.iter().all(|sample| *sample == 0.0),
            "settings made before the device opened are applied to it"
        );

        rig.engine.set_bus_enabled(AudioBus::Music, true);
        let out = rig.render(8);
        assert!(out[0] > 0.0);
    }

    #[test]
    fn unloading_returns_the_slot_and_stops_the_sound() {
        let rig = Rig::new();
        let id = rig.engine.load_clip(tone(4096)).expect("loads");
        rig.engine.play(id, PlaybackParams::new());
        rig.render(8);
        assert_eq!(rig.active_voices(), 1);
        rig.engine.unload(id);
        rig.render(8);
        assert_eq!(rig.active_voices(), 0);

        let again = rig.engine.load_clip(tone(64)).expect("loads");
        assert_eq!(again, id, "the slot is reused");
        assert_eq!(rig.engine.leaked_clips(), 0);
    }

    #[test]
    fn clip_table_reports_when_it_is_full() {
        let rig = Rig::new();
        for _ in 0..MAX_CLIPS {
            rig.engine.load_clip(tone(2)).expect("loads");
        }
        assert_eq!(
            rig.engine.load_clip(tone(2)),
            Err(AudioError::ClipTableFull {
                capacity: MAX_CLIPS
            })
        );
    }

    #[test]
    fn invalid_handles_are_ignored() {
        let rig = Rig::new();
        rig.engine.load_clip(tone(64)).expect("loads");
        rig.engine.play(SoundId::NONE, PlaybackParams::new());
        assert_eq!(
            rig.engine.play_loop(SoundId::NONE, PlaybackParams::new()),
            VoiceId::NONE
        );
        rig.engine.stop(SoundId::NONE);
        rig.engine.unload(SoundId::NONE);
        rig.engine.stop_voice(VoiceId::NONE);
        rig.engine.set_voice_params(VoiceId::NONE, PlaybackParams::new());
        rig.render(4);
        assert_eq!(rig.active_voices(), 0);
    }

    #[test]
    fn retuning_a_voice_reaches_the_mixer() {
        let rig = Rig::new();
        let id = rig.engine.load_clip(tone(4096)).expect("loads");
        let voice = rig.engine.play_loop(id, PlaybackParams::new());
        let before = rig.render(8);
        rig.engine
            .set_voice_params(voice, PlaybackParams::new().volume(0.0));
        let after = rig.render(8);
        assert!(before[0] > 0.0);
        assert_eq!(after[0], 0.0);
    }

    #[test]
    fn suspend_and_resume_reach_the_sink() {
        let rig = Rig::new();
        rig.engine.load_clip(tone(8)).expect("loads");
        rig.engine.suspend();
        assert!(rig.suspended.get());
        rig.engine.resume();
        assert!(!rig.suspended.get());
    }

    #[test]
    fn a_device_that_will_not_open_degrades_to_silence() {
        let rig = Rig::with_failure(true);
        let error = rig.engine.load_clip(tone(8)).expect_err("no device");
        assert!(matches!(error, AudioError::Backend(_)));
        assert!(!rig.engine.is_available());
        assert!(!rig.engine.is_running());

        // Every later call is a no-op instead of a panic.
        rig.engine.play(SoundId::from_raw(1), PlaybackParams::new());
        assert_eq!(
            rig.engine.play_loop(SoundId::from_raw(1), PlaybackParams::new()),
            VoiceId::NONE
        );
        rig.engine.stop_all();
        rig.engine.set_master_volume(0.5);
        rig.engine.suspend();
        rig.engine.resume();
        assert!(rig.engine.take_last_error().is_some());
        assert!(rig.engine.take_last_error().is_none());
        assert_eq!(rig.engine.underruns(), 0);
    }

    #[test]
    fn wav_bytes_decode_through_the_default_load() {
        let rig = Rig::new();
        let mut bytes = Vec::new();
        let data = [0i16, 16_384, -16_384, 0];
        let pcm: Vec<u8> = data.iter().flat_map(|s| s.to_le_bytes()).collect();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36u32 + pcm.len() as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&48_000u32.to_le_bytes());
        bytes.extend_from_slice(&96_000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&pcm);

        let id = rig.engine.load(&bytes).expect("decodes and loads");
        rig.engine.play(id, PlaybackParams::new());
        let out = rig.render(4);
        assert!(out.iter().any(|sample| *sample != 0.0));
        assert!(rig.engine.load(b"not audio").is_err());
    }
}
