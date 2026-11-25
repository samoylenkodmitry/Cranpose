//! Media playback: one item at a time, observable rather than polled.
//!
//! A media player is not a sound-effect engine. [`audio`](crate::audio) mixes
//! short decoded cues; this plays one long encoded item — a track, a podcast,
//! a recording — through whatever the platform already uses for media, and
//! answers the four questions every player screen asks:
//!
//! * **What is it doing?** [`PlaybackState`] is published, not polled. A screen
//!   that asks "is it playing yet?" every frame does that work whether or not
//!   anything changed, and learns about a failure only by noticing that the
//!   position stopped moving.
//! * **Where is it?** [`PlaybackProgress`] carries position, duration and how
//!   much is buffered, published by the backend as it moves. A seek bar reads
//!   [`playback_progress`] while it drags and collects
//!   [`rememberPlaybackProgress`] otherwise.
//! * **May it be heard?** Audio focus is a contract with the rest of the
//!   device, and every application gets it wrong in the same way: it ducks and
//!   forgets to un-duck, or it resumes after a phone call it never paused for.
//!   The policy lives here — see [`publish_audio_focus`] — so a backend only
//!   has to report what the platform told it.
//! * **What does the lock screen say?** [`MediaMetadata`] goes to the platform
//!   media session, and the buttons on it come back as [`MediaCommand`]s. The
//!   transport commands are carried out here; the ones that need a playlist are
//!   handed to the application, because the framework does not have one.
//!
//! Analysis samples are **optional and capability-gated**. A visualiser wants
//! the samples that are being heard; not every platform media stack will give
//! them up, so [`MediaCapabilities::analysis`] says whether this one does
//! instead of publishing silence that looks like a bug. When it does, samples
//! are latest-wins and bounded exactly like camera frames: a visualiser that
//! falls behind draws the sound that is playing now and counts what it missed.

use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use cranpose_core::{EventStream, State, rememberEventStream};
use parking_lot::Mutex;
use portable_atomic::AtomicU64;

mod observers;

use observers::{
    COMMAND_OBSERVERS, FOCUS_OBSERVERS, PROGRESS_OBSERVERS, SAMPLE_OBSERVERS, STATE_OBSERVERS,
};
pub use observers::{
    MediaObserver, observe_audio_focus, observe_media_commands, observe_media_metadata,
    observe_media_samples, observe_playback_progress, observe_playback_state,
};

use crate::{
    background::{BackgroundWorkLease, acquire_background_work},
    host::{LifecycleEvent, LifecycleState},
    registry::ServiceRegistry,
};

mod types;

pub use types::{
    AudioFocus, ClippingConfiguration, DUCKED_GAIN, EqualizerBand, EqualizerSettings, MediaArtwork,
    MediaCapabilities, MediaCommand, MediaError, MediaItem, MediaMetadata, MediaSamples,
    OCTAVE_BAND_CENTERS_HZ, PlaybackProgress, PlaybackState, SubtitleConfiguration,
    octave_equalizer_bands,
};

/// A platform media stack.
///
/// A backend opens items, drives the transport, and publishes what happens
/// through [`publish_playback_state`], [`publish_playback_progress`],
/// [`publish_audio_focus`], [`publish_media_command`] and
/// [`publish_media_samples`]. Nothing here is polled, and no method blocks for
/// the length of an item.
///
/// Applications call the free functions — [`open_media`], [`play_media`],
/// [`seek_media`] — rather than this trait: the free functions are where volume
/// is combined with the focus gain, where the background-work lease is held,
/// and where a seek is clamped to the item.
pub trait MediaPlayer: Send + Sync {
    /// What this backend can do. Read by screens to decide which controls
    /// exist at all.
    fn capabilities(&self) -> MediaCapabilities;

    /// Opens `item` and gets it ready to play, without playing it.
    ///
    /// Returns as soon as the request is accepted; the item's progress arrives
    /// as [`PlaybackState`], because opening a network item takes as long as
    /// the network does.
    fn prepare(&self, item: &MediaItem) -> Result<(), MediaError>;

    /// Starts, or resumes, the open item.
    fn play(&self) -> Result<(), MediaError>;

    /// Stops without giving up the position.
    fn pause(&self);

    /// Stops, closes the item and releases the output device.
    fn stop(&self);

