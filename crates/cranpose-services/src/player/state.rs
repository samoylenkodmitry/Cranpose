//! Internal mutable state and backend event handlers for [`Player`](super::Player).

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use parking_lot::Mutex;

use super::{
    AudioRole, AvailableCommands, CueGroup, LoadControl, MasterClock, MediaProperties,
    PlaybackParameters, PlayerBackend, PlayerCommand, PlayerError, PlayerEvent, PlayerId,
    PlayerState, PositionDiscontinuityReason, RepeatMode, TrackSelectionParameters, Tracks,
    VideoSize,
};
use crate::{
    media::{AudioFocus, DUCKED_GAIN, MediaItem, MediaObserver, audio_focus},
    video_decoder::DecodedFrames,
};

pub(super) type EventObserverFn = dyn Fn(PlayerEvent) + Send + Sync;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum StartPlayback {
    Play,
    Paused,
}

impl StartPlayback {
    pub(super) const fn from_play_when_ready(play_when_ready: bool) -> Self {
        if play_when_ready {
            Self::Play
        } else {
            Self::Paused
        }
    }
}

pub(super) struct Snapshot {
    pub(super) playlist: Vec<MediaItem>,
    pub(super) shuffle_order: Vec<usize>,
    pub(super) current_index: usize,
    pub(super) playback_state: PlayerState,
    pub(super) play_when_ready: bool,
    pub(super) is_loading: bool,
    pub(super) error: Option<PlayerError>,
    pub(super) properties: MediaProperties,
    pub(super) buffered_position: Duration,
    pub(super) parameters: PlaybackParameters,
    pub(super) repeat_mode: RepeatMode,
    pub(super) shuffle_mode: bool,
    pub(super) volume: f32,
    pub(super) focus_gain: f32,
    pub(super) paused_by_focus: bool,
    pub(super) muted: bool,
    pub(super) video_size: VideoSize,
    pub(super) rendered_first_frame: bool,
    pub(super) tracks: Tracks,
    pub(super) track_selection: TrackSelectionParameters,
    pub(super) cues: CueGroup,
    pub(super) decoded_frames: Option<DecodedFrames>,
    pub(super) commands: AvailableCommands,
}

#[derive(Clone, Copy)]
enum StepDirection {
    Next,
    Previous,
}

impl Snapshot {
    pub(super) fn new(audio_role: AudioRole) -> Self {
        let focus_gain = if audio_role == AudioRole::Primary && audio_focus() == AudioFocus::Ducked
        {
            DUCKED_GAIN
        } else {
            1.0
        };
        let mut snap = Self {
            playlist: Vec::new(),
            shuffle_order: Vec::new(),
            current_index: 0,
            playback_state: PlayerState::Idle,
            play_when_ready: false,
            is_loading: false,
            error: None,
            properties: MediaProperties::default(),
            buffered_position: Duration::ZERO,
            parameters: PlaybackParameters::DEFAULT,
            repeat_mode: RepeatMode::Off,
            shuffle_mode: false,
            volume: 1.0,
            focus_gain,
            paused_by_focus: false,
            muted: audio_role == AudioRole::Muted,
            video_size: VideoSize::ZERO,
            rendered_first_frame: false,
            tracks: Tracks::EMPTY,
            track_selection: TrackSelectionParameters::default(),
            cues: CueGroup::EMPTY,
            decoded_frames: None,
            commands: AvailableCommands::EMPTY,
        };
        snap.commands = snap.compute_commands(false);
        snap
    }

    pub(super) fn effective_is_playing(&self) -> bool {
        self.playback_state == PlayerState::Ready && self.play_when_ready && self.error.is_none()
    }

    pub(super) fn effective_volume(&self) -> f32 {
        if self.muted {
            0.0
        } else {
            (self.volume * self.focus_gain).clamp(0.0, 1.0)
        }
    }

