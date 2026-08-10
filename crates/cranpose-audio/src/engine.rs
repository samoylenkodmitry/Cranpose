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
use std::sync::atomic::{fence, AtomicBool, AtomicU32, Ordering};
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
/// The device is opened lazily, on the first call that actually makes sound,
/// so an app that installs the engine but never plays anything costs no audio
/// thread and no battery. Loading clips is not such a call: a clip load is a
/// queue push, and the queue exists from construction, so a title screen can
/// have its whole sound bank resident with the output device still shut.
///
/// The device does not stay open either. When nothing has sounded for
/// [`IDLE_GRACE_SECONDS`](crate::mixer::IDLE_GRACE_SECONDS) the mixer gives the
/// stream up and the next [`play`](AudioPlayer::play) starts it again, so a
/// silent screen costs nothing however it was reached.
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
    /// Whether the output stream is producing audio. Written by both threads:
    /// the mixer clears it when it goes idle, this side sets it when it starts
    /// the stream. See [`AudioEngine::wake_stream`].
    streaming: Arc<AtomicBool>,
    /// Whether this side has already released the device for the current idle
    /// stretch, so it is released once rather than on every call the app makes
    /// while nothing is playing.
    parked: Cell<bool>,
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
        let streaming = Arc::new(AtomicBool::new(false));
        AudioEngine {
            commands: RefCell::new(command_tx),
            retired: RefCell::new(retired_rx),
            seed: RefCell::new(Some(MixerSeed {
                commands: command_rx,
                retired: retired_tx,
                leaked_clips: Arc::clone(&leaked_clips),
                underruns: Arc::clone(&underruns),
                streaming: Arc::clone(&streaming),
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
            streaming,
            parked: Cell::new(false),
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
    ///
    /// Open is not the same as running: a device that has been open for a while
    /// spends most of a quiet screen stopped. See
    /// [`is_streaming`](AudioEngine::is_streaming).
    pub fn is_running(&self) -> bool {
        self.sink.borrow().is_some()
    }

    /// Whether the output stream is live rather than given up as idle.
    ///
    /// `false` with [`is_running`](AudioEngine::is_running) `true` is the
    /// steady state of a silent screen: the device object and every loaded clip
    /// are still there, the stream is not, and the next play starts it again.
    /// A stream paused by [`suspend`](AudioPlayer::suspend) still counts as
    /// live — the app took it away, not the mixer, and it comes back on
    /// [`resume`](AudioPlayer::resume).
    pub fn is_streaming(&self) -> bool {
        self.streaming.load(Ordering::Relaxed)
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
        // Set before the opener runs, not after: the backend starts the stream
        // inside it, and the first callback can land before it returns.
        self.streaming.store(true, Ordering::SeqCst);
        match (self.open_sink)(seed) {
            Ok(sink) => {
                *self.sink.borrow_mut() = Some(sink);
                self.publish_settings();
                true
            }
            Err(error) => {
                log::warn!("cranpose audio device unavailable: {error}");
                self.streaming.store(false, Ordering::SeqCst);
                *self.last_error.borrow_mut() = Some(error);
                self.device_unavailable.set(true);
                false
            }
        }
    }

    /// Sends the engine's mix settings to a mixer that does not have them: a
    /// fresh one, which starts from its own defaults, or one that was stopped
    /// while the app changed a volume (see [`AudioEngine::send`]).
    fn publish_settings(&self) {
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
    }

    /// Starts a stream the mixer gave up, once a command is already queued for
    /// it.
    ///
    /// The order is the whole point and is not an accident of layout: the
    /// caller pushes first and this reads the flag afterwards, while the mixer
    /// publishes the stop first and re-reads the queue afterwards. The fence
    /// puts both pairs into one order, so at least one side always sees the
    /// other's work — see `Mixer::settle` for the matching half. Reading the
    /// flag before the push instead would let a command land in a queue that
    /// nothing will ever drain.
    fn wake_stream(&self) {
        fence(Ordering::SeqCst);
        if self.streaming.swap(true, Ordering::SeqCst) {
            return;
        }
        self.parked.set(false);
        // Anything the app set while the stream was stopped was kept in this
        // struct rather than queued, so the mixer is told about it now — and
        // before the stream starts, so the settings and the sound that woke it
        // arrive in the same drain rather than a buffer apart.
        self.publish_settings();
        if let Some(sink) = self.sink.borrow().as_ref() {
            sink.resume();
        }
    }

    /// Enqueues one command, dropping it if the queue is full rather than
    /// blocking the UI thread on the audio thread.
    fn send(&self, command: Command) {
        self.housekeeping();
        if self.device_unavailable.get() {
            return;
        }
        // With the stream stopped nothing drains the queue, so only commands
        // the mixer must not miss are worth a slot in it. The mixer only stops
        // with every voice silent, which makes anything acting on a voice a
        // no-op, and the gains live in this struct and are re-sent by
        // `wake_stream`. Queueing the rest would let a volume slider dragged on
        // a silent screen fill the ring and push out a real clip load.
        if !self.streaming.load(Ordering::Relaxed) && !survives_a_stopped_stream(&command) {
            return;
        }
        if self.commands.borrow_mut().push(command).is_err() {
            log::debug!("cranpose audio command queue is full; dropped one request");
        }
    }

    /// What every entry point does first: drop clips the mixer handed back, and
    /// release a device the mixer has reported idle.
    fn housekeeping(&self) {
        self.drain_retired();
        self.park_if_idle();
    }

    /// Drops clips the mixer handed back. Called from every engine entry point,
    /// which is what keeps the return ring from ever filling.
    fn drain_retired(&self) {
        let mut retired = self.retired.borrow_mut();
        while let Some(clip) = retired.pop() {
            drop(clip);
        }
    }

    /// Releases the device once the mixer has reported it idle.
    ///
    /// This is the UI-thread half of stopping, and it is best-effort by nature:
    /// it can only run when the app calls the engine. On Android that is a
    /// backstop — the AAudio callback returns `Stop` and the stream winds
    /// itself down — but on cpal, whose callback cannot stop its own stream, it
    /// is the only thing that does the job.
    fn park_if_idle(&self) {
        if self.parked.get() || self.streaming.load(Ordering::SeqCst) {
            return;
        }
        let sink = self.sink.borrow();
        let Some(sink) = sink.as_ref() else {
            return;
        };
        sink.park();
        self.parked.set(true);
        if self.streaming.load(Ordering::SeqCst) {
            // The mixer changed its mind in the callback that raced this one:
            // it re-checks the queue after publishing the stop and carries on
            // if work had arrived. Undo the release rather than leave a live
            // mixer behind a dead device.
            sink.resume();
            self.parked.set(false);
        }
    }

    fn allocate_voice(&self) -> u64 {
        let next = self.next_voice.get().wrapping_add(1).max(1);
        self.next_voice.set(next);
        next
    }

    fn slot_of(id: SoundId) -> Option<u32> {
        id.raw()
            .checked_sub(1)
            .filter(|slot| (*slot as usize) < MAX_CLIPS)
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
        // Only after the command is queued; `wake_stream` explains why.
        self.wake_stream();
        VoiceId::from_raw(voice)
    }
}

/// Whether a command still means anything to a mixer whose stream is stopped.
///
/// `Play` is on the list because [`AudioEngine::start_voice`] has to queue it
/// before it starts the stream, not after.
fn survives_a_stopped_stream(command: &Command) -> bool {
    matches!(
        command,
        Command::LoadClip { .. } | Command::UnloadClip { .. } | Command::Play { .. }
    )
}

impl Default for AudioEngine {
    fn default() -> AudioEngine {
        AudioEngine::new()
    }
}

impl AudioPlayer for AudioEngine {
    /// Takes a clip table slot and queues the clip for the mixer.
    ///
    /// This deliberately does not open the output device. Loading a bank of
    /// cues is what an app does on the way into a screen, long before it plays
    /// anything, and opening the device there was costing a silent title screen
    /// an audio thread and an always-on DSP rail for as long as it was on
    /// display. The command ring outlives every mixer, so the load waits in it
    /// and is drained by the first mixer to start.
    ///
    /// The consequence is a narrower error contract than this used to have.
    /// The only failure it can still report is the one it can determine here,
    /// [`AudioError::ClipTableFull`]; a device that is missing or refuses to
    /// open is no longer a load-time error, because finding that out means
    /// opening it. Callers that need to know ask
    /// [`is_available`](AudioPlayer::is_available), and the failure itself is
    /// available from [`take_last_error`](AudioEngine::take_last_error) once a
    /// play has tried. That also makes this agree with `NoopAudioPlayer`, which
    /// hands out real [`SoundId`]s on a machine with no audio at all so app
    /// logic does not have to branch.
    fn load_clip(&self, clip: AudioClip) -> Result<SoundId, AudioError> {
        self.housekeeping();
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
        self.housekeeping();
        // A stream the mixer already gave up needs no pausing, and pausing a
        // stopped stream is an error on AAudio. Not recording a suspend here is
        // what keeps `resume` from starting a device the app has no sound for.
        if !self.streaming.load(Ordering::Relaxed) {
            return;
        }
        if let Some(sink) = self.sink.borrow().as_ref() {
            sink.suspend();
        }
        self.suspended.set(true);
    }

    fn resume(&self) {
        if self.suspended.replace(false) {
            if let Some(sink) = self.sink.borrow().as_ref() {
                sink.resume();
            }
        }
        self.housekeeping();
    }

    fn is_available(&self) -> bool {
        backend::is_compiled() && !self.device_unavailable.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::{Mixer, RenderStatus, IDLE_GRACE_SECONDS, MAX_VOICES};
    use std::rc::Rc;

    /// The device rate the rig's mixer runs at, and the burst size its
    /// callbacks arrive in. 128 frames at 48 kHz is a realistic AAudio burst.
    const RIG_SAMPLE_RATE: f32 = 48_000.0;
    const RIG_BURST_FRAMES: usize = 128;

    /// What the engine did to the sink, so a test can tell "still running" from
    /// "released and started again".
    #[derive(Default)]
    struct SinkLog {
        suspended: Cell<bool>,
        parks: Cell<u32>,
        resumes: Cell<u32>,
    }

    /// A sink that keeps the mixer where the test can drive it by hand.
    struct TestSink {
        log: Rc<SinkLog>,
    }

    impl AudioSink for TestSink {
        fn suspend(&self) {
            self.log.suspended.set(true);
        }
        fn resume(&self) {
            self.log.suspended.set(false);
            self.log.resumes.set(self.log.resumes.get() + 1);
        }
        fn park(&self) {
            self.log.parks.set(self.log.parks.get() + 1);
        }
    }

    struct Rig {
        engine: AudioEngine,
        mixer: Rc<RefCell<Option<Mixer>>>,
        sink: Rc<SinkLog>,
    }

    impl Rig {
        fn new() -> Rig {
            Rig::with_failure(false)
        }

        fn with_failure(fail: bool) -> Rig {
            let mixer: Rc<RefCell<Option<Mixer>>> = Rc::new(RefCell::new(None));
            let sink = Rc::new(SinkLog::default());
            let mixer_for_opener = Rc::clone(&mixer);
            let sink_for_opener = Rc::clone(&sink);
            let engine = AudioEngine::with_sink_opener(Box::new(move |seed| {
                if fail {
                    return Err(AudioError::Backend("no device in this test".into()));
                }
                *mixer_for_opener.borrow_mut() = Some(Mixer::new(seed, RIG_SAMPLE_RATE, 2));
                Ok(Box::new(TestSink {
                    log: Rc::clone(&sink_for_opener),
                }))
            }));
            Rig {
                engine,
                mixer,
                sink,
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

        /// Runs the mixer for `seconds` in device-sized bursts, stopping early
        /// the moment it asks for the stream to be released — which is what a
        /// real device does, and what makes "did it stop?" observable.
        fn run(&self, seconds: f32) -> RenderStatus {
            let mut out = vec![0.0f32; RIG_BURST_FRAMES * 2];
            let mut remaining = (RIG_SAMPLE_RATE * seconds) as usize;
            let mut status = RenderStatus::Continue;
            while remaining > 0 {
                let take = remaining.min(RIG_BURST_FRAMES);
                status = self
                    .mixer
                    .borrow_mut()
                    .as_mut()
                    .expect("device opened")
                    .render(&mut out[..take * 2]);
                remaining -= take;
                if status == RenderStatus::Idle {
                    break;
                }
            }
            status
        }

        /// Runs past the idle grace period, the way a screen nobody is touching
        /// does.
        fn go_idle(&self) -> RenderStatus {
            self.run(IDLE_GRACE_SECONDS + 0.1)
        }

        fn active_voices(&self) -> usize {
            self.mixer
                .borrow()
                .as_ref()
                .expect("device opened")
                .active_voices()
        }

        fn device_opened(&self) -> bool {
            self.mixer.borrow().is_some()
        }
    }

    fn tone(frames: usize) -> AudioClip {
        AudioClip::from_samples(vec![0.5; frames], 1, 48_000).expect("valid clip")
    }

    #[test]
    fn loading_a_clip_does_not_open_the_device() {
        let rig = Rig::new();
        for _ in 0..8 {
            assert!(rig.engine.load_clip(tone(64)).expect("loads").is_valid());
        }
        assert!(
            !rig.engine.is_running() && !rig.device_opened(),
            "a bank loaded on the way into a screen must not cost an audio thread"
        );
        assert!(!rig.engine.is_streaming());
    }

    #[test]
    fn the_first_play_opens_the_device_and_drains_the_queued_loads() {
        let rig = Rig::new();
        let id = rig.engine.load_clip(tone(4096)).expect("loads");
        assert!(!rig.engine.is_running());

        rig.engine.play(id, PlaybackParams::new());
        assert!(rig.engine.is_running());
        assert!(rig.engine.is_streaming());

        let out = rig.render(16);
        assert!(out[0] > 0.0, "the load queued before the mixer existed ran");
        assert_eq!(rig.active_voices(), 1);
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
        rig.engine
            .set_voice_params(VoiceId::NONE, PlaybackParams::new());
        assert!(
            !rig.engine.is_running(),
            "nothing here could make a sound, so nothing needed a device"
        );
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
        let id = rig.engine.load_clip(tone(1 << 16)).expect("loads");
        rig.engine.play_loop(id, PlaybackParams::new());
        rig.engine.suspend();
        assert!(rig.sink.suspended.get());
        rig.engine.resume();
        assert!(!rig.sink.suspended.get());
    }

    #[test]
    fn suspending_a_stream_the_mixer_already_released_touches_nothing() {
        let rig = Rig::new();
        let id = rig.engine.load_clip(tone(64)).expect("loads");
        rig.engine.play(id, PlaybackParams::new());
        assert_eq!(rig.go_idle(), RenderStatus::Idle);

        rig.engine.suspend();
        assert!(
            !rig.sink.suspended.get(),
            "there is nothing left to pause, and pausing a stopped AAudio \
             stream is an error"
        );
        let resumes = rig.sink.resumes.get();
        rig.engine.resume();
        assert_eq!(
            rig.sink.resumes.get(),
            resumes,
            "coming back to the foreground on a silent screen must not start a \
             device the app has no sound for"
        );
        assert!(!rig.engine.is_streaming());
    }

    #[test]
    fn a_device_that_will_not_open_degrades_to_silence() {
        let rig = Rig::with_failure(true);
        // Loading no longer touches the device, so it no longer discovers that
        // there isn't one; it hands back a real handle exactly as the no-op
        // player would.
        let id = rig
            .engine
            .load_clip(tone(8))
            .expect("loads without a device");
        assert!(id.is_valid());
        assert!(rig.engine.take_last_error().is_none());

        // The first play is what tries to open the device, and what reports it.
        rig.engine.play(id, PlaybackParams::new());
        assert!(!rig.engine.is_available());
        assert!(!rig.engine.is_running());
        assert!(!rig.engine.is_streaming());
        assert!(matches!(
            rig.engine.take_last_error(),
            Some(AudioError::Backend(_))
        ));
        assert!(rig.engine.take_last_error().is_none());

        // Every later call is a no-op instead of a panic.
        assert_eq!(
            rig.engine.play_loop(id, PlaybackParams::new()),
            VoiceId::NONE
        );
        rig.engine.stop_all();
        rig.engine.set_master_volume(0.5);
        rig.engine.suspend();
        rig.engine.resume();
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

    #[test]
    fn a_screen_that_falls_silent_releases_the_device() {
        let rig = Rig::new();
        let id = rig.engine.load_clip(tone(64)).expect("loads");
        rig.engine.play(id, PlaybackParams::new());
        assert!(rig.engine.is_streaming());

        assert_eq!(rig.go_idle(), RenderStatus::Idle);
        assert!(!rig.engine.is_streaming());
        assert!(
            rig.engine.is_running(),
            "the device object and its clips outlive the stream"
        );

        // The backend half. AAudio's callback returning `Stop` is only part of
        // it; this is the call that gives the route back.
        assert_eq!(rig.sink.parks.get(), 0, "nothing has called the engine yet");
        rig.engine.stop_all();
        assert_eq!(rig.sink.parks.get(), 1);
        rig.engine.stop_all();
        assert_eq!(rig.sink.parks.get(), 1, "released once, not once per call");
    }

    #[test]
    fn a_play_after_going_idle_starts_the_stream_and_is_heard() {
        let rig = Rig::new();
        let id = rig.engine.load_clip(tone(4096)).expect("loads");
        rig.engine.play(id, PlaybackParams::new());
        assert_eq!(rig.go_idle(), RenderStatus::Idle);
        rig.engine.stop_all();
        let resumes = rig.sink.resumes.get();

        rig.engine.play(id, PlaybackParams::new());
        assert!(rig.engine.is_streaming());
        assert_eq!(rig.sink.resumes.get(), resumes + 1, "the stream restarted");

        let out = rig.render(16);
        assert_eq!(rig.active_voices(), 1, "the voice really is running");
        assert!(out[0] > 0.0);
        assert_eq!(rig.run(0.5), RenderStatus::Continue, "and it stays running");
    }

    #[test]
    fn settings_changed_while_the_device_is_released_survive_the_restart() {
        let rig = Rig::new();
        let id = rig.engine.load_clip(tone(4096)).expect("loads");
        rig.engine.play(id, PlaybackParams::new());
        assert_eq!(rig.go_idle(), RenderStatus::Idle);

        // A settings screen with the stream stopped: nothing drains the queue,
        // so these are kept here rather than queued, and re-sent on restart.
        rig.engine.set_master_volume(0.0);
        rig.engine.play(id, PlaybackParams::new());
        let out = rig.render(16);
        assert!(
            out.iter().all(|sample| *sample == 0.0),
            "the master volume set while the device was released was applied"
        );

        rig.engine.set_master_volume(1.0);
        let out = rig.render(16);
        assert!(out[0] > 0.0);
    }

    #[test]
    fn a_stopped_stream_does_not_let_chatter_push_out_a_clip_load() {
        let rig = Rig::new();
        let id = rig.engine.load_clip(tone(64)).expect("loads");
        rig.engine.play(id, PlaybackParams::new());
        assert_eq!(rig.go_idle(), RenderStatus::Idle);

        // A volume slider dragged for far longer than the command ring is deep.
        for step in 0..(COMMAND_CAPACITY * 4) {
            rig.engine
                .set_master_volume(step as f32 / (COMMAND_CAPACITY * 4) as f32);
        }
        let late = rig.engine.load_clip(tone(4096)).expect("loads");

        rig.engine.play(late, PlaybackParams::new());
        let out = rig.render(16);
        assert!(
            out[0] > 0.0,
            "the clip loaded after the chatter still reached the mixer"
        );
    }

    #[test]
    fn rapid_cues_inside_the_grace_period_never_stop_the_stream() {
        let rig = Rig::new();
        let id = rig.engine.load_clip(tone(48_000)).expect("loads");

        // A player tapping through a menu for three times the grace period: a
        // cue every 300 ms, each one cut short after 50 ms the way a UI sound
        // is when the next screen arrives.
        for _ in 0..20 {
            rig.engine.play(id, PlaybackParams::new());
            assert_eq!(rig.run(0.05), RenderStatus::Continue);
            rig.engine.stop(id);
            assert_eq!(
                rig.run(0.25),
                RenderStatus::Continue,
                "the stream must not be given up between taps"
            );
        }
        assert!(rig.engine.is_streaming());
        assert_eq!(rig.sink.parks.get(), 0);
        assert_eq!(
            rig.sink.resumes.get(),
            0,
            "no restart means the device was never given up"
        );
    }

    #[test]
    fn a_cue_shorter_than_one_callback_still_restarts_the_grace_period() {
        let rig = Rig::new();
        // Two milliseconds: shorter than the burst the device asks for, so the
        // voice is gone again by the time the callback returns.
        let id = rig.engine.load_clip(tone(96)).expect("loads");
        rig.engine.play(id, PlaybackParams::new());

        assert_eq!(rig.run(IDLE_GRACE_SECONDS - 0.1), RenderStatus::Continue);
        rig.engine.play(id, PlaybackParams::new());
        assert_eq!(
            rig.run(IDLE_GRACE_SECONDS - 0.1),
            RenderStatus::Continue,
            "the second cue restarted the clock rather than being missed"
        );
        assert_eq!(rig.run(0.2), RenderStatus::Idle);
    }

    #[test]
    fn a_looping_voice_keeps_the_device_for_as_long_as_it_plays() {
        let rig = Rig::new();
        let id = rig.engine.load_clip(tone(4096)).expect("loads");
        let voice = rig.engine.play_loop(id, PlaybackParams::new());

        assert_eq!(rig.run(IDLE_GRACE_SECONDS * 2.0), RenderStatus::Continue);
        assert!(rig.engine.is_streaming());

        rig.engine.stop_voice(voice);
        assert_eq!(rig.go_idle(), RenderStatus::Idle);
        assert!(!rig.engine.is_streaming());
    }
}