    /// Moves the position within the open item.
    fn seek_to(&self, _position: Duration) -> Result<(), MediaError> {
        Err(MediaError::NotSeekable)
    }

    /// Sets the output gain, already combined with the audio-focus gain by
    /// [`set_media_volume`]. `0.0` is silent, `1.0` is the item as recorded.
    fn set_volume(&self, volume: f32);

    /// Sets stereo balance: -1 is left, 0 is centered, and 1 is right.
    /// The quieter channel is attenuated without mixing channels. Returns
    /// false when the active backend cannot apply balance.
    fn set_balance(&self, _balance: f32) -> bool {
        false
    }

    /// Sets the playback rate, `1.0` being as recorded. Returns `false` where
    /// the backend does not have one.
    fn set_speed(&self, _speed: f32) -> bool {
        false
    }

    /// Repeats the open item when it reaches its end.
    fn set_looping(&self, _looping: bool) {}

    /// Starts or stops publishing [`MediaSamples`]. Returns `false` where the
    /// backend cannot produce them, which is also what
    /// [`MediaCapabilities::analysis`] reports.
    fn set_analysis_enabled(&self, _enabled: bool) -> bool {
        false
    }

    /// Hands metadata to the platform media session. Called again whenever the
    /// application learns more about the open item, because tags are often
    /// parsed after playback has already started.
    fn set_session_metadata(&self, _metadata: &MediaMetadata) {}

    /// The equalizer bands this backend has, centre frequency and range.
    ///
    /// Empty where there is no equalizer, which is also what
    /// [`MediaCapabilities::equalizer`] reports. A backend states its real
    /// bands: a platform effect has the ones its implementation has, and a
    /// screen that wants a different layout maps onto these rather than being
    /// told a layout that is not there.
    fn equalizer_bands(&self) -> Vec<EqualizerBand> {
        Vec::new()
    }

    /// Applies an equalizer setting, already clamped to this backend's bands.
    fn set_equalizer(&self, _settings: &EqualizerSettings) {}

