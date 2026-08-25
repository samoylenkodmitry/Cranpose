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
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use cranpose_core::{rememberEventStream, EventStream, State};
use parking_lot::Mutex;

use crate::{
    background::{acquire_background_work, BackgroundWorkLease},
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

// -- Published state ---------------------------------------------------------

static STATE: Mutex<PlaybackState> = Mutex::new(PlaybackState::Idle);
static PROGRESS: Mutex<PlaybackProgress> = Mutex::new(PlaybackProgress {
    position: Duration::ZERO,
    duration: None,
    buffered: Duration::ZERO,
});
static CURRENT_ITEM: Mutex<Option<MediaItem>> = Mutex::new(None);
static LATEST_SAMPLES: Mutex<Option<MediaSamples>> = Mutex::new(None);
static FOCUS: Mutex<AudioFocus> = Mutex::new(AudioFocus::Gained);
/// The volume the application asked for, before the focus gain is applied.
static VOLUME: Mutex<f32> = Mutex::new(1.0);
/// The equalizer curve the application asked for. Kept whether or not a
/// platform can apply it, so a stored user setting survives a device that
/// cannot honour it and reaches one that can.
static EQUALIZER: Mutex<EqualizerSettings> = Mutex::new(EqualizerSettings {
    enabled: false,
    preamp_db: 0.0,
    gains_db: Vec::new(),
});
static PAUSED_BY_FOCUS: AtomicBool = AtomicBool::new(false);

/// Sample blocks produced while every observer was still busy with an earlier
/// one. Counted rather than queued, for the same reason camera frames are: a
/// visualiser that falls behind should draw the sound that is playing now.
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

// -- Observers ---------------------------------------------------------------

/// One registry of callbacks. Every published signal has the same shape, so it
/// is written once and instantiated per signal rather than copied per signal.
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

// -- Publishing (called by backends) -----------------------------------------

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

// -- Transport (called by applications) --------------------------------------

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
    if let Some(player) = media_player() {
        if player.capabilities().session {
            player.set_session_metadata(&metadata);
        }
    }
}

// -- Lifecycle ---------------------------------------------------------------

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

/// Whether playback is what is keeping the runtime turning.
///
/// The lease count is one number for the whole process — a durable save holds
/// leases too — so this asks about the one this service took rather than about
/// the total.
#[cfg(test)]
fn holds_background_work() -> bool {
    BACKGROUND_LEASE.lock().is_some()
}

/// Applies a host lifecycle transition to playback.
///
/// Backgrounding does **not** stop a media player: that is the difference
/// between a media player and every other service, and it is why playback holds
/// a background-work lease while it runs. A host being destroyed does stop it,
/// because the device it holds outlives the surface that was drawing.
pub(crate) fn on_lifecycle(event: LifecycleEvent) {
    if event.to == LifecycleState::Destroyed {
        stop_media();
    }
}

// -- Local-file URIs ---------------------------------------------------------

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
        // A Windows path (`C:\Music\x.mp3`) has no leading slash of its own,
        // and `file://C:/...` would read `C:` as a host.
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
    // `file:///path` has an empty authority; `file://host/path` names a host
    // no local backend can read.
    let path = rest.strip_prefix('/')?;
    let decoded = crate::content::percent_decode(path)?;
    if decoded.starts_with('/') || decoded.is_empty() {
        return non_empty_path(&decoded);
    }
    // A Windows path came through as `C:/Music/x.mp3`; anything else that has
    // lost its leading slash is put back where it was.
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

// -- Opening a URI for the in-process decoder --------------------------------

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

// -- Composables -------------------------------------------------------------

/// What the player is doing, observed for as long as this call stays in the
/// composition.
#[allow(non_snake_case)]
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
pub fn rememberPlaybackProgress() -> State<PlaybackProgress> {
    let updates = rememberEventStream((), |sender| {
        observe_playback_progress(move |progress| sender.send(progress))
    });
    cranpose_core::collectAsState(updates, (), playback_progress())
}

