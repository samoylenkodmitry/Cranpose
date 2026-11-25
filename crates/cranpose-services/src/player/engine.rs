//! Asynchronous Media3-aligned [`Player`] handle, builder, and playlist transport API.

use std::{
    hash::{Hash, Hasher},
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use cranpose_core::ProcessIds;
use portable_atomic::AtomicU64;

use super::{
    AudioRole, AvailableCommands, CueGroup, LoadControl, MediaProperties, PlaybackParameters,
    PlayerBackendFactory, PlayerCommand, PlayerError, PlayerEvent, PlayerId, PlayerSink,
    PlayerState, PositionDiscontinuityReason, RepeatMode, TrackSelectionParameters, Tracks,
    VideoSize,
    backend::platform_player_factory,
    state::{PlayerCore, StartPlayback},
};
use crate::{
    media::{MediaCommand, MediaItem, observe_audio_focus, observe_media_commands},
    video_decoder::DecodedFrames,
};

static NEXT_PLAYER_ID: ProcessIds = ProcessIds::starting_at(1);
static NEXT_OBSERVER_ID: ProcessIds = ProcessIds::starting_at(1);
static ACTIVE_PRIMARY_PLAYER_ID: AtomicU64 = AtomicU64::new(0);

const DEFAULT_SEEK_BACK: Duration = Duration::from_secs(5);
const DEFAULT_SEEK_FORWARD: Duration = Duration::from_secs(15);
const RESTART_THRESHOLD: Duration = Duration::from_secs(3);

/// Builder for creating a configured [`Player`].
pub struct PlayerBuilder {
    load_control: LoadControl,
    audio_role: AudioRole,
    seek_back_increment: Duration,
    seek_forward_increment: Duration,
    backend_factory: Option<Arc<dyn PlayerBackendFactory>>,
}

impl PlayerBuilder {
    /// Creates a builder with default buffering, audio role, and seek increments (5 s / 15 s).
    pub fn new() -> Self {
        Self {
            load_control: LoadControl::default(),
            audio_role: AudioRole::Primary,
            seek_back_increment: DEFAULT_SEEK_BACK,
            seek_forward_increment: DEFAULT_SEEK_FORWARD,
            backend_factory: None,
        }
    }

    /// Configures buffering thresholds.
    pub fn with_load_control(mut self, load_control: LoadControl) -> Self {
        self.load_control = load_control;
        self
    }

    /// Configures audio focus and OS media session participation.
    pub fn with_audio_role(mut self, audio_role: AudioRole) -> Self {
        self.audio_role = audio_role;
        self
    }

    /// Configures the backward seek increment used by [`Player::seek_back`].
    pub fn with_seek_back_increment(mut self, increment: Duration) -> Self {
        self.seek_back_increment = increment;
        self
    }

    /// Configures the forward seek increment used by [`Player::seek_forward`].
    pub fn with_seek_forward_increment(mut self, increment: Duration) -> Self {
        self.seek_forward_increment = increment;
        self
    }

    /// Uses a specific [`PlayerBackendFactory`] instead of the process-wide platform factory.
    pub fn with_backend_factory(mut self, factory: Arc<dyn PlayerBackendFactory>) -> Self {
        self.backend_factory = Some(factory);
        self
    }

    /// Creates the [`Player`]. If a platform backend factory is installed (or supplied via
    /// [`with_backend_factory`](Self::with_backend_factory)), attaches it asynchronously without
    /// blocking the calling thread.
    pub fn build(self) -> Result<Player, PlayerError> {
        let id = PlayerId(NEXT_PLAYER_ID.next());
        let core = Arc::new(PlayerCore::new(
            id,
            self.load_control,
            self.audio_role,
            self.seek_back_increment,
            self.seek_forward_increment,
        ));
        if let Some(factory) = self.backend_factory.or_else(platform_player_factory) {
            let sink = PlayerSink::new(Arc::clone(&core));
            let backend = factory.create(sink, self.load_control, self.audio_role)?;
            *core.backend.lock() = Some(backend);
        }
        if self.audio_role == AudioRole::Primary {
            let weak_focus = Arc::downgrade(&core);
            let focus_obs = observe_audio_focus(move |focus| {
                if let Some(core) = weak_focus.upgrade() {
                    core.on_audio_focus(focus);
                }
            });
            let weak_cmd = Arc::downgrade(&core);
            let cmd_obs = observe_media_commands(move |command| {
                if let Some(core) = weak_cmd.upgrade() {
                    dispatch_os_command(&Player { core }, command);
                }
            });
            core.system_observers.lock().extend([focus_obs, cmd_obs]);
        }
        Ok(Player { core })
    }
}

impl Default for PlayerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII registration keeping a [`Player::observe_events`] listener alive until dropped.
pub struct PlayerObserver {
    core: Arc<PlayerCore>,
    id: u64,
}

impl Drop for PlayerObserver {
    fn drop(&mut self) {
        self.core
            .observers
            .lock()
            .retain(|(entry_id, _)| *entry_id != self.id);
    }
}

/// Media3-aligned lightweight, cloneable handle to an asynchronous audio/video player.
///
/// Cloning a `Player` references the same underlying playback engine. Every transport method
/// returns immediately without blocking the composition or UI thread; state changes arrive
/// reactively through [`observe_events`](Self::observe_events) and the `remember*` composables.
#[derive(Clone)]
pub struct Player {
    core: Arc<PlayerCore>,
}

impl PartialEq for Player {
    fn eq(&self, other: &Self) -> bool {
        self.core.id == other.core.id
    }
}

impl Eq for Player {}

impl Hash for Player {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.core.id.hash(state);
    }
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let snap = self.core.state.lock();
        f.debug_struct("Player")
            .field("id", &self.core.id)
            .field("state", &snap.playback_state)
            .field("is_playing", &snap.effective_is_playing())
            .field("playlist_len", &snap.playlist.len())
            .field("current_index", &snap.current_index)
            .finish()
    }
}