    /// The audio file extensions this backend can decode, lower case and
    /// without the dot.
    ///
    /// An application that picks tracks off a disk decides what to offer from
    /// this rather than from a list of its own. Which formats play is a
    /// property of the stack underneath — the platform's decoders on a phone,
    /// the ones compiled in on a desktop — and a list written next to the
    /// picker is a claim about a backend it never asks. It goes stale the
    /// moment the backend changes, and the failure is quiet: the tracks import
    /// and then refuse to play.
    ///
    /// Empty where the backend cannot say, which a caller should read as "no
    /// opinion, offer what you like" rather than as "nothing plays".
    fn audio_extensions(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// Reads how long `item` is without opening it for playback.
    ///
    /// A playlist shows the length of entries nobody has played yet, and the
    /// only thing that can answer is the stack that reads the container.
    /// `None` where this backend cannot tell, which is also what
    /// [`MediaCapabilities::probing`] reports; a screen leaves the duration
    /// blank rather than treating it as an error.
    fn probe_duration(&self, _item: &MediaItem) -> Option<Duration> {
        None
    }
}

/// Shared handle to the platform media player.
pub type MediaPlayerRef = Arc<dyn MediaPlayer>;

static PLATFORM_MEDIA: ServiceRegistry<dyn MediaPlayer> = ServiceRegistry::new();

/// Installs the platform media player, replacing any previous one.
pub fn set_platform_media_player(player: MediaPlayerRef) {
    PLATFORM_MEDIA.set(player);
}

/// Removes the platform media player and forgets everything it published.
pub fn clear_platform_media_player() {
    if let Some(player) = PLATFORM_MEDIA.get() {
        player.stop();
    }
    PLATFORM_MEDIA.clear();
    observers::clear();
    *STATE.lock() = PlaybackState::Idle;
    *PROGRESS.lock() = PlaybackProgress::default();
    *CURRENT_ITEM.lock() = None;
    *LATEST_SAMPLES.lock() = None;
    *FOCUS.lock() = AudioFocus::Gained;
    *VOLUME.lock() = 1.0;
    PAUSED_BY_FOCUS.store(false, Ordering::Release);
    DROPPED_SAMPLES.store(0, Ordering::Release);
    release_background_lease();
}

/// The installed media player, or `None` where this platform has none.
pub fn media_player() -> Option<MediaPlayerRef> {
    PLATFORM_MEDIA.get()
}

/// Whether this platform can play media at all.
pub fn media_playback_supported() -> bool {
    PLATFORM_MEDIA.get().is_some()
}

/// What the installed backend can do, or [`MediaCapabilities::default`] — every
/// capability absent — when there is none.
pub fn media_capabilities() -> MediaCapabilities {
    media_player().map_or_default(|player| player.capabilities())
}

static STATE: Mutex<PlaybackState> = Mutex::new(PlaybackState::Idle);
static PROGRESS: Mutex<PlaybackProgress> = Mutex::new(PlaybackProgress {
    position: Duration::ZERO,
    duration: None,
    buffered: Duration::ZERO,
});
static CURRENT_ITEM: Mutex<Option<MediaItem>> = Mutex::new(None);
static LATEST_SAMPLES: Mutex<Option<MediaSamples>> = Mutex::new(None);
static FOCUS: Mutex<AudioFocus> = Mutex::new(AudioFocus::Gained);
static VOLUME: Mutex<f32> = Mutex::new(1.0);
static EQUALIZER: Mutex<EqualizerSettings> = Mutex::new(EqualizerSettings {
    enabled: false,
    preamp_db: 0.0,
    gains_db: Vec::new(),
});
static PAUSED_BY_FOCUS: AtomicBool = AtomicBool::new(false);

static DROPPED_SAMPLES: AtomicU64 = AtomicU64::new(0);

/// What the player is doing.
pub fn playback_state() -> PlaybackState {
    STATE.lock().clone()
}

/// Where the open item is.
///
/// Read outside composition — while a seek bar is being dragged, or during
/// draw — so a moving position costs no recomposition.
pub fn playback_progress() -> PlaybackProgress {
    *PROGRESS.lock()
}

/// The open item, or `None` when nothing is.
pub fn current_media_item() -> Option<MediaItem> {
    CURRENT_ITEM.lock().clone()
}

/// The last block of samples, or `None` when analysis is off or nothing has
/// played yet. Read during draw, so a visualiser never draws a stale block.
pub fn latest_media_samples() -> Option<MediaSamples> {
    LATEST_SAMPLES.lock().clone()
}

/// How many sample blocks were produced while every observer was still busy.
pub fn dropped_media_samples() -> u64 {
    DROPPED_SAMPLES.load(Ordering::Acquire)
}

/// What the rest of the device is doing with the output.
pub fn audio_focus() -> AudioFocus {
    *FOCUS.lock()
}

/// The volume the application asked for, before the audio-focus gain.
pub fn media_volume() -> f32 {
    *VOLUME.lock()
}

/// Publishes what the player is doing.
///
/// This is also where the background-work lease is taken and given up: an app
/// that is playing has work the runtime must keep turning for even with its
/// surface gone, and an app that has stopped does not.
pub fn publish_playback_state(state: PlaybackState) {
    {
        let mut current = STATE.lock();
        if *current == state {
            return;
        }
        *current = state.clone();
    }
    if state.is_playing() {
        acquire_background_lease();
    } else {
        release_background_lease();
    }
    if !state.is_active() {
        *PROGRESS.lock() = PlaybackProgress::default();
        *LATEST_SAMPLES.lock() = None;
    }
    if matches!(state, PlaybackState::Idle) {
        *CURRENT_ITEM.lock() = None;
        DROPPED_SAMPLES.store(0, Ordering::Release);
    }
    for observer in STATE_OBSERVERS.snapshot() {
        observer(state.clone());
    }
}

/// Publishes where the open item is. Backends call this as the position moves,
/// which for a local file is a handful of times a second.
pub fn publish_playback_progress(progress: PlaybackProgress) {
    let progress = clamp_progress(progress);
    {
        let mut current = PROGRESS.lock();
        if *current == progress {
            return;
        }
        *current = progress;
    }
    for observer in PROGRESS_OBSERVERS.snapshot() {
        observer(progress);
    }
}

fn clamp_progress(mut progress: PlaybackProgress) -> PlaybackProgress {
    if let Some(duration) = progress.duration {
        progress.position = progress.position.min(duration);
        progress.buffered = progress.buffered.min(duration);
    }
    progress
}

/// Publishes a button pressed outside the application's own UI.
///
/// The transport commands are carried out here before observers are told, so an
/// application that only wants to advance its playlist has nothing to wire up:
/// it collects [`rememberMediaCommands`] and reacts to
/// [`MediaCommand::Next`] and [`MediaCommand::Previous`].
pub fn publish_media_command(command: MediaCommand) {
    match command {
        MediaCommand::Play => {
            let _ = play_media();
        }
        MediaCommand::Pause => pause_media(),
        MediaCommand::TogglePlayPause => toggle_media(),
        MediaCommand::Stop => stop_media(),
        MediaCommand::SeekTo(position) => {
            let _ = seek_media(position);
        }
        MediaCommand::Next | MediaCommand::Previous => {}
    }
    for observer in COMMAND_OBSERVERS.snapshot() {
        observer(command);
    }
}

/// Publishes what the rest of the device is doing with the output, and applies
/// the policy that goes with it.
///
/// The policy is the whole point of this living in the framework:
///
/// * [`Ducked`](AudioFocus::Ducked) lowers the gain to [`DUCKED_GAIN`] and
///   keeps playing; regaining focus puts the application's own volume back,
///   whatever it changed to in the meantime.
/// * [`LostTransient`](AudioFocus::LostTransient) pauses **and remembers that
///   it did**, so the next [`Gained`](AudioFocus::Gained) resumes — and a
///   [`Gained`](AudioFocus::Gained) that follows a user's own pause does not.
/// * [`Lost`](AudioFocus::Lost) stops and forgets, because focus lost for good
///   does not come back.
pub fn publish_audio_focus(focus: AudioFocus) {
    {
        let mut current = FOCUS.lock();
        if *current == focus {
            return;
        }
        *current = focus;
    }
    apply_volume();
    match focus {
        AudioFocus::Gained => {
            if PAUSED_BY_FOCUS.swap(false, Ordering::AcqRel) {
                let _ = play_media();
            }
        }
        AudioFocus::Ducked => {}
        AudioFocus::LostTransient => {
            if playback_state().is_playing() {
                PAUSED_BY_FOCUS.store(true, Ordering::Release);
                pause_media();
            }
        }
        AudioFocus::Lost => {
            PAUSED_BY_FOCUS.store(false, Ordering::Release);
            stop_media();
        }
    }
    for observer in FOCUS_OBSERVERS.snapshot() {
        observer(focus);
    }
}

/// Publishes a block of samples as it is heard.
///
/// The newest block always replaces the stored one, so a visualiser drawing
/// [`latest_media_samples`] never draws a stale one; observers that keep up see
/// every block, and blocks nobody could take are counted in
/// [`dropped_media_samples`] rather than queued behind.
pub fn publish_media_samples(samples: MediaSamples) {
    *LATEST_SAMPLES.lock() = Some(samples.clone());
    let observers = SAMPLE_OBSERVERS.snapshot();
    if observers.is_empty() {
        return;
    }
    for observer in observers {
        observer(samples.clone());
    }
}

/// Records that the backend produced a block nobody could take.
pub fn record_dropped_media_samples() {
    DROPPED_SAMPLES.fetch_add(1, Ordering::AcqRel);
}

/// Opens `item`, publishing [`PlaybackState::Loading`] before the backend is
/// asked so a screen shows the wait rather than a gap.
///
/// The item is not played: an application that wants it to start calls
/// [`play_media`] when the backend publishes [`PlaybackState::Paused`], or
/// simply calls it straight away — a backend queues the request against the
/// item it is opening.
pub fn open_media(item: MediaItem) -> Result<(), MediaError> {
    *LATEST_SAMPLES.lock() = None;
    let Some(player) = media_player() else {
        publish_playback_state(PlaybackState::Failed(MediaError::Unsupported));
        return Err(MediaError::Unsupported);
    };
    PAUSED_BY_FOCUS.store(false, Ordering::Release);
    DROPPED_SAMPLES.store(0, Ordering::Release);
    *CURRENT_ITEM.lock() = Some(item.clone());
    publish_playback_progress(PlaybackProgress {
        position: Duration::ZERO,
        duration: item.metadata.duration,
        buffered: Duration::ZERO,
    });
    publish_playback_state(PlaybackState::Loading);
    if player.capabilities().session {
        player.set_session_metadata(&item.metadata);
    }
    player.prepare(&item).inspect_err(|error| {
        publish_playback_state(PlaybackState::Failed(error.clone()));
    })
}

/// Starts, or resumes, the open item.
pub fn play_media() -> Result<(), MediaError> {
    let Some(player) = media_player() else {
        return Err(MediaError::Unsupported);
    };
    if CURRENT_ITEM.lock().is_none() {
        return Err(MediaError::NothingLoaded);
    }
    player.play().inspect_err(|error| {
        publish_playback_state(PlaybackState::Failed(error.clone()));
    })
}

/// Stops without giving up the position.
pub fn pause_media() {
    if let Some(player) = media_player() {
        player.pause();
    }
}

/// Stops, closes the item and releases the output device.
pub fn stop_media() {
    PAUSED_BY_FOCUS.store(false, Ordering::Release);
    if let Some(player) = media_player() {
        player.stop();
    }
    publish_playback_state(PlaybackState::Idle);
}

/// Pauses what is playing and plays what is paused — the one button a headset
/// has, and the space bar.
pub fn toggle_media() {
    if playback_state().is_playing() {
        pause_media();
    } else {
        let _ = play_media();
    }
}

/// Moves the position within the open item.
///
/// Clamped to the item's length here rather than in every backend, because a
/// seek past the end means different things to different platform stacks and
/// none of them mean what the seek bar meant.
pub fn seek_media(position: Duration) -> Result<(), MediaError> {
    let Some(player) = media_player() else {
        return Err(MediaError::Unsupported);
    };
    if CURRENT_ITEM.lock().is_none() {
        return Err(MediaError::NothingLoaded);
    }
    if !player.capabilities().seeking {
        return Err(MediaError::NotSeekable);
    }
    let position = match playback_progress().duration {
        Some(duration) => position.min(duration),
        None => position,
    };
    player.seek_to(position)
}

/// Moves the position to a fraction of the item, which is what a seek bar has.
///
/// Reports [`MediaError::NotSeekable`] for an item with no length, because a
/// fraction of an unknown length is not a position.
pub fn seek_media_fraction(fraction: f32) -> Result<(), MediaError> {
    let Some(duration) = playback_progress().duration else {
        return Err(MediaError::NotSeekable);
    };
    let fraction = fraction.clamp(0.0, 1.0) as f64;
    seek_media(Duration::from_secs_f64(duration.as_secs_f64() * fraction))
}

/// Sets the volume the application asks for, `1.0` being the item as recorded.
///
/// What reaches the device is this combined with the audio-focus gain, so an
/// application may set its volume freely while another app is being heard over
/// the top without undoing the duck.
pub fn set_media_volume(volume: f32) {
    *VOLUME.lock() = volume.clamp(0.0, 1.0);
    apply_volume();
}

/// Applies stereo balance to the installed player. Values outside -1..=1
/// are clamped; non-finite values are rejected without changing playback.
/// Returns false if the player cannot apply balance.
pub fn set_media_balance(balance: f32) -> bool {
    balance.is_finite()
        && media_player().is_some_and(|player| player.set_balance(balance.clamp(-1.0, 1.0)))
}

fn apply_volume() {
    let Some(player) = media_player() else {
        return;
    };
    let gain = match audio_focus() {
        AudioFocus::Ducked => DUCKED_GAIN,
        _ => 1.0,
    };
    player.set_volume(media_volume() * gain);
}

/// Sets the playback rate, `1.0` being as recorded. Returns `false` where the
/// backend has none — see [`MediaCapabilities::speed`].
pub fn set_media_speed(speed: f32) -> bool {
    match media_player() {
        Some(player) if player.capabilities().speed => player.set_speed(speed),
        _ => false,
    }
}

/// Repeats the open item when it reaches its end.
pub fn set_media_looping(looping: bool) {
    if let Some(player) = media_player() {
        player.set_looping(looping);
    }
}

/// Starts or stops publishing [`MediaSamples`]. Returns `false` where the
/// backend cannot produce them — see [`MediaCapabilities::analysis`].
///
/// Off by default: producing samples costs the platform work on every block,
/// and a screen with no visualiser on it should not pay for one.
pub fn set_media_analysis_enabled(enabled: bool) -> bool {
    match media_player() {
        Some(player) if player.capabilities().analysis => {
            if !enabled {
                *LATEST_SAMPLES.lock() = None;
            }
            player.set_analysis_enabled(enabled)
        }
        _ => {
            *LATEST_SAMPLES.lock() = None;
            false
        }
    }
}

/// Reads how long `item` is without playing it.
///
/// `None` where no backend is installed or the installed one cannot tell —
/// see [`MediaCapabilities::probing`].
pub fn probe_media_duration(item: &MediaItem) -> Option<Duration> {
    media_player()?.probe_duration(item)
}

/// The equalizer bands this platform has, in the order gains are given in.
///
/// Empty where there is no equalizer. A screen reads this to know how many
/// controls to draw and what to label them, rather than assuming a layout.
pub fn media_equalizer_bands() -> Vec<EqualizerBand> {
    match media_player() {
        Some(player) if player.capabilities().equalizer => player.equalizer_bands(),
        _ => Vec::new(),
    }
}

/// The audio file extensions the platform backend can decode, lower case and
/// without the dot.
///
/// Empty where there is no backend, or where the backend has no opinion. See
/// [`MediaPlayer::audio_extensions`].
pub fn media_audio_extensions() -> Vec<&'static str> {
    media_player().map_or_default(|player| player.audio_extensions())
}

