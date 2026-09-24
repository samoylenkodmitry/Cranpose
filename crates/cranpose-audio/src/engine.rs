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
#[path = "tests/engine_tests.rs"]
mod tests;
