//! Sound effects and music — the audio analogue of [`haptics`](crate::haptics).
//!
//! ```rust,ignore
//! # use cranpose_services::*;
//! const CUES: &[SoundSpec] = &[
//!     SoundSpec::new("hit", include_bytes!("hit.wav")),
//!     SoundSpec::new("music", include_bytes!("theme.wav")),
//! ];
//!
//! #[composable]
//! fn Game() {
//!     let bank = rememberSoundBank(CUES);
//!     let combo = rememberMutableStateOf(|| 0u32);
//!     // A combo counter raises the pitch of the same cue.
//!     bank.play_with(0, PlaybackParams::new().pitch_semitones(combo.value() as f32));
//! }
//! ```
//!
//! # Threading
//!
//! A real backend owns a real-time audio thread; the handle only enqueues
//! commands for it. Decoding happens in [`AudioPlayer::load`], never during
//! playback, so a cue that fires every few frames costs one queue push.

mod wav;

use crate::registry::ServiceRegistry;
use cranpose_core::compositionLocalOfWithPolicy;
use cranpose_core::CompositionLocal;
use cranpose_core::CompositionLocalProvider;
use cranpose_macros::composable;
use parking_lot::Mutex;
use std::cell::RefCell;
use std::fmt;
use std::ops::Index;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

/// Identifies a decoded clip held by the audio engine.
///
/// Handles are opaque and cheap to copy; store them in an array indexed by the
/// app's own cue enum. [`SoundId::NONE`] never plays anything.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SoundId(u32);

impl SoundId {
    /// A handle that refers to no clip. Playing it is a no-op.
    pub const NONE: SoundId = SoundId(0);

    /// Wraps a backend-assigned raw handle. Backends allocate from 1 upward.
    pub fn from_raw(raw: u32) -> Self {
        SoundId(raw)
    }

    /// The backend-assigned raw handle.
    pub fn raw(self) -> u32 {
        self.0
    }

    /// Whether this handle refers to a clip.
    pub fn is_valid(self) -> bool {
        self.0 != 0
    }
}

/// Identifies one playing voice, so a looping cue can be stopped or retuned
/// while it runs.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct VoiceId(u64);

impl VoiceId {
    /// A handle that refers to no voice. Stopping it is a no-op.
    pub const NONE: VoiceId = VoiceId(0);

    /// Wraps a backend-assigned raw handle. Backends allocate from 1 upward.
    pub fn from_raw(raw: u64) -> Self {
        VoiceId(raw)
    }

    /// The backend-assigned raw handle.
    pub fn raw(self) -> u64 {
        self.0
    }

    /// Whether this handle refers to a voice.
    pub fn is_valid(self) -> bool {
        self.0 != 0
    }
}

/// The two mix buses every app gets, so "sound on" and "music on" are one call
/// each rather than bookkeeping over individual voices.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum AudioBus {
    /// Short cues: hits, pickups, UI clicks.
    #[default]
    Effects,
    /// Sustained background material.
    Music,
}

impl AudioBus {
    /// Every bus, in index order.
    pub const ALL: [AudioBus; 2] = [AudioBus::Effects, AudioBus::Music];

    /// The bus's dense index, for backend-side arrays.
    pub fn index(self) -> usize {
        match self {
            AudioBus::Effects => 0,
            AudioBus::Music => 1,
        }
    }

    /// The bus for a dense index produced by [`AudioBus::index`].
    pub fn from_index(index: usize) -> Option<AudioBus> {
        AudioBus::ALL.get(index).copied()
    }
}

/// How one voice should sound.
///
/// Build with the chained setters — `PlaybackParams::new().volume(0.4).pan(-0.3)`
/// reads like the named arguments a Compose call site would use.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PlaybackParams {
    /// Linear gain. `1.0` is the clip's recorded level.
    pub volume: f32,
    /// Playback rate, which shifts pitch with it. `1.0` is the recorded pitch,
    /// `2.0` is an octave up.
    pub rate: f32,
    /// Stereo position from `-1.0` (hard left) through `0.0` to `1.0`.
    pub pan: f32,
    /// The mix bus this voice belongs to.
    pub bus: AudioBus,
}

impl PlaybackParams {
    /// The neutral parameters: unity gain, recorded pitch, centred, effects bus.
    pub const DEFAULT: PlaybackParams = PlaybackParams {
        volume: 1.0,
        rate: 1.0,
        pan: 0.0,
        bus: AudioBus::Effects,
    };

    /// The slowest rate a backend honours.
    pub const MIN_RATE: f32 = 0.05;
    /// The fastest rate a backend honours.
    pub const MAX_RATE: f32 = 8.0;
    /// The loudest gain a backend honours, leaving room to amplify a quiet clip.
    pub const MAX_VOLUME: f32 = 4.0;

    /// The neutral parameters. Same as [`PlaybackParams::DEFAULT`].
    pub const fn new() -> PlaybackParams {
        PlaybackParams::DEFAULT
    }

    /// Sets the linear gain.
    pub fn volume(mut self, volume: f32) -> PlaybackParams {
        self.volume = volume;
        self
    }

    /// Sets the playback rate (and with it the pitch).
    pub fn rate(mut self, rate: f32) -> PlaybackParams {
        self.rate = rate;
        self
    }