    pub(super) fn compute_commands(&self, released: bool) -> AvailableCommands {
        if released {
            return AvailableCommands::EMPTY;
        }
        let mut cmds = AvailableCommands::EMPTY
            .with(PlayerCommand::SetVolume)
            .with(PlayerCommand::SetRepeatMode)
            .with(PlayerCommand::SetShuffleMode)
            .with(PlayerCommand::SetSpeedAndPitch)
            .with(PlayerCommand::SetTrackSelectionParameters)
            .with(PlayerCommand::Stop);
        if !self.playlist.is_empty() {
            cmds = cmds
                .with(PlayerCommand::Prepare)
                .with(PlayerCommand::PlayPause);
            if self.properties.is_seekable || self.properties.duration.is_some() {
                cmds = cmds
                    .with(PlayerCommand::SeekInCurrentMediaItem)
                    .with(PlayerCommand::SeekBack)
                    .with(PlayerCommand::SeekForward);
            }
            if self.next_index().is_some() {
                cmds = cmds.with(PlayerCommand::SeekToNext);
            }
            if self.previous_index().is_some() || self.properties.is_seekable {
                cmds = cmds.with(PlayerCommand::SeekToPrevious);
            }
        }
        cmds
    }

    pub(super) fn refresh_commands(&mut self, released: bool) -> Option<PlayerEvent> {
        let next = self.compute_commands(released);
        if next != self.commands {
            self.commands = next;
            Some(PlayerEvent::AvailableCommandsChanged(next))
        } else {
            None
        }
    }

    pub(super) fn rebuild_shuffle_order(&mut self) {
        let len = self.playlist.len();
        self.shuffle_order.clear();
        self.shuffle_order.reserve(len);
        if len == 0 {
            return;
        }
        // Deterministic stride permutation keeping the active item first so enabling shuffle
        // never jumps away from the currently playing item.
        self.shuffle_order.push(self.current_index.min(len - 1));
        let stride = coprime_stride(len);
        let mut cur = self.shuffle_order[0];
        for _ in 1..len {
            cur = (cur + stride) % len;
            self.shuffle_order.push(cur);
        }
    }

    pub(super) fn next_index(&self) -> Option<usize> {
        self.adjacent_index(StepDirection::Next)
    }

    pub(super) fn previous_index(&self) -> Option<usize> {
        self.adjacent_index(StepDirection::Previous)
    }

    fn adjacent_index(&self, direction: StepDirection) -> Option<usize> {
        let len = self.playlist.len();
        if len == 0 {
            return None;
        }
        if self.repeat_mode == RepeatMode::One {
            return Some(self.current_index);
        }
        let ordered = self.shuffle_mode && self.shuffle_order.len() == len;
        let pos = if ordered {
            self.shuffle_order
                .iter()
                .position(|&idx| idx == self.current_index)?
        } else {
            self.current_index
        };
        let next_pos = match direction {
            StepDirection::Next if pos + 1 < len => Some(pos + 1),
            StepDirection::Next if self.repeat_mode == RepeatMode::All => Some(0),
            StepDirection::Previous if pos > 0 => Some(pos - 1),
            StepDirection::Previous if self.repeat_mode == RepeatMode::All => Some(len - 1),
            _ => None,
        }?;
        Some(if ordered {
            self.shuffle_order[next_pos]
        } else {
            next_pos
        })
    }
}

fn coprime_stride(len: usize) -> usize {
    if len <= 2 {
        return 1;
    }
    let mut candidate = (len / 2).max(1);
    while gcd(candidate, len) != 1 {
        candidate += 1;
    }
    candidate
}

fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        let rem = a % b;
        a = b;
        b = rem;
    }
    a
}

pub(super) struct PlayerCore {
    pub(super) id: PlayerId,
    pub(super) load_control: LoadControl,
    pub(super) audio_role: AudioRole,
    pub(super) seek_back_increment: Duration,
    pub(super) seek_forward_increment: Duration,
    pub(super) released: AtomicBool,
    pub(super) clock: MasterClock,
    pub(super) state: Mutex<Snapshot>,
    pub(super) backend: Mutex<Option<Arc<dyn PlayerBackend>>>,
    pub(super) observers: Mutex<Vec<(u64, Arc<EventObserverFn>)>>,
    pub(super) system_observers: Mutex<Vec<MediaObserver>>,
}

