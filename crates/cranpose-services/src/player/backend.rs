//! Platform backend contract and event sink for [`Player`](super::Player).

use std::{sync::Arc, time::Duration};

use super::{
    AudioRole, CueGroup, LoadControl, PlaybackParameters, PlayerError, PlayerState,
    TrackSelectionParameters, Tracks, VideoSize, state::PlayerCore,
};
use crate::{media::MediaItem, registry::ServiceRegistry, video_decoder::DecodedFrames};

/// Stream properties reported by a [`PlayerBackend`] once a media item opens or updates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MediaProperties {
    /// Total duration of the active item, or `None` for unknown/live streams.
    pub duration: Option<Duration>,
    /// Whether the active item is a live stream.
    pub is_live: bool,
    /// Whether seeking within the active item is supported.
    pub is_seekable: bool,
}

/// Thread-safe handle through which a [`PlayerBackend`] publishes asynchronous state, frames,
/// tracks, cues, and errors to its owning [`Player`](super::Player) without blocking the UI thread.
#[derive(Clone)]
pub struct PlayerSink {
    core: Arc<PlayerCore>,
}

impl PlayerSink {
    pub(super) fn new(core: Arc<PlayerCore>) -> Self {
        Self { core }
    }

    /// Publishes a lifecycle state transition (`Idle`, `Buffering`, `Ready`, `Ended`).
    pub fn set_playback_state(&self, state: PlayerState) {
        self.core.on_backend_state(state);
    }

    /// Publishes whether the backend is actively loading network or container data.
    pub fn set_loading(&self, loading: bool) {
        self.core.on_backend_loading(loading);
    }

    /// Updates duration, live flag, and seekability for the active media item.
    pub fn set_media_properties(&self, properties: MediaProperties) {
        self.core.on_backend_properties(properties);
    }

    /// Anchors the master clock to the latest audio/media position and updates buffered position.
    pub fn set_progress(&self, position: Duration, buffered: Duration) {
        self.core.on_backend_progress(position, buffered);
    }

    /// Publishes decoded video dimensions and rotation.
    pub fn set_video_size(&self, size: VideoSize) {
        self.core.on_backend_video_size(size);
    }

    /// Signals that the first GPU video frame for the active item has been rendered.
    pub fn notify_rendered_first_frame(&self) {
        self.core.on_backend_first_frame();
    }

    /// Attaches or replaces the zero-copy GPU [`DecodedFrames`] stream feeding `PlayerSurface`.
    pub fn set_decoded_frames(&self, frames: Option<DecodedFrames>) {
        self.core.on_backend_frames(frames);
    }

    /// Publishes discovered tracks and current selections.
    pub fn set_tracks(&self, tracks: Tracks) {
        self.core.on_backend_tracks(tracks);
    }

    /// Publishes active subtitle/caption cues.
    pub fn set_cues(&self, cues: CueGroup) {
        self.core.on_backend_cues(cues);
    }

    /// Reports a fatal playback error, transitioning the player to [`PlayerState::Idle`].
    pub fn report_error(&self, error: PlayerError) {
        self.core.on_backend_error(error);
    }
}

/// Platform playback backend driven asynchronously by [`Player`](super::Player).
///
/// Every method must return promptly without blocking the calling (UI/composition) thread; I/O,
/// probing, decoding, and teardown run off-thread and push updates back through [`PlayerSink`].
pub trait PlayerBackend: Send + Sync {
    /// Opens and prepares `item` starting at `start_position`, publishing readiness via `sink`.
    fn open(&self, item: &MediaItem, start_position: Duration);

    /// Starts or resumes playback.
    fn play(&self);

    /// Pauses playback without discarding position or buffered state.
    fn pause(&self);

    /// Stops playback and closes the active source.
    fn stop(&self);

    /// Seeks the active source to `position`.
    fn seek_to(&self, position: Duration);

    /// Sets the effective output volume in `[0.0, 1.0]` (already incorporating mute and ducking).
    fn set_volume(&self, volume: f32);

    /// Sets playback speed and pitch multipliers.
    fn set_playback_parameters(&self, parameters: PlaybackParameters);

    /// Applies updated track selection constraints and overrides.
    fn set_track_selection(&self, parameters: &TrackSelectionParameters);

    /// Releases all native decoder, surface, and audio resources held by this backend.
    fn release(&self);
}

/// Factory creating one platform [`PlayerBackend`] per [`Player`](super::Player) instance.
pub trait PlayerBackendFactory: Send + Sync {
    /// Creates a new backend wired to `sink` with the requested buffering and audio role.
    fn create(
        &self,
        sink: PlayerSink,
        load_control: LoadControl,
        audio_role: AudioRole,
    ) -> Result<Arc<dyn PlayerBackend>, PlayerError>;
}

static PLATFORM_FACTORY: ServiceRegistry<dyn PlayerBackendFactory> = ServiceRegistry::new();

/// Installs the platform [`PlayerBackendFactory`], replacing any previous registration.
pub fn set_platform_player_factory(factory: Arc<dyn PlayerBackendFactory>) {
    PLATFORM_FACTORY.set(factory);
}

/// Removes the installed platform [`PlayerBackendFactory`].
pub fn clear_platform_player_factory() {
    PLATFORM_FACTORY.clear();
}

/// Returns the currently installed platform [`PlayerBackendFactory`], if any.
pub fn platform_player_factory() -> Option<Arc<dyn PlayerBackendFactory>> {
    PLATFORM_FACTORY.get()
}