    /// Sets the stereo position.
    pub fn pan(mut self, pan: f32) -> PlaybackParams {
        self.pan = pan;
        self
    }

    /// Sets the mix bus.
    pub fn bus(mut self, bus: AudioBus) -> PlaybackParams {
        self.bus = bus;
        self
    }

    /// Sets the rate from a pitch offset in equal-tempered semitones.
    ///
    /// A rising combo counter reads better as `pitch_semitones(combo as f32)`
    /// than as a hand-computed ratio.
    pub fn pitch_semitones(self, semitones: f32) -> PlaybackParams {
        let semitones = if semitones.is_finite() {
            semitones
        } else {
            0.0
        };
        self.rate(2.0f32.powf(semitones / 12.0))
    }

    /// Clamps every field into the range backends honour and scrubs `NaN`.
    ///
    /// Backends call this before handing parameters to the audio thread, which
    /// is why a `NaN` rate from app arithmetic cannot wedge a voice.
    pub fn sanitized(self) -> PlaybackParams {
        fn finite(value: f32, fallback: f32) -> f32 {
            if value.is_finite() {
                value
            } else {
                fallback
            }
        }
        PlaybackParams {
            volume: finite(self.volume, 1.0).clamp(0.0, PlaybackParams::MAX_VOLUME),
            rate: finite(self.rate, 1.0).clamp(PlaybackParams::MIN_RATE, PlaybackParams::MAX_RATE),
            pan: finite(self.pan, 0.0).clamp(-1.0, 1.0),
            bus: self.bus,
        }
    }

    /// Constant-power left/right gains for these parameters, volume included.
    ///
    /// Computed on the caller's thread so the audio callback only multiplies.
    pub fn gains(self) -> (f32, f32) {
        let params = self.sanitized();
        // pan -1..1 maps to 0..PI/2, so cos/sin sweep one full constant-power arc.
        let angle = (params.pan + 1.0) * std::f32::consts::FRAC_PI_4;
        (params.volume * angle.cos(), params.volume * angle.sin())
    }
}

impl Default for PlaybackParams {
    fn default() -> PlaybackParams {
        PlaybackParams::DEFAULT
    }
}

/// Decoded PCM held in memory, ready to play with no further work.
///
/// Samples are interleaved `f32` in `-1.0..=1.0`, one or two channels. The
/// buffer sits behind an [`Arc`] so handing a clip to the audio thread is a
/// refcount bump rather than a copy.
#[derive(Clone)]
pub struct AudioClip {
    samples: Arc<[f32]>,
    channels: u16,
    sample_rate: u32,
}

impl AudioClip {
    /// The largest clip the framework accepts, guarding a malformed header from
    /// turning into a multi-gigabyte allocation.
    pub const MAX_FRAMES: usize = 1 << 26;

    /// Wraps already-decoded interleaved samples.
    pub fn from_samples(
        samples: Vec<f32>,
        channels: u16,
        sample_rate: u32,
    ) -> Result<AudioClip, AudioError> {
        if channels == 0 || channels > 2 {
            return Err(AudioError::UnsupportedFormat(format!(
                "clips must be mono or stereo, got {channels} channels"
            )));
        }
        if sample_rate == 0 {
            return Err(AudioError::UnsupportedFormat(
                "clips must declare a non-zero sample rate".to_string(),
            ));
        }
        if samples.is_empty() {
            return Err(AudioError::Decode("clip holds no samples".to_string()));
        }
        if !samples.len().is_multiple_of(usize::from(channels)) {
            return Err(AudioError::Decode(format!(
                "clip holds {} samples, which is not a whole number of {channels}-channel frames",
                samples.len()
            )));
        }
        if samples.len() / usize::from(channels) > AudioClip::MAX_FRAMES {
            return Err(AudioError::Decode(
                "clip exceeds the maximum in-memory length".to_string(),
            ));
        }
        Ok(AudioClip {
            samples: samples.into(),
            channels,
            sample_rate,
        })
    }

    /// Decodes an encoded clip. RIFF/WAVE is understood everywhere; anything
    /// else reports [`AudioError::UnsupportedFormat`].
    ///
    /// Run this wherever the bytes arrive — including a worker thread — and
    /// hand the result to [`AudioPlayer::load_clip`] when it is ready.
    pub fn decode(bytes: &[u8]) -> Result<AudioClip, AudioError> {
        if wav::is_wav(bytes) {
            wav::decode(bytes)
        } else {
            Err(AudioError::UnsupportedFormat(
                "only RIFF/WAVE clips are decoded by the framework".to_string(),
            ))
        }
    }

    /// The interleaved samples.
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// The shared sample buffer, for a backend that keeps its own reference.
    pub fn shared_samples(&self) -> Arc<[f32]> {
        Arc::clone(&self.samples)
    }

    /// Channel count: 1 or 2.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// The clip's recorded sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Frame count (samples divided by channels).
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels)
    }

    /// Playing length in seconds at unity rate.
    pub fn duration_secs(&self) -> f32 {
        self.frames() as f32 / self.sample_rate as f32
    }
}