impl Player {
    /// Returns a [`PlayerBuilder`] for configuring and creating a player.
    pub fn builder() -> PlayerBuilder {
        PlayerBuilder::new()
    }

    /// Unique identifier for this player instance.
    pub fn id(&self) -> PlayerId {
        self.core.id
    }

    /// Configured buffering thresholds.
    pub fn load_control(&self) -> LoadControl {
        self.core.load_control
    }

    /// Configured audio focus and OS media session role.
    pub fn audio_role(&self) -> AudioRole {
        self.core.audio_role
    }

    /// Configured backward seek increment.
    pub fn seek_back_increment(&self) -> Duration {
        self.core.seek_back_increment
    }

    /// Configured forward seek increment.
    pub fn seek_forward_increment(&self) -> Duration {
        self.core.seek_forward_increment
    }

    /// Replaces the playlist with a single `item` and resets the active index to `0`.
    pub fn set_media_item(&self, item: MediaItem) {
        self.set_media_items(vec![item], 0, Duration::ZERO);
    }

    /// Replaces the playlist with `items`, selecting `start_index` at `start_position`.
    pub fn set_media_items(
        &self,
        items: Vec<MediaItem>,
        start_index: usize,
        start_position: Duration,
    ) {
        if self.core.is_released() {
            return;
        }
        let (current_index, active_item, should_open, start, cmd_ev) = {
            let mut snap = self.core.state.lock();
            snap.playlist = items;
            snap.current_index = if snap.playlist.is_empty() {
                0
            } else {
                start_index.min(snap.playlist.len() - 1)
            };
            snap.rebuild_shuffle_order();
            snap.error = None;
            snap.rendered_first_frame = false;
            snap.video_size = VideoSize::ZERO;
            snap.tracks = Tracks::EMPTY;
            snap.cues = CueGroup::EMPTY;
            snap.properties = MediaProperties::default();
            snap.buffered_position = Duration::ZERO;
            let active_item = snap.playlist.get(snap.current_index).cloned();
            let should_open = snap.playback_state != PlayerState::Idle && active_item.is_some();
            let start = StartPlayback::from_play_when_ready(snap.play_when_ready);
            let cmd_ev = snap.refresh_commands(false);
            (snap.current_index, active_item, should_open, start, cmd_ev)
        };
        self.core.clock.seek(start_position);
        self.core.clock.pause();
        let mut events = Vec::new();
        if active_item.is_some() {
            events.push(PlayerEvent::MediaItemTransition {
                index: current_index,
            });
        }
        if let Some(ev) = cmd_ev {
            events.push(ev);
        }
        self.core.emit(&events);
        if should_open && let Some(item) = active_item {
            self.core.open_active_item(&item, start_position, start);
        }
    }

