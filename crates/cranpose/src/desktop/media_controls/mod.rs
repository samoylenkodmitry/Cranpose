//! The open media item in the operating system's media controls: Now Playing
//! and the media keys on macOS, the media overlay on Windows and MPRIS on
//! Linux. The controls follow the media service, and what a person does in
//! them comes back through `publish_media_command`.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use cranpose_services::{
    MediaMetadata, MediaObserver, PlaybackState, observe_media_metadata, observe_playback_progress,
    observe_playback_state,
};
use winit::window::Window;

use super::integration::{DesktopWaker, DesktopWindowId};

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;
#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod platform;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Status {
    Playing,
    Paused,
    Stopped,
}

// The open item as the controls show it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct NowPlaying {
    pub(super) metadata: MediaMetadata,
    pub(super) status: Status,
    pub(super) position: Duration,
}

impl NowPlaying {
    fn current() -> Option<Self> {
        let item = cranpose_services::current_media_item()?;
        let progress = cranpose_services::playback_progress();
        let mut metadata = item.metadata;
        metadata.duration = progress.duration.or(metadata.duration);
        Some(Self {
            metadata,
            status: status_of(&cranpose_services::playback_state()),
            position: progress.position,
        })
    }
}

pub(super) fn status_of(state: &PlaybackState) -> Status {
    match state {
        PlaybackState::Playing => Status::Playing,
        PlaybackState::Paused | PlaybackState::Loading => Status::Paused,
        PlaybackState::Idle | PlaybackState::Ended | PlaybackState::Failed(_) => Status::Stopped,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Change {
    Nothing,
    // The same item in the same state, at another position.
    Position,
    Everything,
}

// How far the position may move from where the controls count it to, as a
// seek does, before they are told.
const DRIFT: Duration = Duration::from_secs(1);

// What the controls have to be told, when they last showed `shown` at the
// instant beside it. `refresh` is how often a platform that does not count the
// position on by itself is told it while the item plays.
pub(super) fn change(
    shown: Option<&(NowPlaying, Instant)>,
    now: Option<&NowPlaying>,
    at: Instant,
    refresh: Option<Duration>,
) -> Change {
    match (shown, now) {
        (None, None) => Change::Nothing,
        (Some((shown, since)), Some(now))
            if shown.metadata == now.metadata && shown.status == now.status =>
        {
            let elapsed = at.saturating_duration_since(*since);
            let playing = shown.status == Status::Playing;
            let expected = if playing {
                shown.position + elapsed
            } else {
                shown.position
            };
            let due = playing && refresh.is_some_and(|refresh| elapsed >= refresh);
            if now.position.abs_diff(expected) >= DRIFT || due {
                Change::Position
            } else {
                Change::Nothing
            }
        }
        _ => Change::Everything,
    }
}

// The system media controls of the primary window.
#[derive(Default)]
pub(crate) struct MediaControls {
    controls: Option<platform::Controls>,
    observers: Vec<MediaObserver>,
    dirty: Arc<AtomicBool>,
    shown: Option<(NowPlaying, Instant)>,
}

impl MediaControls {
    pub(crate) fn window_opened(
        &mut self,
        id: DesktopWindowId,
        window: &dyn Window,
        waker: DesktopWaker,
    ) {
        if id != DesktopWindowId::Primary || self.controls.is_some() {
            return;
        }
        self.controls = platform::Controls::new(window);
        if self.controls.is_none() {
            return;
        }
        let mark = {
            let dirty = Arc::clone(&self.dirty);
            move || {
                dirty.store(true, Ordering::Release);
                waker.wake();
            }
        };
        let (on_state, on_progress) = (mark.clone(), mark.clone());
        self.observers = vec![
            observe_playback_state(move |_| on_state()),
            observe_playback_progress(move |_| on_progress()),
            observe_media_metadata(move |_| mark()),
        ];
    }

    pub(crate) fn window_closed(&mut self, id: DesktopWindowId) {
        if id == DesktopWindowId::Primary {
            self.observers.clear();
            self.controls = None;
            self.shown = None;
        }
    }

    pub(crate) fn about_to_wait(&mut self) {
        if !self.dirty.swap(false, Ordering::AcqRel) {
            return;
        }
        let Some(controls) = self.controls.as_mut() else {
            return;
        };
        let now = NowPlaying::current();
        let at = Instant::now();
        match change(
            self.shown.as_ref(),
            now.as_ref(),
            at,
            platform::POSITION_REFRESH,
        ) {
            Change::Nothing => return,
            Change::Position => {
                if let Some(now) = &now {
                    controls.show_position(now);
                }
            }
            Change::Everything => controls.show(now.as_ref()),
        }
        self.shown = now.map(|now| (now, at));
    }
}

#[cfg(test)]
#[path = "../tests/media_controls_tests.rs"]
mod tests;
