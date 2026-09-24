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
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use cranpose_core::{EventStream, State, rememberEventStream};
use parking_lot::Mutex;

use crate::{
    background::{BackgroundWorkLease, acquire_background_work},
    host::{LifecycleEvent, LifecycleState},
    registry::ServiceRegistry,
};

/// The gain applied while another app is being heard over this one.
///
/// Ducking rather than pausing is what the platforms ask for on a transient
/// interruption that can share the output — a navigation prompt over music.
pub const DUCKED_GAIN: f32 = 0.2;

/// Artwork for the platform media session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaArtwork {
    /// The encoded image, in whatever the tag carried.
    pub bytes: Arc<[u8]>,
    /// The image's media type, `image/jpeg` and `image/png` being what tags
    /// actually contain.
    pub mime: String,
}

impl MediaArtwork {
    /// Artwork from encoded bytes.
    pub fn new(bytes: impl Into<Arc<[u8]>>, mime: impl Into<String>) -> MediaArtwork {
        MediaArtwork {
            bytes: bytes.into(),
            mime: mime.into(),
        }
    }
}

/// What the platform media session shows: the lock screen, the notification,
/// the car head unit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaMetadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// The item's length when it is known before playback starts — from a tag,
    /// or from a previous play. `None` means "ask the backend once it has
    /// opened the item", which is what [`PlaybackProgress::duration`] reports.
    pub duration: Option<Duration>,
    pub artwork: Option<MediaArtwork>,
}

impl MediaMetadata {
    /// Metadata carrying only a title, which is what a bare file name gives.
    pub fn titled(title: impl Into<String>) -> MediaMetadata {
        MediaMetadata {
            title: title.into(),
            ..MediaMetadata::default()
        }
    }

    /// Sets the performer.
    pub fn artist(mut self, artist: impl Into<String>) -> MediaMetadata {
        self.artist = artist.into();
        self
    }

    /// Sets the album.
    pub fn album(mut self, album: impl Into<String>) -> MediaMetadata {
        self.album = album.into();
        self
    }

    /// Sets the length known ahead of playback.
    pub fn duration(mut self, duration: Duration) -> MediaMetadata {
        self.duration = Some(duration);
        self
    }

    /// Sets the artwork.
    pub fn artwork(mut self, artwork: MediaArtwork) -> MediaMetadata {
        self.artwork = Some(artwork);
        self
    }

    /// Whether there is anything worth showing on a lock screen.
    pub fn is_empty(&self) -> bool {
        self.title.is_empty() && self.artist.is_empty() && self.album.is_empty()
    }
}

/// One playable item.
///
/// The source is a URI because that is the one form every platform media stack
/// takes: `file:` and `content:` on Android, `file:` on desktop and iOS,
/// `blob:` for a file the browser handed over, `http:` and `https:` everywhere.
/// Handing the platform a URI is also what keeps a streamed item streaming
/// instead of being read into memory first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaItem {
    pub uri: String,
    pub metadata: MediaMetadata,
}

impl MediaItem {
    /// An item at `uri`, with no metadata yet.
    pub fn new(uri: impl Into<String>) -> MediaItem {
        MediaItem {
            uri: uri.into(),
            metadata: MediaMetadata::default(),
        }
    }

    /// The same item with metadata attached.
    pub fn with_metadata(mut self, metadata: MediaMetadata) -> MediaItem {
        self.metadata = metadata;
        self
    }

    /// The title, falling back to the last path segment of the URI so a screen
    /// always has something to show.
    pub fn display_title(&self) -> &str {
        if !self.metadata.title.is_empty() {
            return &self.metadata.title;
        }
        let path = self.uri.split(['?', '#']).next().unwrap_or(&self.uri);
        match path.rsplit(['/', '\\']).next() {
            Some(name) if !name.is_empty() => name,
            _ => &self.uri,
        }
    }
}