    /// Appends `item` to the end of the playlist without interrupting active playback.
    pub fn add_media_item(&self, item: MediaItem) {
        if self.core.is_released() {
            return;
        }
        let cmd_ev = {
            let mut snap = self.core.state.lock();
            snap.playlist.push(item);
            snap.rebuild_shuffle_order();
            snap.refresh_commands(false)
        };
        if let Some(ev) = cmd_ev {
            self.core.emit(&[ev]);
        }
    }

    /// Removes the media item at `index` if present.
    pub fn remove_media_item(&self, index: usize) {
        if self.core.is_released() {
            return;
        }
        let cmd_ev = {
            let mut snap = self.core.state.lock();
            if index >= snap.playlist.len() {
                return;
            }
            snap.playlist.remove(index);
            if snap.playlist.is_empty() {
                snap.current_index = 0;
            } else if snap.current_index >= snap.playlist.len() {
                snap.current_index = snap.playlist.len() - 1;
            } else if index < snap.current_index {
                snap.current_index -= 1;
            }
            snap.rebuild_shuffle_order();
            snap.refresh_commands(false)
        };
        if let Some(ev) = cmd_ev {
            self.core.emit(&[ev]);
        }
    }

    /// Clears all items from the playlist and stops playback.
    pub fn clear_media_items(&self) {
        self.stop();
        let released = self.core.is_released();
        let cmd_ev = {
            let mut snap = self.core.state.lock();
            snap.playlist.clear();
            snap.shuffle_order.clear();
            snap.current_index = 0;
            snap.refresh_commands(released)
        };
        if let Some(ev) = cmd_ev {
            self.core.emit(&[ev]);
        }
    }

    /// Begins preparing the active media item asynchronously.
    pub fn prepare(&self) {
        if self.core.is_released() {
            return;
        }
        let (item, start_pos, start, became_buffering, cmd_ev) = {
            let mut snap = self.core.state.lock();
            let Some(item) = snap.playlist.get(snap.current_index).cloned() else {
                return;
            };
            snap.error = None;
            let became_buffering = snap.playback_state == PlayerState::Idle
                || snap.playback_state == PlayerState::Ended;
            if became_buffering {
                snap.playback_state = PlayerState::Buffering;
            }
            let cmd_ev = snap.refresh_commands(false);
            let start = StartPlayback::from_play_when_ready(snap.play_when_ready);
            drop(snap);
            (
                item,
                self.core.clock.position(),
                start,
                became_buffering,
                cmd_ev,
            )
        };
        let mut events = Vec::new();
        if became_buffering {
            events.push(PlayerEvent::PlaybackStateChanged(PlayerState::Buffering));
        }
        if let Some(ev) = cmd_ev {
            events.push(ev);
        }
        self.core.emit(&events);
        if self.core.audio_role == AudioRole::Primary {
            ACTIVE_PRIMARY_PLAYER_ID.store(self.core.id.0, Ordering::Release);
        }
        self.core.open_active_item(&item, start_pos, start);
    }

    /// Requests playback to start (or resume once prepared).
    pub fn play(&self) {
        self.set_play_when_ready(true);
    }

    /// Requests playback to pause without releasing the loaded item.
    pub fn pause(&self) {
        self.set_play_when_ready(false);
    }