impl fmt::Debug for AudioClip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudioClip")
            .field("frames", &self.frames())
            .field("channels", &self.channels)
            .field("sample_rate", &self.sample_rate)
            .finish()
    }
}

/// Why an audio call could not be honoured.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AudioError {
    /// No audio backend is installed on this target.
    #[error("no audio backend is installed")]
    Unsupported,
    /// The container or codec is not one the framework decodes.
    #[error("unsupported audio format: {0}")]
    UnsupportedFormat(String),
    /// The bytes claimed a supported format but did not parse.
    #[error("failed to decode audio: {0}")]
    Decode(String),
    /// The engine already holds its maximum number of clips.
    #[error("the audio engine already holds its maximum of {capacity} clips")]
    ClipTableFull {
        /// How many clips the engine can hold at once.
        capacity: usize,
    },
    /// The platform audio device refused the request.
    #[error("audio backend failure: {0}")]
    Backend(String),
}

/// Plays sound. Installed by the platform backend; the default is a no-op.
///
/// Only [`load_clip`](AudioPlayer::load_clip), [`play`](AudioPlayer::play),
/// [`play_loop`](AudioPlayer::play_loop), [`stop`](AudioPlayer::stop),
/// [`stop_voice`](AudioPlayer::stop_voice) and
/// [`set_master_volume`](AudioPlayer::set_master_volume) must be implemented;
/// everything else has a defaulted body so a partial backend still compiles.
pub trait AudioPlayer: Send + Sync {
    /// Hands an already-decoded clip to the engine.
    fn load_clip(&self, clip: AudioClip) -> Result<SoundId, AudioError>;

    /// Decodes `bytes` and loads the result.
    ///
    /// Decoding happens here, on the calling thread, so that
    /// [`play`](AudioPlayer::play) is only a queue push. To keep even the load
    /// off the UI thread, decode with [`AudioClip::decode`] elsewhere and call
    /// [`load_clip`](AudioPlayer::load_clip).
    fn load(&self, bytes: &[u8]) -> Result<SoundId, AudioError> {
        self.load_clip(AudioClip::decode(bytes)?)
    }

    /// Releases a clip. Voices already playing it are stopped.
    fn unload(&self, _id: SoundId) {}

    /// Starts a one-shot voice. Re-triggering the same clip layers a new voice
    /// rather than restarting the old one.
    fn play(&self, id: SoundId, params: PlaybackParams);

    /// Starts a looping voice and returns its handle.
    fn play_loop(&self, id: SoundId, params: PlaybackParams) -> VoiceId;

    /// Stops every voice playing `id`.
    fn stop(&self, id: SoundId);

    /// Stops one voice.
    fn stop_voice(&self, voice: VoiceId);

    /// Stops every voice on every bus.
    fn stop_all(&self) {}

    /// Retunes a voice while it plays — a looping engine note that follows
    /// speed, for instance.
    fn set_voice_params(&self, _voice: VoiceId, _params: PlaybackParams) {}

    /// Sets the gain applied to every bus.
    fn set_master_volume(&self, volume: f32);

    /// The current master gain.
    fn master_volume(&self) -> f32 {
        1.0
    }

    /// Sets one bus's gain.
    fn set_bus_volume(&self, _bus: AudioBus, _volume: f32) {}

    /// One bus's gain.
    fn bus_volume(&self, _bus: AudioBus) -> f32 {
        1.0
    }

    /// Mutes or unmutes a bus. This is the "sound on" / "music on" toggle;
    /// muting leaves voices running so unmuting resumes mid-track.
    fn set_bus_enabled(&self, _bus: AudioBus, _enabled: bool) {}

    /// Whether a bus is audible.
    fn bus_enabled(&self, _bus: AudioBus) -> bool {
        true
    }

    /// Releases the output device without discarding loaded clips — call it
    /// when the app goes to the background.
    fn suspend(&self) {}

    /// Re-acquires the output device after [`suspend`](AudioPlayer::suspend).
    fn resume(&self) {}

    /// Whether a real device is behind this handle. The no-op default reports
    /// `false`, so an app can honestly grey out its audio settings.
    fn is_available(&self) -> bool {
        false
    }
}

/// Shared handle to the installed [`AudioPlayer`].
pub type AudioPlayerRef = Arc<dyn AudioPlayer>;

/// The player used when no backend is installed.
///
/// It still allocates real [`SoundId`]s and [`VoiceId`]s and remembers the
/// volume and bus settings written to it, so app logic and settings screens
/// behave identically with and without a device.
#[derive(Default)]
pub struct NoopAudioPlayer {
    next_sound: Mutex<u32>,
    next_voice: Mutex<u64>,
    master: Mutex<f32>,
    bus_volumes: Mutex<[f32; 2]>,
    bus_enabled: Mutex<[bool; 2]>,
}

impl NoopAudioPlayer {
    /// Creates a player that accepts everything and produces no sound.
    pub fn new() -> NoopAudioPlayer {
        NoopAudioPlayer {
            next_sound: Mutex::new(0),
            next_voice: Mutex::new(0),
            master: Mutex::new(1.0),
            bus_volumes: Mutex::new([1.0, 1.0]),
            bus_enabled: Mutex::new([true, true]),
        }
    }
}