/// What a platform media stack can actually do.
///
/// Reported rather than assumed: a screen greys out a speed control the device
/// will not honour instead of offering one that silently does nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MediaCapabilities {
    /// Whether [`seek_media`] moves the position.
    pub seeking: bool,
    /// Whether [`set_media_speed`] changes the rate.
    pub speed: bool,
    /// Whether [`set_media_looping`] repeats the item when it ends.
    pub looping: bool,
    /// Whether the backend can publish [`MediaSamples`] while it plays.
    pub analysis: bool,
    /// Whether metadata reaches a platform media session — the lock screen,
    /// the notification, the headset buttons.
    pub session: bool,
    /// Whether the backend has an equalizer. The bands it has are reported by
    /// [`media_equalizer_bands`], because a platform effect has the bands its
    /// implementation has rather than the ones a screen would like.
    pub equalizer: bool,
    /// Whether [`probe_media_duration`] can read an item's length without
    /// playing it. A playlist that shows durations for entries nobody has
    /// opened needs this; one that does not, does not.
    pub probing: bool,
}

impl MediaCapabilities {
    /// A backend that plays, pauses and seeks and does nothing else, which is
    /// the floor for anything worth calling a media player.
    pub const TRANSPORT: MediaCapabilities = MediaCapabilities {
        seeking: true,
        speed: false,
        looping: true,
        analysis: false,
        session: false,
        equalizer: false,
        probing: false,
    };
}

/// One frequency band of a backend's equalizer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EqualizerBand {
    /// The frequency the band is centred on, in hertz.
    pub center_hz: f32,
    /// The most this band can cut, in decibels — a negative number.
    pub min_gain_db: f32,
    /// The most this band can lift, in decibels.
    pub max_gain_db: f32,
}

impl EqualizerBand {
    /// A band centred on `center_hz` with a symmetric range.
    pub fn new(center_hz: f32, range_db: f32) -> EqualizerBand {
        let range = range_db.abs();
        EqualizerBand {
            center_hz,
            min_gain_db: -range,
            max_gain_db: range,
        }
    }

    /// Brings `gain_db` inside what this band can actually do.
    pub fn clamp_gain(&self, gain_db: f32) -> f32 {
        gain_db.clamp(self.min_gain_db, self.max_gain_db)
    }
}

/// The octave centres a graphic equalizer is built on, in hertz.
///
/// The set a hardware graphic equalizer has had since long before software
/// ones. A backend that builds its own filters — the desktop one, the browser
/// one — reports these, so the same curve means the same thing on both. A
/// platform effect reports whatever bands its implementation has instead.
pub const OCTAVE_BAND_CENTERS_HZ: [f32; 10] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
];

/// [`OCTAVE_BAND_CENTERS_HZ`] as bands, each able to lift or cut by `range_db`.
pub fn octave_equalizer_bands(range_db: f32) -> Vec<EqualizerBand> {
    OCTAVE_BAND_CENTERS_HZ
        .iter()
        .map(|center| EqualizerBand::new(*center, range_db))
        .collect()
}

/// What an equalizer is set to.
///
/// `gains_db` is read alongside the bands [`media_equalizer_bands`] reported:
/// entry `n` is band `n`. A shorter list leaves the remaining bands flat, and a
/// longer one is truncated, so a screen built for one band layout still says
/// something sensible on a device with another.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EqualizerSettings {
    /// Whether the equalizer is in circuit at all. A flat, disabled equalizer
    /// is not the same as a flat, enabled one: the disabled one costs nothing.
    pub enabled: bool,
    /// Gain applied ahead of the bands, in decibels.
    pub preamp_db: f32,
    /// Per-band gain in decibels, in the order the bands were reported.
    pub gains_db: Vec<f32>,
}

impl EqualizerSettings {
    /// An enabled equalizer with every band flat.
    pub fn flat(bands: usize) -> EqualizerSettings {
        EqualizerSettings {
            enabled: true,
            preamp_db: 0.0,
            gains_db: vec![0.0; bands],
        }
    }

