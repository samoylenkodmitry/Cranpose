use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering, fence},
};

use cranpose_services::{
    AudioBus, AudioClip, AudioError, AudioPlayer, PlaybackParams, SoundId, VoiceId,
};
use parking_lot::Mutex;

use crate::{
    backend::{self, AudioSink},
    mixer::{BUS_COUNT, ClipData, Command, MAX_CLIPS, MixerSeed},
    ring,
};

const COMMAND_CAPACITY: usize = 512;

const RETIRE_CAPACITY: usize = COMMAND_CAPACITY;

type SinkOpener = Box<dyn Fn(MixerSeed) -> Result<Box<dyn AudioSink>, AudioError> + Send + Sync>;

/// A mixing audio player backed by a platform output device.
///
/// The device is opened lazily, on the first call that actually makes sound,
/// so an app that installs the engine but never plays anything costs no audio
/// thread and no battery. Loading clips is not such a call: a clip load is a
/// queue push, and the queue exists from construction, so a title screen can
/// have its whole sound bank resident with the output device still shut.
///
/// The device does not stay open either. When nothing has sounded for
/// `IDLE_GRACE_SECONDS` the mixer gives the
/// stream up and the next [`play`](AudioPlayer::play) starts it again, so a
/// silent screen costs nothing however it was reached.
pub struct AudioEngine {
    operation: Mutex<()>,
    commands: Mutex<ring::Producer<Command>>,
    retired: Mutex<ring::Consumer<ClipData>>,
    seed: Mutex<Option<MixerSeed>>,
    loaded: Mutex<Vec<Option<ClipData>>>,
    sink: Mutex<Option<Arc<dyn AudioSink>>>,
    open_sink: SinkOpener,
    free_slots: Mutex<Vec<u32>>,
    next_voice: Mutex<u64>,
    master: Mutex<f32>,
    bus_volume: Mutex<[f32; BUS_COUNT]>,
    bus_enabled: Mutex<[bool; BUS_COUNT]>,
    last_error: Mutex<Option<AudioError>>,
    device_unavailable: Mutex<bool>,
    suspended: Mutex<bool>,
    streaming: Arc<AtomicBool>,
    parked: Mutex<bool>,
    leaked_clips: Arc<AtomicU32>,
    underruns: Arc<AtomicU32>,
}