/// The equalizer setting last applied.
pub fn media_equalizer() -> EqualizerSettings {
    EQUALIZER.lock().clone()
}

/// Applies an equalizer setting, clamped to what the platform's bands can do.
///
/// Returns `false` where there is no equalizer — see
/// [`MediaCapabilities::equalizer`]. The setting is remembered either way, so a
/// screen that stores a user's curve reads back what the user chose rather than
/// what a device happened to support.
pub fn set_media_equalizer(settings: EqualizerSettings) -> bool {
    *EQUALIZER.lock() = settings.clone();
    let Some(player) = media_player() else {
        return false;
    };
    if !player.capabilities().equalizer {
        return false;
    }
    player.set_equalizer(&settings.clamped_to(&player.equalizer_bands()));
    true
}

/// Updates the metadata shown by the platform media session for the open item.
///
/// Called when tags finish parsing, which is usually after playback started.
pub fn set_media_metadata(metadata: MediaMetadata) {
    match CURRENT_ITEM.lock().as_mut() {
        Some(item) => item.metadata = metadata.clone(),
        None => return,
    }
    if let Some(player) = media_player()
        && player.capabilities().session
    {
        player.set_session_metadata(&metadata);
    }
    observers::metadata_changed(&metadata);
}

static BACKGROUND_LEASE: Mutex<Option<BackgroundWorkLease>> = Mutex::new(None);