impl AudioPlayer for NoopAudioPlayer {
    fn load_clip(&self, _clip: AudioClip) -> Result<SoundId, AudioError> {
        let mut next = self.next_sound.lock();
        *next = next.saturating_add(1);
        Ok(SoundId::from_raw(*next))
    }

    fn play(&self, _id: SoundId, _params: PlaybackParams) {}

    fn play_loop(&self, id: SoundId, _params: PlaybackParams) -> VoiceId {
        if !id.is_valid() {
            return VoiceId::NONE;
        }
        let mut next = self.next_voice.lock();
        *next = next.saturating_add(1);
        VoiceId::from_raw(*next)
    }

    fn stop(&self, _id: SoundId) {}

    fn stop_voice(&self, _voice: VoiceId) {}

    fn set_master_volume(&self, volume: f32) {
        *self.master.lock() = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            1.0
        };
    }

    fn master_volume(&self) -> f32 {
        *self.master.lock()
    }

    fn set_bus_volume(&self, bus: AudioBus, volume: f32) {
        let mut volumes = self.bus_volumes.lock();
        volumes[bus.index()] = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            1.0
        };
    }

    fn bus_volume(&self, bus: AudioBus) -> f32 {
        self.bus_volumes.lock()[bus.index()]
    }

    fn set_bus_enabled(&self, bus: AudioBus, enabled: bool) {
        self.bus_enabled.lock()[bus.index()] = enabled;
    }

    fn bus_enabled(&self, bus: AudioBus) -> bool {
        self.bus_enabled.lock()[bus.index()]
    }
}

static PLATFORM_AUDIO: ServiceRegistry<dyn AudioPlayer> = ServiceRegistry::new();
static NOOP_AUDIO: OnceLock<AudioPlayerRef> = OnceLock::new();
static DEFAULT_AUDIO: OnceLock<AudioPlayerRef> = OnceLock::new();

struct PlatformAudioPlayer;

fn registered_audio() -> AudioPlayerRef {
    PLATFORM_AUDIO.get_or_warn("audio").unwrap_or_else(|| {
        NOOP_AUDIO
            .get_or_init(|| Arc::new(NoopAudioPlayer::new()))
            .clone()
    })
}

impl AudioPlayer for PlatformAudioPlayer {
    fn load_clip(&self, clip: AudioClip) -> Result<SoundId, AudioError> {
        registered_audio().load_clip(clip)
    }

    fn unload(&self, id: SoundId) {
        registered_audio().unload(id);
    }

    fn play(&self, id: SoundId, params: PlaybackParams) {
        registered_audio().play(id, params);
    }

    fn play_loop(&self, id: SoundId, params: PlaybackParams) -> VoiceId {
        registered_audio().play_loop(id, params)
    }

    fn stop(&self, id: SoundId) {
        registered_audio().stop(id);
    }

    fn stop_voice(&self, voice: VoiceId) {
        registered_audio().stop_voice(voice);
    }

    fn stop_all(&self) {
        registered_audio().stop_all();
    }

    fn set_voice_params(&self, voice: VoiceId, params: PlaybackParams) {
        registered_audio().set_voice_params(voice, params);
    }

    fn set_master_volume(&self, volume: f32) {
        registered_audio().set_master_volume(volume);
    }

    fn master_volume(&self) -> f32 {
        registered_audio().master_volume()
    }

    fn set_bus_volume(&self, bus: AudioBus, volume: f32) {
        registered_audio().set_bus_volume(bus, volume);
    }

    fn bus_volume(&self, bus: AudioBus) -> f32 {
        registered_audio().bus_volume(bus)
    }

    fn set_bus_enabled(&self, bus: AudioBus, enabled: bool) {
        registered_audio().set_bus_enabled(bus, enabled);
    }

    fn bus_enabled(&self, bus: AudioBus) -> bool {
        registered_audio().bus_enabled(bus)
    }

    fn suspend(&self) {
        registered_audio().suspend();
    }

    fn resume(&self) {
        registered_audio().resume();
    }

    fn is_available(&self) -> bool {
        registered_audio().is_available()
    }
}

/// Installs a platform audio player, replacing any previous one.
pub fn set_platform_audio(player: AudioPlayerRef) {
    PLATFORM_AUDIO.set(player);
}

/// Removes any registered platform audio player (tests and teardown).
pub fn clear_platform_audio() {
    PLATFORM_AUDIO.clear();
}

/// The installed platform player, or a no-op one.
pub fn default_audio() -> AudioPlayerRef {
    DEFAULT_AUDIO
        .get_or_init(|| Arc::new(PlatformAudioPlayer))
        .clone()
}

/// The CompositionLocal descendants read to reach the audio player.
pub fn local_audio() -> CompositionLocal<AudioPlayerRef> {
    thread_local! {
        static LOCAL_AUDIO: RefCell<Option<CompositionLocal<AudioPlayerRef>>> = const { RefCell::new(None) };
    }

    LOCAL_AUDIO.with(|cell| {
        let mut local = cell.borrow_mut();
        local
            .get_or_insert_with(|| compositionLocalOfWithPolicy(default_audio, Arc::ptr_eq))
            .clone()
    })
}

