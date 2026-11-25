//! Shared off-UI command mailbox, wake condvar, [`PlayerBackend`] front-end, and track-selection
//! resolver reused across native player backends (Apple, Android, Linux, Windows).

use std::{
    sync::{Arc, Condvar, Mutex, PoisonError},
    time::Duration,
};

use cranpose_services::{
    MediaItem, PlaybackParameters, PlayerBackend, TrackSelectionParameters, TrackType,
};

pub(crate) struct CommandBatch {
    pub(crate) open: Option<(MediaItem, Duration)>,
    pub(crate) play: Option<bool>,
    pub(crate) stop: bool,
    pub(crate) seek: Option<Duration>,
    pub(crate) volume: Option<f32>,
    pub(crate) parameters: Option<PlaybackParameters>,
    pub(crate) track_selection: Option<TrackSelectionParameters>,
}

struct PendingCommands {
    open: Option<(MediaItem, Duration)>,
    want_play: bool,
    play_changed: bool,
    stop: bool,
    seek: Option<Duration>,
    volume: f32,
    volume_changed: bool,
    parameters: PlaybackParameters,
    parameters_changed: bool,
    track_selection: TrackSelectionParameters,
    track_selection_changed: bool,
    released: bool,
}

impl Default for PendingCommands {
    fn default() -> Self {
        Self {
            open: None,
            want_play: false,
            play_changed: false,
            stop: false,
            seek: None,
            volume: 1.0,
            volume_changed: false,
            parameters: PlaybackParameters::DEFAULT,
            parameters_changed: false,
            track_selection: TrackSelectionParameters::default(),
            track_selection_changed: false,
            released: false,
        }
    }
}

type InterruptHook = Arc<dyn Fn() + Send + Sync>;

pub(crate) struct WorkerControl {
    commands: Mutex<PendingCommands>,
    wake_lock: Arc<Mutex<()>>,
    wake_cv: Arc<Condvar>,
    on_interrupt: Option<InterruptHook>,
}