fn acquire_background_lease() {
    let mut lease = BACKGROUND_LEASE.lock();
    if lease.is_none() {
        *lease = Some(acquire_background_work("Playing"));
    }
}

fn release_background_lease() {
    BACKGROUND_LEASE.lock().take();
}

#[cfg(test)]
fn holds_background_work() -> bool {
    BACKGROUND_LEASE.lock().is_some()
}

pub(crate) fn on_lifecycle(event: LifecycleEvent) {
    if event.to == LifecycleState::Destroyed {
        stop_media();
    }
}

pub(crate) fn publish_player_session(
    item: Option<&MediaItem>,
    state: PlaybackState,
    progress: PlaybackProgress,
) {
    let mut current = CURRENT_ITEM.lock();
    let item_changed = current.as_ref() != item;
    if item_changed {
        *current = item.cloned();
    }
    drop(current);
    if item_changed && let Some(active) = item {
        if let Some(player) = media_player()
            && player.capabilities().session
        {
            player.set_session_metadata(&active.metadata);
        }
        observers::metadata_changed(&active.metadata);
    }
    publish_playback_progress(progress);
    publish_playback_state(state);
}

/// The `file:` URI for a path, which is what [`MediaItem`] takes.
///
/// Percent-encodes everything a URI reserves, so a track called `Sgt. Pepper's
/// #1.mp3` survives the trip. Lives here rather than in a backend because
/// every backend that reads local files needs the same answer, and an
/// application building an item needs it too.
pub fn uri_for_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    let mut uri = String::with_capacity(text.len() + 8);
    uri.push_str("file://");
    if !text.starts_with('/') {
        uri.push('/');
    }
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(byte as char);
            }
            b'\\' => uri.push('/'),
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

