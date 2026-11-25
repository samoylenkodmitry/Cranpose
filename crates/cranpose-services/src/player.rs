//! Media3-aligned asynchronous [`Player`] engine, A/V master clock, subtitles, and streaming
//! manifest parsers.

mod backend;
mod clock;
mod cues;
mod engine;
mod hooks;
mod state;
mod streaming;
mod tracks;
mod types;

pub use backend::{
    MediaProperties, PlayerBackend, PlayerBackendFactory, PlayerSink,
    clear_platform_player_factory, platform_player_factory, set_platform_player_factory,
};
pub use clock::{FrameTiming, LIP_SYNC_TOLERANCE, MasterClock};
pub use cues::{
    Cue, CueAlignment, CueGroup, SubtitleTrack, parse_srt, parse_subtitles, parse_webvtt,
};
pub use engine::{Player, PlayerBuilder, PlayerObserver};
pub use hooks::{
    rememberIsLoading, rememberIsPlaying, rememberPlayerError, rememberPlayerEvents,
    rememberPlayerState,
};
pub use streaming::{
    AbrVariant, DashAdaptationSet, DashManifest, DashRepresentation, DashSegment,
    HlsMasterPlaylist, HlsMediaPlaylist, HlsRendition, HlsRenditionType, HlsSegment, HlsVariant,
    ManifestError, parse_dash_mpd, parse_hls_master, parse_hls_media, select_abr_variant,
};
pub use tracks::{
    TrackEntry, TrackFormat, TrackGroup, TrackSelectionOverride, TrackSelectionParameters,
    TrackType, Tracks,
};
pub use types::{
    AudioRole, AvailableCommands, LoadControl, PlaybackParameters, PlayerCommand, PlayerError,
    PlayerEvent, PlayerId, PlayerState, PositionDiscontinuityReason, RepeatMode, VideoSize,
};