impl AudioEngine {
    /// Creates an engine that opens the platform output device on first use.
    pub fn new() -> AudioEngine {
        AudioEngine::with_sink_opener(Box::new(backend::open_mixer))
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
            operation: Mutex::new(()),
            commands: Mutex::new(command_tx),
            retired: Mutex::new(retired_rx),
            seed: Mutex::new(Some(MixerSeed {
                commands: command_rx,
                retired: retired_tx,
                leaked_clips: Arc::clone(&leaked_clips),
                underruns: Arc::clone(&underruns),
                streaming: Arc::clone(&streaming),
            })),
            loaded: Mutex::new(vec![None; MAX_CLIPS]),
            sink: Mutex::new(None),
            open_sink,
            free_slots: Mutex::new((0..MAX_CLIPS as u32).rev().collect()),
            next_voice: Mutex::new(0),
            master: Mutex::new(1.0),
            bus_volume: Mutex::new([1.0; BUS_COUNT]),
            bus_enabled: Mutex::new([true; BUS_COUNT]),
            last_error: Mutex::new(None),
            device_unavailable: Mutex::new(false),
            suspended: Mutex::new(false),
            streaming,
            parked: Mutex::new(false),
            leaked_clips,
            underruns,
        }
    }

    /// The most recent failure, if the device refused to open or a call was
    /// rejected. Cleared by reading it.
    pub fn take_last_error(&self) -> Option<AudioError> {
        self.last_error.lock().take()
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
        self.sink.lock().is_some()
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

    fn rebuild_seed(&self) -> MixerSeed {
        let (command_tx, command_rx) = ring::channel::<Command>(COMMAND_CAPACITY);
        let (retired_tx, retired_rx) = ring::channel::<ClipData>(RETIRE_CAPACITY);
        *self.commands.lock() = command_tx;
        *self.retired.lock() = retired_rx;
        MixerSeed {
            commands: command_rx,
            retired: retired_tx,
            leaked_clips: Arc::clone(&self.leaked_clips),
            underruns: Arc::clone(&self.underruns),
            streaming: Arc::clone(&self.streaming),
        }
    }

    fn refill_clips(&self) {
        let clips: Vec<(u32, ClipData)> = self
            .loaded
            .lock()
            .iter()
            .enumerate()
            .filter_map(|(slot, entry)| entry.clone().map(|data| (slot as u32, data)))
            .collect();
        if clips.is_empty() {
            return;
        }
        for (slot, clip) in clips {
            self.send(Command::LoadClip { slot, clip });
        }
    }

    fn ensure_running(&self) -> bool {
        let sink = self.sink.lock().clone();
        if sink.as_ref().is_some_and(|sink| sink.is_running()) {
            return true;
        }
        if self.sink.lock().is_some() {
            *self.sink.lock() = None;
            self.streaming.store(false, Ordering::SeqCst);
            *self.parked.lock() = false;
        }
        if *self.device_unavailable.lock() {
            return false;
        }
        let seed = match self.seed.lock().take() {
            Some(seed) => seed,
            None => self.rebuild_seed(),
        };
        self.streaming.store(true, Ordering::SeqCst);
        match (self.open_sink)(seed) {
            Ok(sink) => {
                *self.sink.lock() = Some(Arc::from(sink));
                self.publish_settings();
                self.refill_clips();
                true
            }
            Err(error) => {
                log::warn!("cranpose audio device unavailable: {error}");
                self.streaming.store(false, Ordering::SeqCst);
                *self.last_error.lock() = Some(error);
                *self.device_unavailable.lock() = true;
                false
            }
        }
    }

    fn publish_settings(&self) {
        let master = *self.master.lock();
        let volumes = *self.bus_volume.lock();
        let enabled = *self.bus_enabled.lock();
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

    fn wake_stream(&self) {
        fence(Ordering::SeqCst);
        if self.streaming.swap(true, Ordering::SeqCst) {
            return;
        }
        *self.parked.lock() = false;
        self.publish_settings();
        let sink = self.sink.lock().clone();
        if let Some(sink) = sink {
            sink.resume();
        }
    }

    fn send(&self, command: Command) {
        self.housekeeping();
        if *self.device_unavailable.lock() {
            return;
        }
        if !self.streaming.load(Ordering::Relaxed) && !survives_a_stopped_stream(&command) {
            return;
        }
        if self.commands.lock().push(command).is_err() {
            log::warn!("cranpose audio command queue is full; dropped one request");
        }
    }

    fn housekeeping(&self) {
        self.drop_dead_sink();
        self.drain_retired();
        self.park_if_idle();
    }

    fn drop_dead_sink(&self) {
        let sink = self.sink.lock().clone();
        let dead = sink.as_ref().is_some_and(|sink| !sink.is_running());
        if !dead {
            return;
        }
        *self.sink.lock() = None;
        self.streaming.store(false, Ordering::SeqCst);
        *self.parked.lock() = false;
        *self.suspended.lock() = false;
    }

    fn drain_retired(&self) {
        let mut retired = self.retired.lock();
        while let Some(clip) = retired.pop() {
            drop(clip);
        }
    }

    fn park_if_idle(&self) {
        if *self.parked.lock() || self.streaming.load(Ordering::SeqCst) {
            return;
        }
        let sink = self.sink.lock().clone();
        let Some(sink) = sink else {
            return;
        };
        sink.park();
        *self.parked.lock() = true;
        if self.streaming.load(Ordering::SeqCst) {
            sink.resume();
            *self.parked.lock() = false;
        }
    }

    fn allocate_voice(&self) -> u64 {
        let mut next = self.next_voice.lock();
        *next = next.wrapping_add(1).max(1);
        *next
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
        self.wake_stream();
        VoiceId::from_raw(voice)
    }
}

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
    fn load_clip(&self, clip: AudioClip) -> Result<SoundId, AudioError> {
        let _operation = self.operation.lock();
        self.housekeeping();
        let slot = self
            .free_slots
            .lock()
            .pop()
            .ok_or(AudioError::ClipTableFull {
                capacity: MAX_CLIPS,
            })?;
        let data = ClipData {
            samples: clip.shared_samples(),
            channels: clip.channels().min(2) as u8,
            sample_rate: clip.sample_rate(),
        };
        if let Some(entry) = self.loaded.lock().get_mut(slot as usize) {
            *entry = Some(data.clone());
        }
        self.send(Command::LoadClip { slot, clip: data });
        Ok(SoundId::from_raw(slot + 1))
    }

    fn unload(&self, id: SoundId) {
        let _operation = self.operation.lock();
        let Some(slot) = Self::slot_of(id) else {
            return;
        };
        self.send(Command::UnloadClip { slot });
        if let Some(entry) = self.loaded.lock().get_mut(slot as usize) {
            *entry = None;
        }
        let mut free = self.free_slots.lock();
        if !free.contains(&slot) {
            free.push(slot);
        }
    }

    fn play(&self, id: SoundId, params: PlaybackParams) {
        let _operation = self.operation.lock();
        self.start_voice(id, params, false);
    }

    fn play_loop(&self, id: SoundId, params: PlaybackParams) -> VoiceId {
        let _operation = self.operation.lock();
        self.start_voice(id, params, true)
    }

    fn stop(&self, id: SoundId) {
        let _operation = self.operation.lock();
        if let Some(slot) = Self::slot_of(id) {
            self.send(Command::StopClip { slot });
        }
    }

    fn stop_voice(&self, voice: VoiceId) {
        let _operation = self.operation.lock();
        if voice.is_valid() {
            self.send(Command::StopVoice { voice: voice.raw() });
        }
    }

    fn stop_all(&self) {
        let _operation = self.operation.lock();
        self.send(Command::StopAll);
    }

    fn set_voice_params(&self, voice: VoiceId, params: PlaybackParams) {
        let _operation = self.operation.lock();
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
        let _operation = self.operation.lock();
        let volume = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            1.0
        };
        *self.master.lock() = volume;
        self.send(Command::SetMaster(volume));
    }

    fn master_volume(&self) -> f32 {
        *self.master.lock()
    }

    fn set_bus_volume(&self, bus: AudioBus, volume: f32) {
        let _operation = self.operation.lock();
        let volume = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let mut volumes = *self.bus_volume.lock();
        volumes[bus.index()] = volume;
        *self.bus_volume.lock() = volumes;
        self.send(Command::SetBusVolume {
            bus: bus.index() as u8,
            volume,
        });
    }

    fn bus_volume(&self, bus: AudioBus) -> f32 {
        self.bus_volume.lock()[bus.index()]
    }

    fn set_bus_enabled(&self, bus: AudioBus, enabled: bool) {
        let _operation = self.operation.lock();
        let mut flags = *self.bus_enabled.lock();
        flags[bus.index()] = enabled;
        *self.bus_enabled.lock() = flags;
        self.send(Command::SetBusEnabled {
            bus: bus.index() as u8,
            enabled,
        });
    }

    fn bus_enabled(&self, bus: AudioBus) -> bool {
        self.bus_enabled.lock()[bus.index()]
    }

    fn suspend(&self) {
        let _operation = self.operation.lock();
        self.housekeeping();
        if !self.streaming.load(Ordering::Relaxed) {
            return;
        }
        let sink = self.sink.lock().clone();
        if let Some(sink) = sink {
            sink.suspend();
        }
        *self.suspended.lock() = true;
    }

    fn resume(&self) {
        let _operation = self.operation.lock();
        if std::mem::replace(&mut *self.suspended.lock(), false) {
            let sink = self.sink.lock().clone();
            if let Some(sink) = sink {
                sink.resume();
            }
        }
        self.housekeeping();
    }

    fn is_available(&self) -> bool {
        backend::is_compiled() && !*self.device_unavailable.lock()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    };

    use parking_lot::Mutex;

    use super::*;
    use crate::mixer::{IDLE_GRACE_SECONDS, MAX_VOICES, Mixer, RenderStatus};

    const RIG_SAMPLE_RATE: f32 = 48_000.0;
    const RIG_BURST_FRAMES: usize = 128;

    struct SharedBool(AtomicBool);
    impl SharedBool {
        fn new(value: bool) -> Self {
            Self(AtomicBool::new(value))
        }
        fn get(&self) -> bool {
            self.0.load(Ordering::Acquire)
        }
        fn set(&self, value: bool) {
            self.0.store(value, Ordering::Release);
        }
    }

    struct SharedU32(AtomicU32);
    impl SharedU32 {
        fn new(value: u32) -> Self {
            Self(AtomicU32::new(value))
        }
        fn get(&self) -> u32 {
            self.0.load(Ordering::Acquire)
        }
        fn set(&self, value: u32) {
            self.0.store(value, Ordering::Release);
        }
    }

    struct SinkLog {
        dead: SharedBool,
        suspended: SharedBool,
        parks: SharedU32,
        resumes: SharedU32,
    }

    struct TestSink {
        log: Arc<SinkLog>,
    }

    impl AudioSink for TestSink {
        fn is_running(&self) -> bool {
            !self.log.dead.get()
        }
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
        opens: Arc<SharedU32>,
        engine: AudioEngine,
        mixer: Arc<Mutex<Option<Mixer>>>,
        sink: Arc<SinkLog>,
    }

    impl Rig {
        fn new() -> Rig {
            Rig::with_failure(false)
        }

        fn with_failure(fail: bool) -> Rig {
            let mixer: Arc<Mutex<Option<Mixer>>> = Arc::new(Mutex::new(None));
            let sink = Arc::new(SinkLog {
                dead: SharedBool::new(false),
                suspended: SharedBool::new(false),
                parks: SharedU32::new(0),
                resumes: SharedU32::new(0),
            });
            let mixer_for_opener = Arc::clone(&mixer);
            let sink_for_opener = Arc::clone(&sink);
            let opens = Arc::new(SharedU32::new(0));
            let opens_for_opener = Arc::clone(&opens);
            let engine = AudioEngine::with_sink_opener(Box::new(move |seed| {
                if fail {
                    return Err(AudioError::Backend("no device in this test".into()));
                }
                opens_for_opener.set(opens_for_opener.get() + 1);
                sink_for_opener.dead.set(false);
                *mixer_for_opener.lock() = Some(Mixer::new(seed, RIG_SAMPLE_RATE, 2));
                Ok(Box::new(TestSink {
                    log: Arc::clone(&sink_for_opener),
                }))
            }));
            Rig {
                opens,
                engine,
                mixer,
                sink,
            }
        }

        fn render(&self, frames: usize) -> Vec<f32> {
            let mut out = vec![0.0f32; frames * 2];
            self.mixer
                .lock()
                .as_mut()
                .expect("device opened")
                .render(&mut out);
            out
        }

        fn run(&self, seconds: f32) -> RenderStatus {
            let mut out = vec![0.0f32; RIG_BURST_FRAMES * 2];
            let mut remaining = (RIG_SAMPLE_RATE * seconds) as usize;
            let mut status = RenderStatus::Continue;
            while remaining > 0 {
                let take = remaining.min(RIG_BURST_FRAMES);
                status = self
                    .mixer
                    .lock()
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

        fn go_idle(&self) -> RenderStatus {
            self.run(IDLE_GRACE_SECONDS + 0.1)
        }

        fn active_voices(&self) -> usize {
            self.mixer
                .lock()
                .as_ref()
                .expect("device opened")
                .active_voices()
        }

        fn device_opened(&self) -> bool {
            self.mixer.lock().is_some()
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
    fn a_stream_reclaimed_without_a_callback_is_reopened() {
        let rig = Rig::new();
        let id = rig.engine.load_clip(tone(4096)).expect("loads");
        rig.engine.play(id, PlaybackParams::new());
        assert!(
            rig.render(16)[0] > 0.0,
            "audible before the device is taken"
        );
        assert_eq!(rig.opens.get(), 1);

        rig.sink.dead.set(true);
        assert!(
            rig.engine.is_streaming(),
            "the flag is exactly as stale as on the watch"
        );

        rig.engine.play(id, PlaybackParams::new());
        assert_eq!(
            rig.opens.get(),
            2,
            "a dead stream must be dropped and reopened; presence is not liveness"
        );
        assert!(
            rig.render(16)[0] > 0.0,
            "and the reopened device must be audible"
        );
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
        let id = rig
            .engine
            .load_clip(tone(8))
            .expect("loads without a device");
        assert!(id.is_valid());
        assert!(rig.engine.take_last_error().is_none());

        rig.engine.play(id, PlaybackParams::new());
        assert!(!rig.engine.is_available());
        assert!(!rig.engine.is_running());
        assert!(!rig.engine.is_streaming());
        assert!(matches!(
            rig.engine.take_last_error(),
            Some(AudioError::Backend(_))
        ));
        assert!(rig.engine.take_last_error().is_none());

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
        assert_eq!(rig.opens.get(), 1, "an idle stream is resumed in place");
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