    /// This setting with every gain brought inside what `bands` can do, and
    /// its length matched to theirs.
    pub fn clamped_to(&self, bands: &[EqualizerBand]) -> EqualizerSettings {
        EqualizerSettings {
            enabled: self.enabled,
            preamp_db: self.preamp_db,
            gains_db: bands
                .iter()
                .enumerate()
                .map(|(index, band)| {
                    band.clamp_gain(self.gains_db.get(index).copied().unwrap_or(0.0))
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum MediaError {
    /// No media backend on this platform.
    #[error("media playback is not supported here")]
    Unsupported,
    /// The backend cannot open this URI — an unknown scheme, a codec it has
    /// no decoder for, a file that is not there.
    #[error("cannot play {0}")]
    UnsupportedSource(String),
    /// A transport call arrived before anything was opened.
    #[error("no media item is loaded")]
    NothingLoaded,
    /// The backend has no seek for this item — a live stream, or a container
    /// without an index.
    #[error("this item cannot be seeked")]
    NotSeekable,
    /// Any other failure the platform reported.
    #[error("{0}")]
    Failed(String),
}

/// What the player is doing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PlaybackState {
    /// Nothing is open.
    #[default]
    Idle,
    /// An item is opening or refilling its buffer. A separate state rather
    /// than a gap, because opening a network item takes long enough that a
    /// screen has to say so.
    Loading,
    /// Sound is coming out.
    Playing,
    /// An item is open and positioned, and stopped.
    Paused,
    /// The item played to its end. Distinct from [`Paused`](Self::Paused):
    /// this is what advances a playlist.
    Ended,
    /// The item could not be played, or playback ended in a failure.
    Failed(MediaError),
}

impl PlaybackState {
    /// Whether sound is coming out now.
    pub fn is_playing(&self) -> bool {
        matches!(self, PlaybackState::Playing)
    }

    /// Whether an item is open — playing, paused, or still opening.
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            PlaybackState::Loading | PlaybackState::Playing | PlaybackState::Paused
        )
    }

    /// The failure, if playback ended in one.
    pub fn failure(&self) -> Option<&MediaError> {
        match self {
            PlaybackState::Failed(error) => Some(error),
            _ => None,
        }
    }
}

/// Where the open item is.
#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub struct PlaybackProgress {
    /// How far in the item playback has reached.
    pub position: Duration,
    /// The item's length, or `None` for a stream that has none.
    pub duration: Option<Duration>,
    /// How far ahead of the position the buffer reaches. Equal to `duration`
    /// for a local file, which is what makes a local file's buffer bar full.
    pub buffered: Duration,
}

impl PlaybackProgress {
    /// Progress through an item of known length.
    pub fn new(position: Duration, duration: Duration) -> PlaybackProgress {
        PlaybackProgress {
            position: position.min(duration),
            duration: Some(duration),
            buffered: duration,
        }
    }

    /// How far through the item this is, or `None` when it has no length.
    pub fn fraction(&self) -> Option<f32> {
        let duration = self.duration?;
        if duration.is_zero() {
            return Some(0.0);
        }
        Some((self.position.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0))
    }

    /// How much of the item is buffered, or `None` when it has no length.
    pub fn buffered_fraction(&self) -> Option<f32> {
        let duration = self.duration?;
        if duration.is_zero() {
            return Some(0.0);
        }
        Some((self.buffered.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0))
    }
}

/// A button pressed somewhere the application does not draw: a lock screen, a
/// notification, a headset, a car.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaCommand {
    Play,
    Pause,
    /// The one button a headset has.
    TogglePlayPause,
    Stop,
    /// Needs a playlist, so it is reported and not carried out.
    Next,
    /// Needs a playlist, so it is reported and not carried out.
    Previous,
    SeekTo(Duration),
}

impl MediaCommand {
    /// Whether this command is one the framework carries out itself.
    ///
    /// The transport is player state and lives here. [`Next`](Self::Next) and
    /// [`Previous`](Self::Previous) need an order the framework does not have,
    /// so they are only reported.
    pub fn is_transport(self) -> bool {
        !matches!(self, MediaCommand::Next | MediaCommand::Previous)
    }
}

/// What the rest of the device is doing with the output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioFocus {
    /// This app may be heard at its own volume.
    #[default]
    Gained,
    /// Something short is being said over the top — a navigation prompt.
    /// Playback continues at [`DUCKED_GAIN`].
    Ducked,
    /// Something else has the output for a moment — a call, another player.
    /// Playback pauses and resumes on the next [`Gained`](Self::Gained).
    LostTransient,
    /// Something else has the output for good. Playback stops and does not
    /// come back on its own.
    Lost,
}

/// Samples as they are being heard, for a visualiser.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaSamples {
    /// Samples per second per channel.
    pub sample_rate: u32,
    /// How many channels are interleaved in [`samples`](Self::samples).
    pub channels: u16,
    /// Interleaved samples, nominally in `[-1, 1]`.
    pub samples: Arc<[f32]>,
    /// Which block this is, so a visualiser can tell a repeat from a new one
    /// and count what it missed.
    pub sequence: u64,
}