impl WorkerControl {
    pub(crate) fn new(on_interrupt: Option<InterruptHook>) -> Self {
        Self {
            commands: Mutex::new(PendingCommands::default()),
            wake_lock: Arc::new(Mutex::new(())),
            wake_cv: Arc::new(Condvar::new()),
            on_interrupt,
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub(crate) fn wake_pair(&self) -> (Arc<Mutex<()>>, Arc<Condvar>) {
        (Arc::clone(&self.wake_lock), Arc::clone(&self.wake_cv))
    }

    pub(crate) fn wake(&self) {
        let _guard = self
            .wake_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.wake_cv.notify_all();
    }

    pub(crate) fn has_pending(&self) -> bool {
        let cmd = self.commands.lock().unwrap_or_else(PoisonError::into_inner);
        cmd.released
            || cmd.stop
            || cmd.open.is_some()
            || cmd.play_changed
            || cmd.seek.is_some()
            || cmd.volume_changed
            || cmd.parameters_changed
            || cmd.track_selection_changed
    }

    pub(crate) fn take_batch(&self) -> Option<CommandBatch> {
        let mut cmd = self.commands.lock().unwrap_or_else(PoisonError::into_inner);
        if cmd.released {
            return None;
        }
        let batch = CommandBatch {
            open: cmd.open.take(),
            play: std::mem::take(&mut cmd.play_changed).then_some(cmd.want_play),
            stop: std::mem::take(&mut cmd.stop),
            seek: cmd.seek.take(),
            volume: std::mem::take(&mut cmd.volume_changed).then_some(cmd.volume),
            parameters: std::mem::take(&mut cmd.parameters_changed).then_some(cmd.parameters),
            track_selection: std::mem::take(&mut cmd.track_selection_changed)
                .then(|| cmd.track_selection.clone()),
        };
        drop(cmd);
        Some(batch)
    }

    pub(crate) fn park_until(
        &self,
        active_wait: Option<Duration>,
        extra_ready: impl FnOnce() -> bool,
    ) {
        let wake_guard = self
            .wake_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if self.has_pending() || extra_ready() {
            drop(wake_guard);
            return;
        }
        if let Some(timeout) = active_wait {
            drop(self.wake_cv.wait_timeout(wake_guard, timeout));
        } else {
            drop(self.wake_cv.wait(wake_guard));
        }
    }

    fn interrupt(&self) {
        if let Some(hook) = &self.on_interrupt {
            hook();
        }
    }

    fn lock_commands(&self) -> std::sync::MutexGuard<'_, PendingCommands> {
        self.commands.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub(crate) struct WorkerBackend {
    control: Arc<WorkerControl>,
}

impl WorkerBackend {
    pub(crate) fn new(control: Arc<WorkerControl>) -> Self {
        Self { control }
    }
}

impl Drop for WorkerBackend {
    fn drop(&mut self) {
        self.release();
    }
}

impl PlayerBackend for WorkerBackend {
    fn open(&self, item: &MediaItem, start_position: Duration) {
        self.control.interrupt();
        let mut cmd = self.control.lock_commands();
        if cmd.released {
            return;
        }
        cmd.open = Some((item.clone(), start_position));
        cmd.stop = false;
        cmd.seek = None;
        drop(cmd);
        self.control.wake();
    }

    fn play(&self) {
        let mut cmd = self.control.lock_commands();
        if cmd.released {
            return;
        }
        cmd.want_play = true;
        cmd.play_changed = true;
        drop(cmd);
        self.control.wake();
    }

    fn pause(&self) {
        let mut cmd = self.control.lock_commands();
        if cmd.released {
            return;
        }
        cmd.want_play = false;
        cmd.play_changed = true;
        drop(cmd);
        self.control.wake();
    }

    fn stop(&self) {
        self.control.interrupt();
        let mut cmd = self.control.lock_commands();
        if cmd.released {
            return;
        }
        cmd.open = None;
        cmd.want_play = false;
        cmd.play_changed = true;
        cmd.stop = true;
        cmd.seek = None;
        drop(cmd);
        self.control.wake();
    }

    fn seek_to(&self, position: Duration) {
        let mut cmd = self.control.lock_commands();
        if cmd.released {
            return;
        }
        cmd.seek = Some(position);
        drop(cmd);
        self.control.wake();
    }

    fn set_volume(&self, volume: f32) {
        let mut cmd = self.control.lock_commands();
        if cmd.released {
            return;
        }
        cmd.volume = volume.clamp(0.0, 1.0);
        cmd.volume_changed = true;
        drop(cmd);
        self.control.wake();
    }

    fn set_playback_parameters(&self, parameters: PlaybackParameters) {
        let mut cmd = self.control.lock_commands();
        if cmd.released {
            return;
        }
        cmd.parameters = parameters;
        cmd.parameters_changed = true;
        drop(cmd);
        self.control.wake();
    }

    fn set_track_selection(&self, parameters: &TrackSelectionParameters) {
        let mut cmd = self.control.lock_commands();
        if cmd.released {
            return;
        }
        cmd.track_selection = parameters.clone();
        cmd.track_selection_changed = true;
        drop(cmd);
        self.control.wake();
    }

    fn release(&self) {
        self.control.interrupt();
        self.control.lock_commands().released = true;
        self.control.wake();
    }
}

pub(crate) fn select_track_index<'a>(
    count: usize,
    language_at: impl Fn(usize) -> Option<&'a str>,
    track_type: TrackType,
    group_id: &str,
    params: &TrackSelectionParameters,
) -> Option<usize> {
    if count == 0 || params.is_type_disabled(track_type) {
        return None;
    }
    if let Some(ov) = params.overrides.iter().find(|o| o.group_id == group_id) {
        return ov.track_indices.first().copied().filter(|&i| i < count);
    }
    let prefs = match track_type {
        TrackType::Audio => &params.preferred_audio_languages,
        TrackType::Text => &params.preferred_text_languages,
        TrackType::Video => return Some(0),
    };
    for pref in prefs {
        for idx in 0..count {
            if language_at(idx).is_some_and(|lang| lang.eq_ignore_ascii_case(pref)) {
                return Some(idx);
            }
        }
    }
    Some(0)
}