    /// Sets `play_when_ready` directly.
    pub fn set_play_when_ready(&self, play_when_ready: bool) {
        if self.core.is_released() {
            return;
        }
        let (pwr_changed, was_ended, prev_playing, now_playing, cmd_ev, active_item) = {
            let mut snap = self.core.state.lock();
            let prev_playing = snap.effective_is_playing();
            snap.paused_by_focus = false;
            let pwr_changed = snap.play_when_ready != play_when_ready;
            snap.play_when_ready = play_when_ready;
            let was_ended = play_when_ready && snap.playback_state == PlayerState::Ended;
            if was_ended {
                snap.playback_state = PlayerState::Buffering;
            }
            let now_playing = snap.effective_is_playing();
            let cmd_ev = snap.refresh_commands(false);
            let active_item = snap.playlist.get(snap.current_index).cloned();
            drop(snap);
            (
                pwr_changed,
                was_ended,
                prev_playing,
                now_playing,
                cmd_ev,
                active_item,
            )
        };
        if was_ended {
            self.core.clock.seek(Duration::ZERO);
        }
        if now_playing {
            self.core.clock.resume();
        } else {
            self.core.clock.pause();
        }
        let mut events = Vec::new();
        if pwr_changed {
            events.push(PlayerEvent::PlayWhenReadyChanged(play_when_ready));
        }
        if was_ended {
            events.push(PlayerEvent::PositionDiscontinuity {
                reason: PositionDiscontinuityReason::Seek,
                position: Duration::ZERO,
            });
            events.push(PlayerEvent::PlaybackStateChanged(PlayerState::Buffering));
        }
        if now_playing != prev_playing {
            events.push(PlayerEvent::IsPlayingChanged(now_playing));
        }
        if let Some(ev) = cmd_ev {
            events.push(ev);
        }
        self.core.emit(&events);
        if play_when_ready && self.core.audio_role == AudioRole::Primary {
            ACTIVE_PRIMARY_PLAYER_ID.store(self.core.id.0, Ordering::Release);
        }
        if was_ended && let Some(item) = active_item {
            self.core
                .open_active_item(&item, Duration::ZERO, StartPlayback::Play);
        } else if let Some(backend) = self.core.backend() {
            if play_when_ready {
                backend.play();
            } else {
                backend.pause();
            }
        }
    }

    /// Stops playback, transitioning to [`PlayerState::Idle`].
    pub fn stop(&self) {
        if self.core.released.load(Ordering::Acquire) {
            return;
        }
        let events = self.core.halt_to_idle(false);
        self.core.emit(&events);
        if let Some(backend) = self.core.backend() {
            backend.stop();
        }
    }

    /// Seeks within the current media item to `position`, clamped to known duration.
    pub fn seek_to(&self, position: Duration) {
        if self.core.released.load(Ordering::Acquire) {
            return;
        }
        let clamped = {
            let snap = self.core.state.lock();
            match snap.properties.duration {
                Some(dur) => position.min(dur),
                None => position,
            }
        };
        self.core.clock.seek(clamped);
        self.core.emit(&[PlayerEvent::PositionDiscontinuity {
            reason: PositionDiscontinuityReason::Seek,
            position: clamped,
        }]);
        if let Some(backend) = self.core.backend() {
            backend.seek_to(clamped);
        }
    }

    /// Seeks backward by [`seek_back_increment`](Self::seek_back_increment).
    pub fn seek_back(&self) {
        let target = self
            .current_position()
            .saturating_sub(self.core.seek_back_increment);
        self.seek_to(target);
    }

    /// Seeks forward by [`seek_forward_increment`](Self::seek_forward_increment).
    pub fn seek_forward(&self) {
        let target = self
            .current_position()
            .saturating_add(self.core.seek_forward_increment);
        self.seek_to(target);
    }

    /// Skips to the next media item according to repeat and shuffle modes.
    pub fn seek_to_next(&self) {
        if self.core.released.load(Ordering::Acquire) {
            return;
        }
        let next = self.core.state.lock().next_index();
        if let Some(index) = next {
            self.jump_to_index(index);
        }
    }

    /// Returns to the start of the current item if past 3 seconds, or skips to the previous item.
    pub fn seek_to_previous(&self) {
        if self.core.released.load(Ordering::Acquire) {
            return;
        }
        if self.current_position() > RESTART_THRESHOLD {
            self.seek_to(Duration::ZERO);
            return;
        }
        let prev = self.core.state.lock().previous_index();
        if let Some(index) = prev {
            self.jump_to_index(index);
        } else {
            self.seek_to(Duration::ZERO);
        }
    }