impl MediaSamples {
    /// A block of samples, or `None` when the layout does not describe the
    /// data — which reads as a broken visualiser rather than as an error if it
    /// is let through.
    pub fn new(
        sample_rate: u32,
        channels: u16,
        sequence: u64,
        samples: impl Into<Arc<[f32]>>,
    ) -> Option<MediaSamples> {
        let samples = samples.into();
        if sample_rate == 0 || channels == 0 || samples.len() % channels as usize != 0 {
            return None;
        }
        Some(MediaSamples {
            sample_rate,
            channels,
            samples,
            sequence,
        })
    }

    /// How many samples there are per channel.
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    /// How long this block lasts.
    pub fn span(&self) -> Duration {
        if self.sample_rate == 0 {
            return Duration::ZERO;
        }
        Duration::from_secs_f64(self.frames() as f64 / self.sample_rate as f64)
    }
}

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
    STATE_OBSERVERS.clear();
    PROGRESS_OBSERVERS.clear();
    COMMAND_OBSERVERS.clear();
    FOCUS_OBSERVERS.clear();
    SAMPLE_OBSERVERS.clear();
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
    media_player()
        .map(|player| player.capabilities())
        .unwrap_or_default()
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

struct ObserverList<T: ?Sized> {
    entries: Mutex<Vec<(u64, Arc<T>)>>,
}

impl<T: ?Sized> ObserverList<T> {
    const fn new() -> ObserverList<T> {
        ObserverList {
            entries: Mutex::new(Vec::new()),
        }
    }

    fn add(&self, observer: Arc<T>) -> u64 {
        let id = NEXT_OBSERVER.fetch_add(1, Ordering::Relaxed);
        self.entries.lock().push((id, observer));
        id
    }

    fn remove(&self, id: u64) {
        self.entries.lock().retain(|(entry, _)| *entry != id);
    }

    fn snapshot(&self) -> Vec<Arc<T>> {
        self.entries
            .lock()
            .iter()
            .map(|(_, observer)| Arc::clone(observer))
            .collect()
    }

    fn clear(&self) {
        self.entries.lock().clear();
    }
}

static NEXT_OBSERVER: AtomicU64 = AtomicU64::new(1);

type StateObserverFn = dyn Fn(PlaybackState) + Send + Sync;
type ProgressObserverFn = dyn Fn(PlaybackProgress) + Send + Sync;
type CommandObserverFn = dyn Fn(MediaCommand) + Send + Sync;
type FocusObserverFn = dyn Fn(AudioFocus) + Send + Sync;
type SampleObserverFn = dyn Fn(MediaSamples) + Send + Sync;

static STATE_OBSERVERS: ObserverList<StateObserverFn> = ObserverList::new();
static PROGRESS_OBSERVERS: ObserverList<ProgressObserverFn> = ObserverList::new();
static COMMAND_OBSERVERS: ObserverList<CommandObserverFn> = ObserverList::new();
static FOCUS_OBSERVERS: ObserverList<FocusObserverFn> = ObserverList::new();
static SAMPLE_OBSERVERS: ObserverList<SampleObserverFn> = ObserverList::new();

/// Keeps a media observer registered until it is dropped.
pub struct MediaObserver {
    id: u64,
    remove: fn(u64),
}

impl Drop for MediaObserver {
    fn drop(&mut self) {
        (self.remove)(self.id);
    }
}

/// Registers `observer` for playback state. The current state is delivered at
/// once, so a screen composed mid-item shows what is happening rather than
/// waiting for the next change.
pub fn observe_playback_state(
    observer: impl Fn(PlaybackState) + Send + Sync + 'static,
) -> MediaObserver {
    let observer: Arc<StateObserverFn> = Arc::new(observer);
    let id = STATE_OBSERVERS.add(Arc::clone(&observer));
    observer(playback_state());
    MediaObserver {
        id,
        remove: |id| STATE_OBSERVERS.remove(id),
    }
}

/// Registers `observer` for position updates. The current position is
/// delivered at once.
pub fn observe_playback_progress(
    observer: impl Fn(PlaybackProgress) + Send + Sync + 'static,
) -> MediaObserver {
    let observer: Arc<ProgressObserverFn> = Arc::new(observer);
    let id = PROGRESS_OBSERVERS.add(Arc::clone(&observer));
    observer(playback_progress());
    MediaObserver {
        id,
        remove: |id| PROGRESS_OBSERVERS.remove(id),
    }
}