/// The path a media URI addresses, or `None` when it addresses something that
/// is not a local file — a stream, a content provider, a browser blob.
///
/// A bare path is accepted as itself: an application that already has a
/// `PathBuf` should not have to build a URI to hand it back.
pub fn path_from_uri(uri: &str) -> Option<PathBuf> {
    let rest = match uri.split_once("://") {
        Some(("file", rest)) => rest,
        Some(_) => return None,
        None => return crate::media_uri::bare_path(uri),
    };
    let path = rest.strip_prefix('/')?;
    let decoded = crate::content::percent_decode(path)?;
    if decoded.starts_with('/') || decoded.is_empty() {
        return non_empty_path(&decoded);
    }
    if decoded.as_bytes().get(1) == Some(&b':') {
        non_empty_path(&decoded)
    } else {
        non_empty_path(&format!("/{decoded}"))
    }
}

fn non_empty_path(text: &str) -> Option<PathBuf> {
    if text.is_empty() {
        return None;
    }
    Some(PathBuf::from(text))
}

pub use crate::media_source::{MediaSourceHandle, MediaSourceOpener, MediaSourceOpenerRef};

pub(crate) static PLATFORM_MEDIA_SOURCE: ServiceRegistry<dyn MediaSourceOpener> =
    ServiceRegistry::new();