    fn jump_to_index(&self, index: usize) {
        let (item, should_open, start, cmd_ev) = {
            let mut snap = self.core.state.lock();
            if index >= snap.playlist.len() {
                return;
            }
            snap.current_index = index;
            snap.error = None;
            snap.rendered_first_frame = false;
            snap.video_size = VideoSize::ZERO;
            snap.buffered_position = Duration::ZERO;
            snap.tracks = Tracks::EMPTY;
            snap.cues = CueGroup::EMPTY;
            let item = snap.playlist[index].clone();
            let open = snap.playback_state != PlayerState::Idle;
            if open {
                snap.playback_state = PlayerState::Buffering;
            }
            let cmd_ev = snap.refresh_commands(false);
            let start = StartPlayback::from_play_when_ready(snap.play_when_ready);
            drop(snap);
            (item, open, start, cmd_ev)
        };
        self.core.clock.seek(Duration::ZERO);
        self.core.clock.pause();
        let mut events = vec![
            PlayerEvent::MediaItemTransition { index },
            PlayerEvent::PositionDiscontinuity {
                reason: PositionDiscontinuityReason::Skip,
                position: Duration::ZERO,
            },
        ];
        if should_open {
            events.push(PlayerEvent::PlaybackStateChanged(PlayerState::Buffering));
        }
        if let Some(ev) = cmd_ev {
            events.push(ev);
        }
        self.core.emit(&events);
        if should_open {
            self.core.open_active_item(&item, Duration::ZERO, start);
        }
    }

    /// Sets playback speed and pitch multipliers.
    pub fn set_playback_parameters(&self, parameters: PlaybackParameters) {
        if self.core.released.load(Ordering::Acquire) {
            return;
        }
        let validated = PlaybackParameters::new(parameters.speed, parameters.pitch);
        let changed = {
            let mut snap = self.core.state.lock();
            if snap.parameters == validated {
                false
            } else {
                snap.parameters = validated;
                true
            }
        };
        if changed {
            self.core.clock.set_rate(validated.speed);
            self.core
                .emit(&[PlayerEvent::PlaybackParametersChanged(validated)]);
            if let Some(backend) = self.core.backend() {
                backend.set_playback_parameters(validated);
            }
        }
    }

    /// Sets playlist repeat mode (`Off`, `One`, `All`).
    pub fn set_repeat_mode(&self, repeat_mode: RepeatMode) {
        if self.core.is_released() {
            return;
        }
        let (changed, cmd_ev) = {
            let mut snap = self.core.state.lock();
            if snap.repeat_mode != repeat_mode {
                snap.repeat_mode = repeat_mode;
                (true, snap.refresh_commands(false))
            } else {
                (false, None)
            }
        };
        if changed {
            let mut events = vec![PlayerEvent::RepeatModeChanged(repeat_mode)];
            if let Some(ev) = cmd_ev {
                events.push(ev);
            }
            self.core.emit(&events);
        }
    }

    /// Enables or disables shuffled playlist order.
    pub fn set_shuffle_mode(&self, enabled: bool) {
        if self.core.is_released() {
            return;
        }
        let (changed, cmd_ev) = {
            let mut snap = self.core.state.lock();
            if snap.shuffle_mode != enabled {
                snap.shuffle_mode = enabled;
                snap.rebuild_shuffle_order();
                (true, snap.refresh_commands(false))
            } else {
                (false, None)
            }
        };
        if changed {
            let mut events = vec![PlayerEvent::ShuffleModeChanged(enabled)];
            if let Some(ev) = cmd_ev {
                events.push(ev);
            }
            self.core.emit(&events);
        }
    }

    /// Sets linear volume in `[0.0, 1.0]`.
    pub fn set_volume(&self, volume: f32) {
        if self.core.released.load(Ordering::Acquire) {
            return;
        }
        let clamped = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let (changed, effective) = {
            let mut snap = self.core.state.lock();
            let changed = (snap.volume - clamped).abs() > f32::EPSILON;
            snap.volume = clamped;
            (changed, snap.effective_volume())
        };
        if changed {
            self.core.emit(&[PlayerEvent::VolumeChanged(clamped)]);
            if let Some(backend) = self.core.backend() {
                backend.set_volume(effective);
            }
        }
    }