/// Provides the installed audio player to `content`.
#[allow(non_snake_case)]
#[composable]
pub fn ProvideAudio(content: impl FnOnce()) {
    let player = cranpose_core::remember(default_audio).with(|state| state.clone());
    let local = local_audio();
    CompositionLocalProvider(vec![local.provides(player)], move || {
        content();
    });
}

/// One entry in a [`SoundBank`]: where the bytes come from and how the cue
/// should sit in the mix before per-call parameters are applied.
#[derive(Clone, Copy, Debug)]
pub struct SoundSpec<'a> {
    /// A stable name used for lookups and error reporting.
    pub name: &'static str,
    /// The encoded clip, typically an `include_bytes!` WAV.
    pub bytes: &'a [u8],
    /// The cue's own level, so a loud explosion and a quiet tick can share one
    /// call site without per-call bookkeeping.
    pub base_volume: f32,
    /// The bus the cue plays on.
    pub bus: AudioBus,
}

impl<'a> SoundSpec<'a> {
    /// A cue at unity gain on the effects bus.
    pub fn new(name: &'static str, bytes: &'a [u8]) -> SoundSpec<'a> {
        SoundSpec {
            name,
            bytes,
            base_volume: 1.0,
            bus: AudioBus::Effects,
        }
    }

    /// Sets the cue's own level.
    pub fn volume(mut self, base_volume: f32) -> SoundSpec<'a> {
        self.base_volume = base_volume;
        self
    }

    /// Sets the cue's bus.
    pub fn bus(mut self, bus: AudioBus) -> SoundSpec<'a> {
        self.bus = bus;
        self
    }
}

/// A loaded cue inside a [`SoundBank`].
#[derive(Clone, Copy, Debug)]
pub struct SoundBankEntry {
    /// The name from the [`SoundSpec`].
    pub name: &'static str,
    /// The engine handle.
    pub id: SoundId,
    /// The cue's own level.
    pub base_volume: f32,
    /// The cue's bus.
    pub bus: AudioBus,
}

/// A cue that did not load, reported instead of silently vanishing.
#[derive(Clone, Debug)]
pub struct SoundBankFailure {
    /// The name from the [`SoundSpec`].
    pub name: &'static str,
    /// Why it did not load.
    pub error: AudioError,
}

struct SoundBankInner {
    player: AudioPlayerRef,
    entries: Vec<SoundBankEntry>,
    failures: Vec<SoundBankFailure>,
}

impl Drop for SoundBankInner {
    fn drop(&mut self) {
        for entry in &self.entries {
            self.player.unload(entry.id);
        }
    }
}

/// A set of cues loaded once and kept alive for as long as the composable that
/// remembered it stays in the composition.
///
/// Indexing is positional, so an app's cue enum casts straight to an index;
/// [`SoundBank::find`] covers the cases where a name is more convenient. The
/// clips are released when the last clone drops.
#[derive(Clone)]
pub struct SoundBank {
    inner: Rc<SoundBankInner>,
}

impl SoundBank {
    /// Loads every spec through `player`, collecting the ones that fail rather
    /// than aborting the whole bank.
    pub fn load(player: AudioPlayerRef, specs: &[SoundSpec<'_>]) -> SoundBank {
        let mut entries = Vec::with_capacity(specs.len());
        let mut failures = Vec::new();
        for spec in specs {
            match player.load(spec.bytes) {
                Ok(id) => entries.push(SoundBankEntry {
                    name: spec.name,
                    id,
                    base_volume: spec.base_volume,
                    bus: spec.bus,
                }),
                Err(error) => {
                    // A missing cue must not take the rest of the bank with it:
                    // the entry keeps its slot with `SoundId::NONE` so every
                    // later index still lines up with the app's cue enum.
                    entries.push(SoundBankEntry {
                        name: spec.name,
                        id: SoundId::NONE,
                        base_volume: spec.base_volume,
                        bus: spec.bus,
                    });
                    failures.push(SoundBankFailure {
                        name: spec.name,
                        error,
                    });
                }
            }
        }
        SoundBank {
            inner: Rc::new(SoundBankInner {
                player,
                entries,
                failures,
            }),
        }
    }

    /// How many cues the bank holds, failures included.
    pub fn len(&self) -> usize {
        self.inner.entries.len()
    }

    /// Whether the bank holds no cues.
    pub fn is_empty(&self) -> bool {
        self.inner.entries.is_empty()
    }

    /// The cues, in spec order.
    pub fn entries(&self) -> &[SoundBankEntry] {
        &self.inner.entries
    }

    /// The cues that failed to load.
    pub fn failures(&self) -> &[SoundBankFailure] {
        &self.inner.failures
    }

    /// The player the bank loaded through.
    pub fn player(&self) -> AudioPlayerRef {
        Arc::clone(&self.inner.player)
    }

    /// The handle at `index`, or [`SoundId::NONE`] when out of range.
    pub fn id(&self, index: usize) -> SoundId {
        self.inner
            .entries
            .get(index)
            .map(|entry| entry.id)
            .unwrap_or(SoundId::NONE)
    }

    /// The handle for `name`, if the bank holds it.
    pub fn find(&self, name: &str) -> Option<SoundId> {
        self.inner
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| entry.id)
    }