/// Installs the platform media source opener, replacing any previous one.
pub fn set_platform_media_source_opener(opener: MediaSourceOpenerRef) {
    PLATFORM_MEDIA_SOURCE.set(opener);
}

/// Removes the platform media source opener.
pub fn clear_platform_media_source_opener() {
    PLATFORM_MEDIA_SOURCE.clear();
}

/// Opens `uri` for decoding.
///
/// `file:` URIs and bare paths are opened here. Anything else is the platform's
/// to answer, and a platform that registered no opener gets
/// [`ErrorKind::Unsupported`](std::io::ErrorKind::Unsupported) rather than a
/// guess.
pub fn open_media_source(uri: &str) -> std::io::Result<MediaSourceHandle> {
    if let Some(path) = path_from_uri(uri) {
        let stream = File::open(path)?;
        let len = stream.metadata().ok().map(|metadata| metadata.len());
        return Ok(MediaSourceHandle::new(stream, len));
    }
    match PLATFORM_MEDIA_SOURCE.get() {
        Some(opener) => opener.open(uri),
        None => Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("no platform opener for {uri}"),
        )),
    }
}

/// What the player is doing, observed for as long as this call stays in the
/// composition.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberPlaybackState() -> State<PlaybackState> {
    let updates = rememberEventStream((), |sender| {
        observe_playback_state(move |state| sender.send(state))
    });
    cranpose_core::collectAsState(updates, (), playback_state())
}

/// Where the open item is, observed for as long as this call stays in the
/// composition.
///
/// This recomposes as the position moves, which is what a seek bar and a time
/// label want. A visualiser or a waveform that redraws every frame anyway reads
/// [`playback_progress`] during draw instead.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberPlaybackProgress() -> State<PlaybackProgress> {
    let updates = rememberEventStream((), |sender| {
        observe_playback_progress(move |progress| sender.send(progress))
    });
    cranpose_core::collectAsState(updates, (), playback_progress())
}

/// What the rest of the device is doing with the output, observed for as long
/// as this call stays in the composition.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberAudioFocus() -> State<AudioFocus> {
    let updates = rememberEventStream((), |sender| {
        observe_audio_focus(move |focus| sender.send(focus))
    });
    cranpose_core::collectAsState(updates, (), audio_focus())
}

/// Buttons pressed outside the application's own UI, as a stream this
/// composition collects.
///
/// The transport commands have already been carried out by the time they arrive
/// here; what an application acts on is [`MediaCommand::Next`] and
/// [`MediaCommand::Previous`], which need the playlist it owns.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberMediaCommands() -> EventStream<MediaCommand> {
    rememberEventStream((), |sender| {
        observe_media_commands(move |command| sender.send(command))
    })
}

/// Samples as they are heard, as a stream this composition collects.
///
/// Enable them with [`set_media_analysis_enabled`] first; a backend that cannot
/// produce them says so through [`MediaCapabilities::analysis`].
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberMediaSamples() -> EventStream<MediaSamples> {
    rememberEventStream((), |sender| {
        observe_media_samples(move |samples| sender.send(samples))
    })
}

#[cfg(test)]
#[path = "tests/media_tests.rs"]
mod tests;