impl PlayerCore {
    pub(super) fn new(
        id: PlayerId,
        load_control: LoadControl,
        audio_role: AudioRole,
        seek_back_increment: Duration,
        seek_forward_increment: Duration,
    ) -> Self {
        Self {
            id,
            load_control,
            audio_role,
            seek_back_increment,
            seek_forward_increment,
            released: AtomicBool::new(false),
            clock: MasterClock::new(),
            state: Mutex::new(Snapshot::new(audio_role)),
            backend: Mutex::new(None),
            observers: Mutex::new(Vec::new()),
            system_observers: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn on_audio_focus(&self, focus: AudioFocus) {
        if self.released.load(Ordering::Acquire) || self.audio_role != AudioRole::Primary {
            return;
        }
        let mut events = Vec::new();
        let (volume_out, backend_action) = {
            let mut snap = self.state.lock();
            snap.focus_gain = if focus == AudioFocus::Ducked {
                DUCKED_GAIN
            } else {
                1.0
            };
            let prev_playing = snap.effective_is_playing();
            let action = match focus {
                AudioFocus::Gained => {
                    if snap.paused_by_focus {
                        snap.paused_by_focus = false;
                        if !snap.play_when_ready {
                            snap.play_when_ready = true;
                            events.push(PlayerEvent::PlayWhenReadyChanged(true));
                        }
                        Some(StartPlayback::Play)
                    } else {
                        None
                    }
                }
                AudioFocus::Ducked => None,
                AudioFocus::LostTransient => {
                    if snap.play_when_ready {
                        snap.paused_by_focus = true;
                        snap.play_when_ready = false;
                        events.push(PlayerEvent::PlayWhenReadyChanged(false));
                        Some(StartPlayback::Paused)
                    } else {
                        None
                    }
                }
                AudioFocus::Lost => {
                    snap.paused_by_focus = false;
                    if snap.play_when_ready {
                        snap.play_when_ready = false;
                        events.push(PlayerEvent::PlayWhenReadyChanged(false));
                        Some(StartPlayback::Paused)
                    } else {
                        None
                    }
                }
            };
            let now_playing = snap.effective_is_playing();
            if now_playing {
                self.clock.resume();
            } else {
                self.clock.pause();
            }
            if now_playing != prev_playing {
                events.push(PlayerEvent::IsPlayingChanged(now_playing));
            }
            (snap.effective_volume(), action)
        };
        self.emit(&events);
        if let Some(backend) = self.backend() {
            backend.set_volume(volume_out);
            match backend_action {
                Some(StartPlayback::Play) => backend.play(),
                Some(StartPlayback::Paused) => backend.pause(),
                None => {}
            }
        }
    }

    pub(super) fn emit(&self, events: &[PlayerEvent]) {
        if events.is_empty() {
            return;
        }
        let snapshot: Vec<Arc<EventObserverFn>> = self
            .observers
            .lock()
            .iter()
            .map(|(_, cb)| Arc::clone(cb))
            .collect();
        for event in events {
            for observer in &snapshot {
                observer(event.clone());
            }
        }
    }

    pub(super) fn is_released(&self) -> bool {
        self.released.load(Ordering::Acquire)
    }

    pub(super) fn backend(&self) -> Option<Arc<dyn PlayerBackend>> {
        self.backend.lock().clone()
    }

    pub(super) fn halt_to_idle(&self, clear_frames: bool) -> Vec<PlayerEvent> {
        let released = self.is_released();
        let (prev_playing, state_changed, cmd_event) = {
            let mut snap = self.state.lock();
            let prev_playing = snap.effective_is_playing();
            snap.play_when_ready = false;
            snap.is_loading = false;
            snap.rendered_first_frame = false;
            if clear_frames {
                snap.decoded_frames = None;
            }
            let state_changed = snap.playback_state != PlayerState::Idle;
            snap.playback_state = PlayerState::Idle;
            let cmd_event = snap.refresh_commands(released);
            drop(snap);
            (prev_playing, state_changed, cmd_event)
        };
        self.clock.pause();
        idle_events(state_changed, prev_playing, None, cmd_event)
    }

    pub(super) fn open_active_item(
        &self,
        item: &MediaItem,
        start_pos: Duration,
        start: StartPlayback,
    ) {
        if let Some(backend) = self.backend() {
            let (vol, params, selection) = {
                let snap = self.state.lock();
                (
                    snap.effective_volume(),
                    snap.parameters,
                    snap.track_selection.clone(),
                )
            };
            backend.set_volume(vol);
            backend.set_playback_parameters(params);
            backend.set_track_selection(&selection);
            backend.open(item, start_pos);
            if start == StartPlayback::Play {
                backend.play();
            }
        }
    }

    pub(super) fn on_backend_state(&self, next_state: PlayerState) {
        if self.is_released() {
            return;
        }
        let mut events = Vec::new();
        let mut auto_advance: Option<(MediaItem, StartPlayback)> = None;
        let clock_action;
        {
            let mut snap = self.state.lock();
            if next_state == PlayerState::Ended {
                if let Some(next_idx) = snap.next_index() {
                    let prev_playing = snap.effective_is_playing();
                    snap.current_index = next_idx;
                    snap.rendered_first_frame = false;
                    snap.video_size = VideoSize::ZERO;
                    snap.buffered_position = Duration::ZERO;
                    snap.playback_state = PlayerState::Buffering;
                    let next_item = snap.playlist[next_idx].clone();
                    let start = StartPlayback::from_play_when_ready(snap.play_when_ready);
                    let cmd_ev = snap.refresh_commands(false);
                    drop(snap);
                    clock_action = Some((true, false));
                    events.push(PlayerEvent::MediaItemTransition { index: next_idx });
                    events.push(PlayerEvent::PositionDiscontinuity {
                        reason: PositionDiscontinuityReason::AutoTransition,
                        position: Duration::ZERO,
                    });
                    events.push(PlayerEvent::PlaybackStateChanged(PlayerState::Buffering));
                    if prev_playing {
                        events.push(PlayerEvent::IsPlayingChanged(false));
                    }
                    if let Some(ev) = cmd_ev {
                        events.push(ev);
                    }
                    auto_advance = Some((next_item, start));
                } else {
                    let prev_playing = snap.effective_is_playing();
                    let changed = snap.playback_state != PlayerState::Ended;
                    snap.playback_state = PlayerState::Ended;
                    let cmd_ev = snap.refresh_commands(false);
                    drop(snap);
                    clock_action = Some((false, false));
                    if changed {
                        events.push(PlayerEvent::PlaybackStateChanged(PlayerState::Ended));
                    }
                    if prev_playing {
                        events.push(PlayerEvent::IsPlayingChanged(false));
                    }
                    if let Some(ev) = cmd_ev {
                        events.push(ev);
                    }
                }
            } else {
                let prev_playing = snap.effective_is_playing();
                let changed = snap.playback_state != next_state;
                snap.playback_state = next_state;
                let now_playing = snap.effective_is_playing();
                let cmd_ev = snap.refresh_commands(false);
                drop(snap);
                clock_action = Some((false, now_playing));
                if changed {
                    events.push(PlayerEvent::PlaybackStateChanged(next_state));
                }
                if now_playing != prev_playing {
                    events.push(PlayerEvent::IsPlayingChanged(now_playing));
                }
                if let Some(ev) = cmd_ev {
                    events.push(ev);
                }
            }
        }
        if let Some((reset_zero, running)) = clock_action {
            if reset_zero {
                self.clock.seek(Duration::ZERO);
            }
            if running {
                self.clock.resume();
            } else {
                self.clock.pause();
            }
        }
        self.emit(&events);
        if let Some((item, start)) = auto_advance {
            self.open_active_item(&item, Duration::ZERO, start);
        }
    }

    pub(super) fn on_backend_loading(&self, loading: bool) {
        if self.is_released() {
            return;
        }
        let changed = {
            let mut snap = self.state.lock();
            if snap.is_loading != loading {
                snap.is_loading = loading;
                true
            } else {
                false
            }
        };
        if changed {
            self.emit(&[PlayerEvent::IsLoadingChanged(loading)]);
        }
    }

    pub(super) fn on_backend_properties(&self, properties: MediaProperties) {
        if self.is_released() {
            return;
        }
        let cmd_ev = {
            let mut snap = self.state.lock();
            snap.properties = properties;
            snap.refresh_commands(false)
        };
        if let Some(ev) = cmd_ev {
            self.emit(&[ev]);
        }
    }

    pub(super) fn on_backend_progress(&self, position: Duration, buffered: Duration) {
        if self.is_released() {
            return;
        }
        self.clock.sync_audio(position);
        self.state.lock().buffered_position = buffered;
    }

    pub(super) fn on_backend_video_size(&self, size: VideoSize) {
        if self.is_released() {
            return;
        }
        let changed = {
            let mut snap = self.state.lock();
            if snap.video_size != size {
                snap.video_size = size;
                true
            } else {
                false
            }
        };
        if changed {
            self.emit(&[PlayerEvent::VideoSizeChanged(size)]);
        }
    }

    pub(super) fn on_backend_first_frame(&self) {
        if self.is_released() {
            return;
        }
        let should_emit = {
            let mut snap = self.state.lock();
            if snap.rendered_first_frame {
                false
            } else {
                snap.rendered_first_frame = true;
                true
            }
        };
        if should_emit {
            self.emit(&[PlayerEvent::RenderedFirstFrame]);
        }
    }

    pub(super) fn on_backend_frames(&self, frames: Option<DecodedFrames>) {
        if self.is_released() {
            return;
        }
        let changed = {
            let mut snap = self.state.lock();
            if snap.decoded_frames == frames {
                false
            } else {
                snap.decoded_frames = frames;
                true
            }
        };
        if changed {
            self.emit(&[PlayerEvent::VideoFramesChanged]);
        }
    }

    pub(super) fn on_backend_tracks(&self, tracks: Tracks) {
        if self.is_released() {
            return;
        }
        let changed = {
            let mut snap = self.state.lock();
            if snap.tracks != tracks {
                snap.tracks = tracks.clone();
                true
            } else {
                false
            }
        };
        if changed {
            self.emit(&[PlayerEvent::TracksChanged(tracks)]);
        }
    }

    pub(super) fn on_backend_cues(&self, cues: CueGroup) {
        if self.is_released() {
            return;
        }
        let changed = {
            let mut snap = self.state.lock();
            if snap.cues != cues {
                snap.cues = cues.clone();
                true
            } else {
                false
            }
        };
        if changed {
            self.emit(&[PlayerEvent::CuesChanged(cues)]);
        }
    }

    pub(super) fn on_backend_error(&self, error: PlayerError) {
        if self.is_released() {
            return;
        }
        let (prev_playing, state_changed, cmd_ev) = {
            let mut snap = self.state.lock();
            let prev_playing = snap.effective_is_playing();
            snap.error = Some(error.clone());
            snap.is_loading = false;
            let state_changed = snap.playback_state != PlayerState::Idle;
            snap.playback_state = PlayerState::Idle;
            let cmd_ev = snap.refresh_commands(false);
            drop(snap);
            (prev_playing, state_changed, cmd_ev)
        };
        self.clock.pause();
        let events = idle_events(state_changed, prev_playing, Some(error), cmd_ev);
        self.emit(&events);
    }
}

fn idle_events(
    state_changed: bool,
    prev_playing: bool,
    error: Option<PlayerError>,
    cmd_ev: Option<PlayerEvent>,
) -> Vec<PlayerEvent> {
    let mut events = Vec::new();
    if state_changed {
        events.push(PlayerEvent::PlaybackStateChanged(PlayerState::Idle));
    }
    if prev_playing {
        events.push(PlayerEvent::IsPlayingChanged(false));
    }
    if let Some(err) = error {
        events.push(PlayerEvent::Error(err));
    }
    if let Some(ev) = cmd_ev {
        events.push(ev);
    }
    events
}