    /// Plays a cue at its own level.
    pub fn play(&self, index: usize) {
        self.play_with(index, PlaybackParams::DEFAULT);
    }

    /// Plays a cue, multiplying `params.volume` by the cue's own level. The
    /// bus comes from the spec, so a cue cannot escape its bus by accident.
    pub fn play_with(&self, index: usize, params: PlaybackParams) {
        let Some(entry) = self.inner.entries.get(index) else {
            return;
        };
        if !entry.id.is_valid() {
            return;
        }
        self.inner.player.play(entry.id, entry.apply(params));
    }

    /// Plays the cue called `name`, if the bank holds it.
    pub fn play_named(&self, name: &str, params: PlaybackParams) {
        if let Some(index) = self.inner.entries.iter().position(|e| e.name == name) {
            self.play_with(index, params);
        }
    }

    /// Starts a looping voice for a cue.
    pub fn play_loop(&self, index: usize, params: PlaybackParams) -> VoiceId {
        let Some(entry) = self.inner.entries.get(index) else {
            return VoiceId::NONE;
        };
        if !entry.id.is_valid() {
            return VoiceId::NONE;
        }
        self.inner.player.play_loop(entry.id, entry.apply(params))
    }

    /// Stops every voice of a cue.
    pub fn stop(&self, index: usize) {
        let id = self.id(index);
        if id.is_valid() {
            self.inner.player.stop(id);
        }
    }
}

impl SoundBankEntry {
    fn apply(&self, params: PlaybackParams) -> PlaybackParams {
        PlaybackParams {
            volume: params.volume * self.base_volume,
            rate: params.rate,
            pan: params.pan,
            bus: self.bus,
        }
    }
}

impl fmt::Debug for SoundBank {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SoundBank")
            .field("entries", &self.inner.entries.len())
            .field("failures", &self.inner.failures.len())
            .finish()
    }
}

impl Index<usize> for SoundBank {
    type Output = SoundId;

    fn index(&self, index: usize) -> &SoundId {
        static NONE: SoundId = SoundId::NONE;
        self.inner
            .entries
            .get(index)
            .map(|entry| &entry.id)
            .unwrap_or(&NONE)
    }
}