    /// Sets muted state independently of the stored volume level.
    pub fn set_muted(&self, muted: bool) {
        if self.core.released.load(Ordering::Acquire) {
            return;
        }
        let (changed, effective) = {
            let mut snap = self.core.state.lock();
            let changed = snap.muted != muted;
            snap.muted = muted;
            (changed, snap.effective_volume())
        };
        if changed {
            self.core.emit(&[PlayerEvent::MutedChanged(muted)]);
            if let Some(backend) = self.core.backend() {
                backend.set_volume(effective);
            }
        }
    }

    /// Updates track selection constraints and explicit overrides.
    pub fn set_track_selection_parameters(&self, parameters: TrackSelectionParameters) {
        if self.core.released.load(Ordering::Acquire) {
            return;
        }
        {
            let mut snap = self.core.state.lock();
            if snap.track_selection == parameters {
                return;
            }
            snap.track_selection = parameters;
        }
        if let Some(backend) = self.core.backend() {
            let current = self.core.state.lock().track_selection.clone();
            backend.set_track_selection(&current);
        }
    }

    /// Releases the player and its native backend resources. Subsequent commands are ignored.
    pub fn release(&self) {
        if self.core.released.swap(true, Ordering::AcqRel) {
            return;
        }
        let backend = self.core.backend.lock().take();
        self.core.system_observers.lock().clear();
        let _ = ACTIVE_PRIMARY_PLAYER_ID.compare_exchange(
            self.core.id.0,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        let events = self.core.halt_to_idle(true);
        self.core.emit(&events);
        if let Some(backend) = backend {
            backend.release();
        }
    }

    /// Current playback lifecycle state (`Idle`, `Buffering`, `Ready`, `Ended`).
    pub fn playback_state(&self) -> PlayerState {
        self.core.state.lock().playback_state
    }

    /// Whether playback will proceed automatically whenever media is ready.
    pub fn play_when_ready(&self) -> bool {
        self.core.state.lock().play_when_ready
    }

    /// Whether media is actively playing (`Ready && play_when_ready && no error`).
    pub fn is_playing(&self) -> bool {
        self.core.state.lock().effective_is_playing()
    }

    /// Whether the backend is currently fetching or buffering media data.
    pub fn is_loading(&self) -> bool {
        self.core.state.lock().is_loading
    }

    /// Latest fatal playback error, if any.
    pub fn player_error(&self) -> Option<PlayerError> {
        self.core.state.lock().error.clone()
    }

    /// Current presentation position from the master clock, clamped to known duration.
    pub fn current_position(&self) -> Duration {
        let pos = self.core.clock.position();
        match self.core.state.lock().properties.duration {
            Some(dur) => pos.min(dur),
            None => pos,
        }
    }

    /// Furthest buffered media position ahead of the playhead.
    pub fn buffered_position(&self) -> Duration {
        self.core.state.lock().buffered_position
    }

    /// Total duration of the active media item, or `None` when unknown or live.
    pub fn duration(&self) -> Option<Duration> {
        self.core.state.lock().properties.duration
    }

    /// Whether the active media item is a live stream.
    pub fn is_current_media_item_live(&self) -> bool {
        self.core.state.lock().properties.is_live
    }

    /// Whether seeking is supported on the active media item.
    pub fn is_current_media_item_seekable(&self) -> bool {
        self.core.state.lock().properties.is_seekable
    }

    /// Active playback speed and pitch multipliers.
    pub fn playback_parameters(&self) -> PlaybackParameters {
        self.core.state.lock().parameters
    }

    /// Active playlist repeat mode.
    pub fn repeat_mode(&self) -> RepeatMode {
        self.core.state.lock().repeat_mode
    }

    /// Whether playlist shuffle mode is enabled.
    pub fn shuffle_mode_enabled(&self) -> bool {
        self.core.state.lock().shuffle_mode
    }

    /// Configured linear volume in `[0.0, 1.0]`.
    pub fn volume(&self) -> f32 {
        self.core.state.lock().volume
    }

    /// Whether output audio is muted.
    pub fn is_muted(&self) -> bool {
        self.core.state.lock().muted
    }

    /// Active video dimensions and unapplied rotation.
    pub fn video_size(&self) -> VideoSize {
        self.core.state.lock().video_size
    }

    /// Whether the first GPU video frame of the active item has rendered.
    pub fn rendered_first_frame(&self) -> bool {
        self.core.state.lock().rendered_first_frame
    }

    /// Active discovered tracks and selections.
    pub fn current_tracks(&self) -> Tracks {
        self.core.state.lock().tracks.clone()
    }

    /// Active track selection constraints and overrides.
    pub fn track_selection_parameters(&self) -> TrackSelectionParameters {
        self.core.state.lock().track_selection.clone()
    }

    /// Currently active subtitle/caption cues.
    pub fn current_cues(&self) -> CueGroup {
        self.core.state.lock().cues.clone()
    }

    /// Bitset of commands currently available on this player.
    pub fn available_commands(&self) -> AvailableCommands {
        self.core.state.lock().commands
    }

    /// Whether `command` is currently available.
    pub fn is_command_available(&self, command: PlayerCommand) -> bool {
        self.available_commands().contains(command)
    }

    /// Active zero-copy GPU [`DecodedFrames`] handle feeding `PlayerSurface`, if any.
    pub fn decoded_frames(&self) -> Option<DecodedFrames> {
        self.core.state.lock().decoded_frames.clone()
    }

    /// Marks the first GPU video frame for the active item as rendered (idempotent per item).
    ///
    /// Called by `PlayerSurface` on first GPU presentation so shutters in `ContentFrame` dismiss
    /// cleanly even when a backend delivers frames purely through [`DecodedFrames`].
    pub fn notify_rendered_first_frame(&self) {
        self.core.on_backend_first_frame();
    }

    /// Active index in the playlist (`0` when empty).
    pub fn current_media_item_index(&self) -> usize {
        self.core.state.lock().current_index
    }

    /// Number of items in the playlist.
    pub fn media_item_count(&self) -> usize {
        self.core.state.lock().playlist.len()
    }

    /// Active media item, if the playlist is non-empty.
    pub fn current_media_item(&self) -> Option<MediaItem> {
        let snap = self.core.state.lock();
        snap.playlist.get(snap.current_index).cloned()
    }

    /// Media item at `index`, if present.
    pub fn media_item_at(&self, index: usize) -> Option<MediaItem> {
        self.core.state.lock().playlist.get(index).cloned()
    }

    /// Whether [`seek_to_next`](Self::seek_to_next) has a next item to advance to.
    pub fn has_next_media_item(&self) -> bool {
        self.core.state.lock().next_index().is_some()
    }

    /// Whether [`seek_to_previous`](Self::seek_to_previous) has a previous item to move to.
    pub fn has_previous_media_item(&self) -> bool {
        self.core.state.lock().previous_index().is_some()
    }

    /// Registers an observer notified on every [`PlayerEvent`]. Drop the returned
    /// [`PlayerObserver`] to unsubscribe.
    pub fn observe_events(
        &self,
        observer: impl Fn(PlayerEvent) + Send + Sync + 'static,
    ) -> PlayerObserver {
        let id = NEXT_OBSERVER_ID.next();
        self.core.observers.lock().push((id, Arc::new(observer)));
        PlayerObserver {
            core: Arc::clone(&self.core),
            id,
        }
    }
}

fn dispatch_os_command(player: &Player, command: MediaCommand) {
    if ACTIVE_PRIMARY_PLAYER_ID.load(Ordering::Acquire) != player.core.id.0
        || player.media_item_count() == 0
        || player.playback_state() == PlayerState::Idle
    {
        return;
    }
    match command {
        MediaCommand::Play => player.play(),
        MediaCommand::Pause => player.pause(),
        MediaCommand::TogglePlayPause => {
            if player.play_when_ready() {
                player.pause();
            } else {
                player.play();
            }
        }
        MediaCommand::Stop => player.stop(),
        MediaCommand::Next => player.seek_to_next(),
        MediaCommand::Previous => player.seek_to_previous(),
        MediaCommand::SeekTo(position) => player.seek_to(position),
    }
}