/// Registers `observer` for media-session commands.
pub fn observe_media_commands(
    observer: impl Fn(MediaCommand) + Send + Sync + 'static,
) -> MediaObserver {
    let id = COMMAND_OBSERVERS.add(Arc::new(observer));
    MediaObserver {
        id,
        remove: |id| COMMAND_OBSERVERS.remove(id),
    }
}

/// Registers `observer` for audio-focus changes. The current focus is
/// delivered at once.
pub fn observe_audio_focus(observer: impl Fn(AudioFocus) + Send + Sync + 'static) -> MediaObserver {
    let observer: Arc<FocusObserverFn> = Arc::new(observer);
    let id = FOCUS_OBSERVERS.add(Arc::clone(&observer));
    observer(audio_focus());
    MediaObserver {
        id,
        remove: |id| FOCUS_OBSERVERS.remove(id),
    }
}

/// Registers `observer` for analysis samples.
pub fn observe_media_samples(
    observer: impl Fn(MediaSamples) + Send + Sync + 'static,
) -> MediaObserver {
    let id = SAMPLE_OBSERVERS.add(Arc::new(observer));
    MediaObserver {
        id,
        remove: |id| SAMPLE_OBSERVERS.remove(id),
    }
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
        _ => false,
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
    media_player()
        .map(|player| player.audio_extensions())
        .unwrap_or_default()
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
    {
        let mut item = CURRENT_ITEM.lock();
        let Some(item) = item.as_mut() else {
            return;
        };
        item.metadata = metadata.clone();
    }
    if let Some(player) = media_player()
        && player.capabilities().session
    {
        player.set_session_metadata(&metadata);
    }
}

static BACKGROUND_LEASE: Mutex<Option<BackgroundWorkLease>> = Mutex::new(None);

fn acquire_background_lease() {
    let mut lease = BACKGROUND_LEASE.lock();
    if lease.is_none() {
        *lease = Some(acquire_background_work());
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
        None => return non_empty_path(uri),
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

/// A media stream the platform opened, and what it knows about it.
#[derive(Debug)]
pub struct MediaSourceHandle {
    /// The descriptor to read. Seekable for a real file; a pipe for a provider
    /// that streams.
    pub stream: File,
    /// How long the whole thing is, when the platform knows.
    ///
    /// A provider that streams cannot answer `stat` — that is what makes its
    /// descriptor a pipe — but it listed a size for the document all the same,
    /// and a decoder that knows the length can seek by it instead of waiting
    /// for the stream to end to find out where the end is.
    pub len: Option<u64>,
}

/// Opens a media URI the decoder cannot open for itself.
///
/// A `file:` URI is a path and needs nobody, which is why
/// [`open_media_source`] answers those without asking. A `content://` document
/// belongs to an Android provider and only the platform layer can ask that
/// provider for a descriptor, so the platform layer registers this and the
/// decode thread calls it.
///
/// A descriptor rather than a reader because that is what both sides really
/// have: a real file is seekable, a provider that streams hands back a pipe,
/// and the decoder tells them apart by trying to seek.
pub trait MediaSourceOpener: Send + Sync {
    /// Opens `uri` for reading.
    fn open(&self, uri: &str) -> std::io::Result<MediaSourceHandle>;
}

/// Shared handle to the platform media source opener.
pub type MediaSourceOpenerRef = Arc<dyn MediaSourceOpener>;

static PLATFORM_MEDIA_SOURCE: ServiceRegistry<dyn MediaSourceOpener> = ServiceRegistry::new();

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
        return Ok(MediaSourceHandle { stream, len });
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
#[allow(non_snake_case)]
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
#[allow(non_snake_case)]
#[track_caller]
pub fn rememberPlaybackProgress() -> State<PlaybackProgress> {
    let updates = rememberEventStream((), |sender| {
        observe_playback_progress(move |progress| sender.send(progress))
    });
    cranpose_core::collectAsState(updates, (), playback_progress())
}

/// What the rest of the device is doing with the output, observed for as long
/// as this call stays in the composition.
#[allow(non_snake_case)]
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
#[allow(non_snake_case)]
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
#[allow(non_snake_case)]
#[track_caller]
pub fn rememberMediaSamples() -> EventStream<MediaSamples> {
    rememberEventStream((), |sender| {
        observe_media_samples(move |samples| sender.send(samples))
    })
}

#[cfg(test)]
#[path = "tests/media_tests.rs"]
mod tests;