/// What the rest of the device is doing with the output, observed for as long
/// as this call stays in the composition.
#[allow(non_snake_case)]
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
pub fn rememberMediaSamples() -> EventStream<MediaSamples> {
    rememberEventStream((), |sender| {
        observe_media_samples(move |samples| sender.send(samples))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::test_service_guard;

    /// A backend that records what it was asked and publishes what a real one
    /// would.
    struct FakePlayer {
        capabilities: MediaCapabilities,
        calls: Mutex<Vec<String>>,
        volume: Mutex<f32>,
        prepare_fails: bool,
    }

    impl FakePlayer {
        fn new() -> Arc<FakePlayer> {
            Arc::new(FakePlayer {
                capabilities: MediaCapabilities {
                    seeking: true,
                    speed: true,
                    looping: true,
                    analysis: true,
                    session: true,
                    equalizer: true,
                    probing: true,
                },
                calls: Mutex::new(Vec::new()),
                volume: Mutex::new(1.0),
                prepare_fails: false,
            })
        }

        fn with(capabilities: MediaCapabilities) -> Arc<FakePlayer> {
            Arc::new(FakePlayer {
                capabilities,
                calls: Mutex::new(Vec::new()),
                volume: Mutex::new(1.0),
                prepare_fails: false,
            })
        }

        fn failing() -> Arc<FakePlayer> {
            Arc::new(FakePlayer {
                capabilities: MediaCapabilities::TRANSPORT,
                calls: Mutex::new(Vec::new()),
                volume: Mutex::new(1.0),
                prepare_fails: true,
            })
        }

        fn note(&self, call: impl Into<String>) {
            self.calls.lock().push(call.into());
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().clone()
        }
    }

    impl MediaPlayer for FakePlayer {
        fn capabilities(&self) -> MediaCapabilities {
            self.capabilities
        }

        fn prepare(&self, item: &MediaItem) -> Result<(), MediaError> {
            self.note(format!("prepare {}", item.uri));
            if self.prepare_fails {
                return Err(MediaError::UnsupportedSource(item.uri.clone()));
            }
            publish_playback_state(PlaybackState::Paused);
            Ok(())
        }

        fn play(&self) -> Result<(), MediaError> {
            self.note("play");
            publish_playback_state(PlaybackState::Playing);
            Ok(())
        }

        fn pause(&self) {
            self.note("pause");
            publish_playback_state(PlaybackState::Paused);
        }

        fn stop(&self) {
            self.note("stop");
        }

        fn seek_to(&self, position: Duration) -> Result<(), MediaError> {
            self.note(format!("seek {}", position.as_millis()));
            Ok(())
        }

        fn set_volume(&self, volume: f32) {
            *self.volume.lock() = volume;
        }

        fn set_speed(&self, speed: f32) -> bool {
            self.note(format!("speed {speed}"));
            true
        }

        fn set_looping(&self, looping: bool) {
            self.note(format!("looping {looping}"));
        }

        fn set_analysis_enabled(&self, enabled: bool) -> bool {
            self.note(format!("analysis {enabled}"));
            true
        }

        fn set_session_metadata(&self, metadata: &MediaMetadata) {
            self.note(format!("session {}", metadata.title));
        }

        fn equalizer_bands(&self) -> Vec<EqualizerBand> {
            // Three bands with a narrow range, so a test can tell a clamp from
            // a pass-through and a band count from a hard-coded ten.
            vec![
                EqualizerBand::new(60.0, 6.0),
                EqualizerBand::new(1_000.0, 6.0),
                EqualizerBand::new(10_000.0, 6.0),
            ]
        }

        fn set_equalizer(&self, settings: &EqualizerSettings) {
            self.note(format!(
                "equalizer {} preamp {} gains {:?}",
                settings.enabled, settings.preamp_db, settings.gains_db
            ));
        }
    }

    fn install() -> (crate::registry::TestServiceGuard, Arc<FakePlayer>) {
        let guard = test_service_guard();
        clear_platform_media_player();
        let player = FakePlayer::new();
        set_platform_media_player(player.clone());
        (guard, player)
    }

    fn track() -> MediaItem {
        MediaItem::new("file:///music/track.flac").with_metadata(
            MediaMetadata::titled("Track")
                .artist("Artist")
                .duration(Duration::from_secs(200)),
        )
    }

    #[test]
    fn metadata_carries_everything_a_lock_screen_shows() {
        let artwork = MediaArtwork {
            bytes: vec![1, 2, 3].into(),
            mime: "image/png".to_string(),
        };
        let metadata = MediaMetadata::titled("Song")
            .artist("Band")
            .album("Record")
            .duration(Duration::from_secs(210))
            .artwork(artwork.clone());

        assert_eq!(metadata.album, "Record");
        assert_eq!(metadata.artwork.as_ref(), Some(&artwork));
        assert!(!metadata.is_empty());
        // An album on its own is still something to show.
        assert!(!MediaMetadata::default().album("Record").is_empty());
        assert!(MediaMetadata::default().is_empty());
    }

    #[test]
    fn a_band_reports_what_it_can_actually_do() {
        let band = EqualizerBand::new(1_000.0, 6.0);
        assert_eq!(band.clamp_gain(0.0), 0.0);
        assert_eq!(band.clamp_gain(6.0), 6.0);
        assert_eq!(band.clamp_gain(7.5), 6.0);
        assert_eq!(band.clamp_gain(-7.5), -6.0);
    }

    #[test]
    fn an_equalizer_setting_is_clamped_to_the_bands_the_backend_has() {
        let bands = vec![
            EqualizerBand::new(60.0, 6.0),
            EqualizerBand::new(1_000.0, 6.0),
        ];
        let asked = EqualizerSettings {
            enabled: true,
            preamp_db: -3.0,
            // Too loud for these bands, and one entry too many.
            gains_db: vec![12.0, -12.0, 4.0],
        };
        let applied = asked.clamped_to(&bands);
        assert_eq!(applied.gains_db, vec![6.0, -6.0]);
        assert_eq!(applied.preamp_db, -3.0);
        assert!(applied.enabled);
    }

    #[test]
    fn a_setting_shorter_than_the_bands_leaves_the_rest_flat() {
        let bands = octave_equalizer_bands(12.0);
        let applied = EqualizerSettings {
            enabled: true,
            preamp_db: 0.0,
            gains_db: vec![3.0],
        }
        .clamped_to(&bands);
        assert_eq!(applied.gains_db.len(), bands.len());
        assert_eq!(applied.gains_db[0], 3.0);
        assert!(applied.gains_db[1..].iter().all(|gain| *gain == 0.0));
    }

    #[test]
    fn a_curve_reaches_the_backend_clamped_to_its_own_bands() {
        let (_guard, player) = install();

        assert_eq!(media_equalizer_bands().len(), 3);
        assert!(set_media_equalizer(EqualizerSettings {
            enabled: true,
            preamp_db: -2.0,
            gains_db: vec![9.0, 0.0, -9.0],
        }));

        assert!(
            player
                .calls()
                .iter()
                .any(|call| call == "equalizer true preamp -2 gains [6.0, 0.0, -6.0]"),
            "the backend was not given the clamped curve: {:?}",
            player.calls()
        );
    }

    #[test]
    fn a_curve_is_remembered_even_where_nothing_can_apply_it() {
        let _guard = test_service_guard();
        clear_platform_media_player();

        let asked = EqualizerSettings {
            enabled: true,
            preamp_db: -1.0,
            gains_db: vec![4.0, -4.0],
        };
        // No backend: the user's curve is still theirs, and reaches the next
        // device that can honour it.
        assert!(!set_media_equalizer(asked.clone()));
        assert_eq!(media_equalizer(), asked);
        assert!(media_equalizer_bands().is_empty());
    }

    #[test]
    fn a_backend_without_an_equalizer_says_so_rather_than_pretending() {
        let _guard = test_service_guard();
        clear_platform_media_player();
        set_platform_media_player(FakePlayer::with(MediaCapabilities {
            equalizer: false,
            ..MediaCapabilities::TRANSPORT
        }));

        assert!(media_equalizer_bands().is_empty());
        assert!(!set_media_equalizer(EqualizerSettings::flat(10)));
    }

    #[test]
    fn an_item_falls_back_to_its_file_name_for_a_title() {
        assert_eq!(
            MediaItem::new("file:///music/03 - Song.mp3").display_title(),
            "03 - Song.mp3"
        );
        assert_eq!(
            MediaItem::new("https://host/stream?token=1").display_title(),
            "stream"
        );
        assert_eq!(track().display_title(), "Track");
    }

    #[test]
    fn progress_reports_fractions_only_for_items_that_have_a_length() {
        let known = PlaybackProgress::new(Duration::from_secs(30), Duration::from_secs(120));
        assert_eq!(known.fraction(), Some(0.25));
        assert_eq!(known.buffered_fraction(), Some(1.0));

        let live = PlaybackProgress {
            position: Duration::from_secs(30),
            duration: None,
            buffered: Duration::from_secs(35),
        };
        assert_eq!(live.fraction(), None);
        assert_eq!(live.buffered_fraction(), None);
    }

    #[test]
    fn progress_never_reads_past_the_end_of_the_item() {
        let progress = PlaybackProgress::new(Duration::from_secs(500), Duration::from_secs(120));
        assert_eq!(progress.position, Duration::from_secs(120));
        assert_eq!(progress.fraction(), Some(1.0));
    }

    #[test]
    fn samples_reject_a_layout_that_does_not_describe_the_data() {
        assert!(MediaSamples::new(44_100, 2, 0, vec![0.0; 3]).is_none());
        assert!(MediaSamples::new(0, 2, 0, vec![0.0; 4]).is_none());
        assert!(MediaSamples::new(44_100, 0, 0, vec![0.0; 4]).is_none());

        let block = MediaSamples::new(44_100, 2, 7, vec![0.0; 4410]).expect("well-formed block");
        assert_eq!(block.frames(), 2205);
        assert_eq!(block.span(), Duration::from_millis(50));
        assert_eq!(block.sequence, 7);
    }

    #[test]
    fn without_a_backend_every_call_reports_that_it_is_unsupported() {
        let _guard = test_service_guard();
        clear_platform_media_player();

        assert!(!media_playback_supported());
        assert_eq!(media_capabilities(), MediaCapabilities::default());
        assert_eq!(open_media(track()), Err(MediaError::Unsupported));
        assert_eq!(
            playback_state(),
            PlaybackState::Failed(MediaError::Unsupported)
        );
        assert_eq!(play_media(), Err(MediaError::Unsupported));
        assert_eq!(seek_media(Duration::ZERO), Err(MediaError::Unsupported));
        assert!(!set_media_speed(2.0));
        assert!(!set_media_analysis_enabled(true));
    }

    #[test]
    fn opening_an_item_shows_the_wait_before_the_backend_is_asked() {
        let (_guard, player) = install();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        let _observer = observe_playback_state(move |state| recorder.lock().push(state));

        open_media(track()).expect("the fake backend opens anything");

        assert_eq!(
            *seen.lock(),
            vec![
                PlaybackState::Idle,
                PlaybackState::Loading,
                PlaybackState::Paused,
            ]
        );
        assert_eq!(
            player.calls(),
            vec!["session Track", "prepare file:///music/track.flac"]
        );
        assert_eq!(current_media_item().map(|item| item.uri), Some(track().uri));
    }

    #[test]
    fn an_item_that_cannot_be_opened_publishes_the_failure() {
        let _guard = test_service_guard();
        clear_platform_media_player();
        set_platform_media_player(FakePlayer::failing());

        let error = open_media(track()).expect_err("the failing backend refuses");
        assert_eq!(
            error,
            MediaError::UnsupportedSource("file:///music/track.flac".to_string())
        );
        assert_eq!(playback_state().failure(), Some(&error));
    }

    #[test]
    fn the_transport_routes_to_the_backend_and_publishes_what_it_did() {
        let (_guard, player) = install();

        open_media(track()).expect("opens");
        play_media().expect("plays");
        assert!(playback_state().is_playing());

        toggle_media();
        assert_eq!(playback_state(), PlaybackState::Paused);

        toggle_media();
        assert!(playback_state().is_playing());

        stop_media();
        assert_eq!(playback_state(), PlaybackState::Idle);
        assert_eq!(current_media_item(), None);

        assert_eq!(
            player.calls(),
            vec![
                "session Track",
                "prepare file:///music/track.flac",
                "play",
                "pause",
                "play",
                "stop",
            ]
        );
    }

    #[test]
    fn playing_nothing_reports_that_nothing_is_loaded() {
        let (_guard, _player) = install();

        assert_eq!(play_media(), Err(MediaError::NothingLoaded));
        assert_eq!(
            seek_media(Duration::from_secs(1)),
            Err(MediaError::NothingLoaded)
        );
    }

    #[test]
    fn a_seek_is_clamped_to_the_item_rather_than_to_each_backend() {
        let (_guard, player) = install();
        open_media(track()).expect("opens");

        seek_media(Duration::from_secs(1_000)).expect("seeks");

        assert!(player.calls().contains(&"seek 200000".to_string()));
    }

    #[test]
    fn a_seek_bar_fraction_maps_onto_the_item() {
        let (_guard, player) = install();
        open_media(track()).expect("opens");

        seek_media_fraction(0.25).expect("seeks");
        seek_media_fraction(3.0).expect("clamps rather than refusing");

        let calls = player.calls();
        assert!(calls.contains(&"seek 50000".to_string()));
        assert!(calls.contains(&"seek 200000".to_string()));
    }

    #[test]
    fn a_stream_with_no_length_has_no_seek_bar_fraction() {
        let (_guard, _player) = install();
        open_media(MediaItem::new("https://host/live")).expect("opens");

        assert_eq!(seek_media_fraction(0.5), Err(MediaError::NotSeekable));
    }

    #[test]
    fn a_backend_that_cannot_seek_says_so_instead_of_moving_nothing() {
        let _guard = test_service_guard();
        clear_platform_media_player();
        set_platform_media_player(FakePlayer::with(MediaCapabilities {
            seeking: false,
            ..MediaCapabilities::TRANSPORT
        }));

        open_media(track()).expect("opens");
        assert_eq!(
            seek_media(Duration::from_secs(1)),
            Err(MediaError::NotSeekable)
        );
    }

    #[test]
    fn what_reaches_the_device_is_the_volume_combined_with_the_focus_gain() {
        let (_guard, player) = install();
        open_media(track()).expect("opens");

        set_media_volume(0.5);
        assert_eq!(*player.volume.lock(), 0.5);
        assert_eq!(media_volume(), 0.5);

        publish_audio_focus(AudioFocus::Ducked);
        assert_eq!(*player.volume.lock(), 0.5 * DUCKED_GAIN);

        // The application may still change its own volume while ducked, and
        // doing so must not undo the duck.
        set_media_volume(1.0);
        assert_eq!(*player.volume.lock(), DUCKED_GAIN);

        publish_audio_focus(AudioFocus::Gained);
        assert_eq!(*player.volume.lock(), 1.0);
    }

    #[test]
    fn a_volume_outside_the_range_is_brought_back_into_it() {
        let (_guard, player) = install();

        set_media_volume(4.0);
        assert_eq!(media_volume(), 1.0);
        set_media_volume(-1.0);
        assert_eq!(media_volume(), 0.0);
        assert_eq!(*player.volume.lock(), 0.0);
    }

    #[test]
    fn a_transient_loss_pauses_and_the_next_gain_resumes() {
        let (_guard, _player) = install();
        open_media(track()).expect("opens");
        play_media().expect("plays");

        publish_audio_focus(AudioFocus::LostTransient);
        assert_eq!(playback_state(), PlaybackState::Paused);

        publish_audio_focus(AudioFocus::Gained);
        assert!(playback_state().is_playing());
    }

    #[test]
    fn regaining_focus_does_not_resume_what_the_user_paused() {
        let (_guard, _player) = install();
        open_media(track()).expect("opens");
        play_media().expect("plays");
        pause_media();

        publish_audio_focus(AudioFocus::LostTransient);
        publish_audio_focus(AudioFocus::Gained);

        assert_eq!(playback_state(), PlaybackState::Paused);
    }

    #[test]
    fn focus_lost_for_good_stops_and_does_not_come_back() {
        let (_guard, _player) = install();
        open_media(track()).expect("opens");
        play_media().expect("plays");

        publish_audio_focus(AudioFocus::Lost);
        assert_eq!(playback_state(), PlaybackState::Idle);

        publish_audio_focus(AudioFocus::Gained);
        assert_eq!(playback_state(), PlaybackState::Idle);
    }

    #[test]
    fn session_commands_drive_the_transport_and_still_reach_the_application() {
        let (_guard, player) = install();
        open_media(track()).expect("opens");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        let _observer = observe_media_commands(move |command| recorder.lock().push(command));

        publish_media_command(MediaCommand::Play);
        assert!(playback_state().is_playing());
        publish_media_command(MediaCommand::TogglePlayPause);
        assert_eq!(playback_state(), PlaybackState::Paused);
        publish_media_command(MediaCommand::SeekTo(Duration::from_secs(10)));
        publish_media_command(MediaCommand::Next);

        assert_eq!(
            *seen.lock(),
            vec![
                MediaCommand::Play,
                MediaCommand::TogglePlayPause,
                MediaCommand::SeekTo(Duration::from_secs(10)),
                MediaCommand::Next,
            ]
        );
        // `Next` needs a playlist the framework does not have, so it reached
        // the application without touching the transport.
        assert!(player.calls().contains(&"seek 10000".to_string()));
        assert_eq!(playback_state(), PlaybackState::Paused);
    }

    #[test]
    fn next_and_previous_are_the_commands_the_framework_leaves_alone() {
        assert!(MediaCommand::Play.is_transport());
        assert!(MediaCommand::SeekTo(Duration::ZERO).is_transport());
        assert!(!MediaCommand::Next.is_transport());
        assert!(!MediaCommand::Previous.is_transport());
    }

    #[test]
    fn analysis_is_off_until_it_is_asked_for_and_only_where_it_exists() {
        let (_guard, player) = install();
        assert!(set_media_analysis_enabled(true));
        assert!(player.calls().contains(&"analysis true".to_string()));

        clear_platform_media_player();
        set_platform_media_player(FakePlayer::with(MediaCapabilities::TRANSPORT));
        assert!(!set_media_analysis_enabled(true));
    }

    /// A backend states the formats it decodes, so a picker offers what will
    /// play rather than what some list next to it once claimed.
    ///
    /// CranAmp shipped the second: a hardcoded list left over from decoding in
    /// process, kept after the tracks were handed to the platform's decoder.
    /// It went on offering AIFF and CAF on a phone whose `MediaPlayer` reads
    /// neither, so they imported and then refused to play with nothing on
    /// screen to say why.
    #[test]
    fn the_backend_states_which_audio_formats_it_decodes() {
        let guard = test_service_guard();
        clear_platform_media_player();
        assert!(
            media_audio_extensions().is_empty(),
            "with no backend there is nothing to claim"
        );

        struct Narrow;
        impl MediaPlayer for Narrow {
            fn capabilities(&self) -> MediaCapabilities {
                MediaCapabilities::TRANSPORT
            }
            fn prepare(&self, _item: &MediaItem) -> Result<(), MediaError> {
                Ok(())
            }
            fn play(&self) -> Result<(), MediaError> {
                Ok(())
            }
            fn pause(&self) {}
            fn stop(&self) {}
            fn set_volume(&self, _volume: f32) {}
            fn audio_extensions(&self) -> Vec<&'static str> {
                vec!["mp3", "wav"]
            }
        }

        set_platform_media_player(Arc::new(Narrow));
        assert_eq!(media_audio_extensions(), vec!["mp3", "wav"]);

        clear_platform_media_player();
        set_platform_media_player(FakePlayer::with(MediaCapabilities::TRANSPORT));
        assert!(
            media_audio_extensions().is_empty(),
            "a backend with no opinion says so rather than guessing"
        );
        drop(guard);
    }

    #[test]
    fn the_newest_sample_block_replaces_the_stored_one() {
        let (_guard, _player) = install();
        let first = MediaSamples::new(48_000, 1, 1, vec![0.25; 8]).expect("block");
        let second = MediaSamples::new(48_000, 1, 2, vec![0.5; 8]).expect("block");

        publish_media_samples(first);
        publish_media_samples(second.clone());

        assert_eq!(latest_media_samples(), Some(second));
        record_dropped_media_samples();
        record_dropped_media_samples();
        assert_eq!(dropped_media_samples(), 2);
    }

    #[test]
    fn turning_analysis_off_forgets_the_last_block() {
        let (_guard, _player) = install();
        publish_media_samples(MediaSamples::new(48_000, 1, 1, vec![0.25; 8]).expect("block"));

        assert!(set_media_analysis_enabled(false));

        assert_eq!(latest_media_samples(), None);
    }

    #[test]
    fn observers_stop_being_called_once_they_are_dropped() {
        let (_guard, _player) = install();
        let seen = Arc::new(Mutex::new(0usize));
        let recorder = Arc::clone(&seen);
        let observer = observe_playback_progress(move |_| *recorder.lock() += 1);

        publish_playback_progress(PlaybackProgress::new(
            Duration::from_secs(1),
            Duration::from_secs(10),
        ));
        let delivered = *seen.lock();
        drop(observer);
        publish_playback_progress(PlaybackProgress::new(
            Duration::from_secs(2),
            Duration::from_secs(10),
        ));

        assert_eq!(*seen.lock(), delivered);
    }

    #[test]
    fn published_progress_never_reads_past_the_end() {
        let (_guard, _player) = install();
        publish_playback_progress(PlaybackProgress {
            position: Duration::from_secs(99),
            duration: Some(Duration::from_secs(10)),
            buffered: Duration::from_secs(99),
        });

        let progress = playback_progress();
        assert_eq!(progress.position, Duration::from_secs(10));
        assert_eq!(progress.buffered, Duration::from_secs(10));
    }

    #[test]
    fn playing_holds_the_runtime_awake_and_stopping_lets_it_sleep() {
        let (_guard, _player) = install();
        assert!(!holds_background_work());

        open_media(track()).expect("opens");
        assert!(
            !holds_background_work(),
            "an item that is open but not playing is not work the runtime must keep turning for"
        );

        play_media().expect("plays");
        assert!(holds_background_work());

        pause_media();
        assert!(!holds_background_work());

        play_media().expect("plays");
        assert!(holds_background_work());
        stop_media();
        assert!(!holds_background_work());
    }

    #[test]
    fn a_destroyed_host_stops_playback_but_a_backgrounded_one_does_not() {
        let (_guard, _player) = install();
        open_media(track()).expect("opens");
        play_media().expect("plays");

        on_lifecycle(LifecycleEvent {
            from: LifecycleState::Resumed,
            to: LifecycleState::Stopped,
        });
        assert!(playback_state().is_playing());

        on_lifecycle(LifecycleEvent {
            from: LifecycleState::Stopped,
            to: LifecycleState::Destroyed,
        });
        assert_eq!(playback_state(), PlaybackState::Idle);
    }

    #[test]
    fn metadata_learned_after_playback_started_reaches_the_session() {
        let (_guard, player) = install();
        open_media(MediaItem::new("file:///music/untagged.mp3")).expect("opens");

        set_media_metadata(MediaMetadata::titled("Late Tag").artist("Artist"));

        assert!(player.calls().contains(&"session Late Tag".to_string()));
        assert_eq!(
            current_media_item().map(|item| item.metadata.title),
            Some("Late Tag".to_string())
        );
    }

    #[test]
    fn metadata_with_nothing_in_it_is_metadata_a_lock_screen_can_skip() {
        assert!(MediaMetadata::default().is_empty());
        assert!(!MediaMetadata::titled("Track").is_empty());
    }

    /// Who answers for a URI: a path is opened here, and anything else is the
    /// platform's. One test rather than three, because the opener is a process
    /// -wide registry and three would race each other clearing it.
    #[test]
    fn a_path_opens_here_and_everything_else_is_the_platforms() {
        struct Opener;
        impl MediaSourceOpener for Opener {
            fn open(&self, uri: &str) -> std::io::Result<MediaSourceHandle> {
                let path = crate::test_scratch_dir("media-source-opener").join("document.bin");
                std::fs::write(&path, uri.as_bytes())?;
                // A streaming provider knows the length it listed even though
                // its descriptor cannot be stat-ed, which is the case this
                // handle exists to carry.
                Ok(MediaSourceHandle {
                    stream: File::open(path)?,
                    len: Some(uri.len() as u64),
                })
            }
        }
        fn read(handle: MediaSourceHandle) -> String {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut { handle.stream }, &mut text).expect("read");
            text
        }

        clear_platform_media_source_opener();
        let path = crate::test_scratch_dir("media-source").join("track.bin");
        std::fs::write(&path, b"bytes").expect("write the fixture");
        // A file is a file on every target that has a filesystem, so no
        // platform has to be asked about one.
        let file = open_media_source(&uri_for_path(&path)).expect("the file");
        assert_eq!(file.len, Some(5), "a real file states its length");
        assert_eq!(read(file), "bytes");
        // A document URI on a platform that registered nothing says so rather
        // than guessing at a path.
        let error = open_media_source("content://provider/document/7").expect_err("no opener");
        assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);

        set_platform_media_source_opener(Arc::new(Opener));
        let opened = open_media_source("content://provider/document/7").expect("the opener");
        clear_platform_media_source_opener();
        assert_eq!(opened.len, Some(29));
        assert_eq!(read(opened), "content://provider/document/7");
    }

    #[test]
    fn a_path_survives_the_round_trip_through_a_uri() {
        let path = PathBuf::from("/music/Sgt. Pepper's #1.mp3");
        let uri = uri_for_path(&path);

        assert_eq!(uri, "file:///music/Sgt.%20Pepper%27s%20%231.mp3");
        assert_eq!(path_from_uri(&uri), Some(path));
    }

    #[test]
    fn a_windows_path_keeps_its_drive_letter() {
        let uri = uri_for_path(Path::new("C:\\Music\\track.mp3"));

        assert_eq!(uri, "file:///C%3A/Music/track.mp3");
        assert_eq!(
            path_from_uri(&uri),
            Some(PathBuf::from("C:/Music/track.mp3"))
        );
    }

    #[test]
    fn a_bare_path_is_accepted_as_itself() {
        assert_eq!(
            path_from_uri("/music/track.mp3"),
            Some(PathBuf::from("/music/track.mp3"))
        );
    }

    #[test]
    fn anything_that_is_not_a_local_file_has_no_path() {
        assert_eq!(path_from_uri("https://host/stream.mp3"), None);
        assert_eq!(path_from_uri("content://media/audio/1"), None);
        assert_eq!(path_from_uri("blob:https://host/abc"), None);
        assert_eq!(path_from_uri("file://host/share/track.mp3"), None);
        assert_eq!(path_from_uri(""), None);
    }

    #[test]
    fn a_truncated_escape_is_not_guessed_at() {
        assert_eq!(path_from_uri("file:///music/track%2"), None);
        assert_eq!(path_from_uri("file:///music/track%zz.mp3"), None);
    }

    #[test]
    fn speed_and_looping_reach_a_backend_that_has_them() {
        let (_guard, player) = install();
        assert!(set_media_speed(1.5));
        set_media_looping(true);

        let calls = player.calls();
        assert!(calls.contains(&"speed 1.5".to_string()));
        assert!(calls.contains(&"looping true".to_string()));
    }
}