/// Loads a set of cues once and keeps them alive across recompositions.
///
/// The bank is rebuilt only when the spec list changes shape (its length or
/// the set of names), which is the `remember(key)` contract applied to a
/// resource that costs a decode.
#[allow(non_snake_case)]
#[composable(no_skip)]
pub fn rememberSoundBank(specs: &[SoundSpec<'_>]) -> SoundBank {
    let key = sound_bank_key(specs);
    let player = local_audio().current();
    cranpose_core::rememberKeyed((key, Arc::as_ptr(&player) as *const () as usize), |_| {
        SoundBank::load(Arc::clone(&player), specs)
    })
}

/// A cheap fingerprint of a spec list: its length plus an FNV-1a hash of the
/// cue names, so a changed cue set reloads without hashing megabytes of PCM.
fn sound_bank_key(specs: &[SoundSpec<'_>]) -> (usize, u64) {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for spec in specs {
        for byte in spec.name.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash ^= spec.bytes.len() as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (specs.len(), hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_test_composition;
    use parking_lot::Mutex;
    use std::cell::Cell;

    /// A minimal single-frame mono WAVE stream.
    fn tiny_wav() -> Vec<u8> {
        let data = 0i16.to_le_bytes();
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36u32 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&8000u32.to_le_bytes());
        out.extend_from_slice(&16000u32.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    #[derive(Default)]
    struct RecordingPlayer {
        played: Mutex<Vec<(SoundId, PlaybackParams)>>,
        unloaded: Mutex<Vec<SoundId>>,
        next: Mutex<u32>,
    }

    impl AudioPlayer for RecordingPlayer {
        fn load_clip(&self, _clip: AudioClip) -> Result<SoundId, AudioError> {
            let mut next = self.next.lock();
            *next += 1;
            Ok(SoundId::from_raw(*next))
        }
        fn play(&self, id: SoundId, params: PlaybackParams) {
            self.played.lock().push((id, params));
        }
        fn play_loop(&self, _id: SoundId, _params: PlaybackParams) -> VoiceId {
            VoiceId::from_raw(7)
        }
        fn stop(&self, _id: SoundId) {}
        fn stop_voice(&self, _voice: VoiceId) {}
        fn set_master_volume(&self, _volume: f32) {}
        fn unload(&self, id: SoundId) {
            self.unloaded.lock().push(id);
        }
        fn is_available(&self) -> bool {
            true
        }
    }

    #[test]
    fn playback_params_default_is_neutral() {
        let params = PlaybackParams::default();
        assert_eq!(params.volume, 1.0);
        assert_eq!(params.rate, 1.0);
        assert_eq!(params.pan, 0.0);
        assert_eq!(params.bus, AudioBus::Effects);
        assert_eq!(params, PlaybackParams::new());
        assert_eq!(params, PlaybackParams::DEFAULT);
    }

    #[test]
    fn playback_params_sanitizes_out_of_range_and_nan() {
        let wild = PlaybackParams {
            volume: f32::NAN,
            rate: 1_000.0,
            pan: -9.0,
            bus: AudioBus::Music,
        }
        .sanitized();
        assert_eq!(wild.volume, 1.0);
        assert_eq!(wild.rate, PlaybackParams::MAX_RATE);
        assert_eq!(wild.pan, -1.0);
        assert_eq!(wild.bus, AudioBus::Music);

        let slow = PlaybackParams::new().rate(0.0).sanitized();
        assert_eq!(slow.rate, PlaybackParams::MIN_RATE);
    }

    #[test]
    fn pitch_semitones_maps_octaves_to_rate() {
        let up = PlaybackParams::new().pitch_semitones(12.0);
        assert!((up.rate - 2.0).abs() < 1e-5);
        let down = PlaybackParams::new().pitch_semitones(-12.0);
        assert!((down.rate - 0.5).abs() < 1e-5);
        let broken = PlaybackParams::new().pitch_semitones(f32::NAN);
        assert_eq!(broken.rate, 1.0);
    }

    #[test]
    fn pan_gains_are_constant_power() {
        let (left, right) = PlaybackParams::new().gains();
        assert!((left - right).abs() < 1e-6);
        assert!((left * left + right * right - 1.0).abs() < 1e-5);

        let (left, right) = PlaybackParams::new().pan(-1.0).gains();
        assert!((left - 1.0).abs() < 1e-5);
        assert!(right.abs() < 1e-5);

        let (left, right) = PlaybackParams::new().pan(1.0).gains();
        assert!(left.abs() < 1e-5);
        assert!((right - 1.0).abs() < 1e-5);
    }

    #[test]
    fn audio_bus_indices_round_trip() {
        for bus in AudioBus::ALL {
            assert_eq!(AudioBus::from_index(bus.index()), Some(bus));
        }
        assert_eq!(AudioBus::from_index(2), None);
        assert_eq!(AudioBus::default(), AudioBus::Effects);
    }

    #[test]
    fn noop_player_hands_out_handles_and_keeps_settings() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_audio();
        let player = default_audio();
        assert!(!player.is_available());

        let id = player.load(&tiny_wav()).expect("no-op load succeeds");
        assert!(id.is_valid());
        let second = player.load(&tiny_wav()).expect("no-op load succeeds");
        assert_ne!(id, second);

        // None of these do anything, and none of them panic.
        player.play(id, PlaybackParams::new());
        let voice = player.play_loop(id, PlaybackParams::new());
        assert!(voice.is_valid());
        player.stop_voice(voice);
        player.stop(id);
        player.stop_all();
        player.set_voice_params(voice, PlaybackParams::new());
        player.unload(id);
        player.suspend();
        player.resume();

        player.set_master_volume(0.25);
        assert_eq!(player.master_volume(), 0.25);
        player.set_master_volume(f32::NAN);
        assert_eq!(player.master_volume(), 1.0);
        player.set_bus_enabled(AudioBus::Music, false);
        assert!(!player.bus_enabled(AudioBus::Music));
        assert!(player.bus_enabled(AudioBus::Effects));
        player.set_bus_volume(AudioBus::Effects, 0.5);
        assert_eq!(player.bus_volume(AudioBus::Effects), 0.5);
    }

    #[test]
    fn noop_player_rejects_invalid_loop_handle() {
        let _guard = crate::registry::test_service_guard();
        let player = NoopAudioPlayer::new();
        assert_eq!(
            player.play_loop(SoundId::NONE, PlaybackParams::new()),
            VoiceId::NONE
        );
    }

    #[test]
    fn registered_player_replaces_the_default() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_audio();
        assert!(!default_audio().is_available());
        let player: AudioPlayerRef = Arc::new(RecordingPlayer::default());
        set_platform_audio(player);
        assert!(default_audio().is_available());
        clear_platform_audio();
        assert!(!default_audio().is_available());
    }

    #[test]
    fn audio_clip_validates_shape() {
        assert!(matches!(
            AudioClip::from_samples(vec![0.0], 0, 44_100),
            Err(AudioError::UnsupportedFormat(_))
        ));
        assert!(matches!(
            AudioClip::from_samples(vec![0.0], 3, 44_100),
            Err(AudioError::UnsupportedFormat(_))
        ));
        assert!(matches!(
            AudioClip::from_samples(vec![0.0], 1, 0),
            Err(AudioError::UnsupportedFormat(_))
        ));
        assert!(matches!(
            AudioClip::from_samples(Vec::new(), 1, 44_100),
            Err(AudioError::Decode(_))
        ));
        assert!(matches!(
            AudioClip::from_samples(vec![0.0, 0.0, 0.0], 2, 44_100),
            Err(AudioError::Decode(_))
        ));

        let clip = AudioClip::from_samples(vec![0.0, 0.5], 2, 44_100).expect("valid clip");
        assert_eq!(clip.frames(), 1);
        assert_eq!(clip.channels(), 2);
        assert!(clip.duration_secs() > 0.0);
        assert_eq!(clip.shared_samples().len(), 2);
        assert!(format!("{clip:?}").contains("AudioClip"));
    }

    #[test]
    fn audio_clip_decode_rejects_unknown_container() {
        assert!(matches!(
            AudioClip::decode(b"OggS not really"),
            Err(AudioError::UnsupportedFormat(_))
        ));
    }

    #[test]
    fn sound_bank_loads_applies_base_volume_and_unloads_on_drop() {
        let player = Arc::new(RecordingPlayer::default());
        let wav = tiny_wav();
        let specs = [
            SoundSpec::new("hit", &wav).volume(0.5),
            SoundSpec::new("music", &wav).bus(AudioBus::Music),
            SoundSpec::new("broken", b"not audio"),
        ];
        let player_ref: AudioPlayerRef = player.clone();
        let bank = SoundBank::load(player_ref, &specs);

        assert_eq!(bank.len(), 3);
        assert!(!bank.is_empty());
        assert_eq!(bank.failures().len(), 1);
        assert_eq!(bank.failures()[0].name, "broken");
        assert!(!bank.id(2).is_valid());
        assert_eq!(bank.find("music"), Some(bank.id(1)));
        assert_eq!(bank.find("absent"), None);
        assert_eq!(bank[0], bank.id(0));
        assert_eq!(bank[99], SoundId::NONE);
        assert!(format!("{bank:?}").contains("SoundBank"));

        bank.play(0);
        bank.play_with(1, PlaybackParams::new().volume(0.5));
        bank.play_named("hit", PlaybackParams::new().pan(1.0));
        bank.play_with(2, PlaybackParams::new());
        bank.play_named("absent", PlaybackParams::new());
        assert_eq!(bank.play_loop(2, PlaybackParams::new()), VoiceId::NONE);
        assert!(bank.play_loop(0, PlaybackParams::new()).is_valid());
        assert_eq!(bank.play_loop(99, PlaybackParams::new()), VoiceId::NONE);
        bank.stop(0);
        bank.stop(2);

        let played = player.played.lock().clone();
        assert_eq!(played.len(), 3);
        assert!((played[0].1.volume - 0.5).abs() < 1e-6);
        assert_eq!(played[0].1.bus, AudioBus::Effects);
        assert!((played[1].1.volume - 0.5).abs() < 1e-6);
        assert_eq!(played[1].1.bus, AudioBus::Music);
        assert!((played[2].1.pan - 1.0).abs() < 1e-6);

        drop(bank);
        assert_eq!(player.unloaded.lock().len(), 3);
    }

    #[test]
    fn sound_bank_key_tracks_names_and_lengths() {
        let a = [1u8, 2, 3];
        let b = [1u8, 2, 3, 4];
        assert_eq!(
            sound_bank_key(&[SoundSpec::new("x", &a)]),
            sound_bank_key(&[SoundSpec::new("x", &a)])
        );
        assert_ne!(
            sound_bank_key(&[SoundSpec::new("x", &a)]),
            sound_bank_key(&[SoundSpec::new("y", &a)])
        );
        assert_ne!(
            sound_bank_key(&[SoundSpec::new("x", &a)]),
            sound_bank_key(&[SoundSpec::new("x", &b)])
        );
        assert_ne!(
            sound_bank_key(&[SoundSpec::new("x", &a)]),
            sound_bank_key(&[SoundSpec::new("x", &a), SoundSpec::new("x", &a)])
        );
    }

    #[test]
    fn provide_audio_publishes_the_platform_player() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_audio();
        let player: AudioPlayerRef = Arc::new(RecordingPlayer::default());
        set_platform_audio(player);

        let captured = Rc::new(RefCell::new(None));
        {
            let captured = Rc::clone(&captured);
            run_test_composition(move || {
                let captured = Rc::clone(&captured);
                ProvideAudio(move || {
                    *captured.borrow_mut() = Some(local_audio().current().is_available());
                });
            });
        }

        assert_eq!(*captured.borrow(), Some(true));
        clear_platform_audio();
    }

    #[test]
    fn local_audio_defaults_to_the_noop_player() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_audio();
        let captured = Rc::new(RefCell::new(None));
        {
            let captured = Rc::clone(&captured);
            run_test_composition(move || {
                let captured = Rc::clone(&captured);
                ProvideAudio(move || {
                    *captured.borrow_mut() = Some(local_audio().current().is_available());
                });
            });
        }
        assert_eq!(*captured.borrow(), Some(false));
    }

    #[test]
    fn remember_sound_bank_loads_once_across_recompositions() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_audio();
        let player = Arc::new(RecordingPlayer::default());
        let player_ref: AudioPlayerRef = player.clone();
        set_platform_audio(player_ref);

        let wav = tiny_wav();
        let bank_len = Rc::new(Cell::new(0usize));
        let bank_len_build = Rc::clone(&bank_len);
        let mut build = move || {
            let specs = [SoundSpec::new("a", &wav), SoundSpec::new("b", &wav)];
            let bank = rememberSoundBank(&specs);
            bank_len_build.set(bank.len());
        };

        let key = cranpose_core::location_key(file!(), line!(), column!());
        let mut composition = cranpose_core::Composition::new(cranpose_core::MemoryApplier::new());
        composition.render(key, &mut build).expect("first render");
        composition.render(key, &mut build).expect("second render");

        assert_eq!(bank_len.get(), 2);
        assert_eq!(
            *player.next.lock(),
            2,
            "the bank decodes once across renders"
        );
        clear_platform_audio();
    }
}
