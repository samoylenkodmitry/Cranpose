//! Data types for media items, capabilities, playback state, equalizer settings, and samples.

use std::{sync::Arc, time::Duration};

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
    pub fn new(bytes: impl Into<Arc<[u8]>>, mime: impl Into<String>) -> Self {
        Self {
            bytes: bytes.into(),
            mime: mime.into(),
        }
    }
}

/// What the platform media session shows: the lock screen, the notification,
/// the car head unit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaMetadata {
    /// Title of the item, or empty for none.
    pub title: String,
    /// Artist or performer name, or empty for none.
    pub artist: String,
    /// Album name, or empty for none.
    pub album: String,
    /// The item's length when it is known before playback starts — from a tag,
    /// or from a previous play. `None` means "ask the backend once it has
    /// opened the item", which is what [`PlaybackProgress::duration`] reports.
    pub duration: Option<Duration>,
    /// Cover image of the item, or `None` for none.
    pub artwork: Option<MediaArtwork>,
}

impl MediaMetadata {
    /// Metadata carrying only a title, which is what a bare file name gives.
    pub fn titled(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// Sets the performer.
    pub fn artist(mut self, artist: impl Into<String>) -> Self {
        self.artist = artist.into();
        self
    }

    /// Sets the album.
    pub fn album(mut self, album: impl Into<String>) -> Self {
        self.album = album.into();
        self
    }

    /// Sets the length known ahead of playback.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Sets the artwork.
    pub fn artwork(mut self, artwork: MediaArtwork) -> Self {
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
    /// The source of the item, such as `file:`, `content:`, `blob:`, `http:` or `https:`.
    pub uri: String,
    /// The title, artist and artwork that the platform media session shows for the item.
    pub metadata: MediaMetadata,
}

impl MediaItem {
    /// An item at `uri`, with no metadata yet.
    pub fn new(uri: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            metadata: MediaMetadata::default(),
        }
    }

    /// The same item with metadata attached.
    pub fn with_metadata(mut self, metadata: MediaMetadata) -> Self {
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
    /// Whether [`seek_media`](super::seek_media) moves the position.
    pub seeking: bool,
    /// Whether [`set_media_speed`](super::set_media_speed) changes the rate.
    pub speed: bool,
    /// Whether [`set_media_looping`](super::set_media_looping) repeats the item when it ends.
    pub looping: bool,
    /// Whether the backend can publish [`MediaSamples`] while it plays.
    pub analysis: bool,
    /// Whether metadata reaches a platform media session — the lock screen,
    /// the notification, the headset buttons.
    pub session: bool,
    /// Whether the backend has an equalizer. The bands it has are reported by
    /// [`media_equalizer_bands`](super::media_equalizer_bands), because a platform effect has the bands its
    /// implementation has rather than the ones a screen would like.
    pub equalizer: bool,
    /// Whether [`probe_media_duration`](super::probe_media_duration) can read an item's length without
    /// playing it. A playlist that shows durations for entries nobody has
    /// opened needs this; one that does not, does not.
    pub probing: bool,
    /// Whether this media player backend renders video frames in addition to audio.
    pub video: bool,
}

impl MediaCapabilities {
    /// A backend that plays, pauses and seeks and does nothing else, which is
    /// the floor for anything worth calling a media player.
    pub const TRANSPORT: Self = Self {
        seeking: true,
        speed: false,
        looping: true,
        analysis: false,
        session: false,
        equalizer: false,
        probing: false,
        video: false,
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
    pub fn new(center_hz: f32, range_db: f32) -> Self {
        let range = range_db.abs();
        Self {
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
/// `gains_db` is read alongside the bands [`media_equalizer_bands`](super::media_equalizer_bands) reported:
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
    pub fn flat(bands: usize) -> Self {
        Self {
            enabled: true,
            preamp_db: 0.0,
            gains_db: vec![0.0; bands],
        }
    }

    /// This setting with every gain brought inside what `bands` can do, and
    /// its length matched to theirs.
    pub fn clamped_to(&self, bands: &[EqualizerBand]) -> Self {
        Self {
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

/// Why a media call failed.
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
        matches!(self, Self::Playing)
    }

    /// Whether an item is open — playing, paused, or still opening.
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Loading | Self::Playing | Self::Paused)
    }

    /// The failure, if playback ended in one.
    pub fn failure(&self) -> Option<&MediaError> {
        match self {
            Self::Failed(error) => Some(error),
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
    pub fn new(position: Duration, duration: Duration) -> Self {
        Self {
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
    /// Starts or resumes playback.
    Play,
    /// Pauses playback.
    Pause,
    /// The one button a headset has.
    TogglePlayPause,
    /// Stops playback and closes the item.
    Stop,
    /// Needs a playlist, so it is reported and not carried out.
    Next,
    /// Needs a playlist, so it is reported and not carried out.
    Previous,
    /// Moves playback to the given position from the start of the item.
    SeekTo(Duration),
}

impl MediaCommand {
    /// Whether this command is one the framework carries out itself.
    ///
    /// The transport is player state and lives here. [`Next`](Self::Next) and
    /// [`Previous`](Self::Previous) need an order the framework does not have,
    /// so they are only reported.
    pub fn is_transport(self) -> bool {
        !matches!(self, Self::Next | Self::Previous)
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
    ) -> Option<Self> {
        let samples = samples.into();
        if sample_rate == 0 || channels == 0 || samples.len() % channels as usize != 0 {
            return None;
        }
        Some(Self {
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
