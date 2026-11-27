//! App lifecycle policy, [`KeepScreenOn`](crate::KeepScreenOn) wiring, and platform fullscreen /
//! Picture-in-Picture hooks for [`Player`].

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use cranpose_core::{
    CollectEvents, MutableState, ProcessIds, State, collectAsState, mutableStateOf,
    rememberEventStream, rememberKeyed,
};
use cranpose_services::{LifecycleState, Player, rememberIsPlaying, rememberLifecycleState};
use cranpose_ui::composable;

/// How [`PlayerLifecycleEffect`] reacts when the application moves to the background and returns
/// to the foreground.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlayerLifecyclePolicy {
    /// Pause playback when backgrounded and resume on return only if this policy paused it.
    #[default]
    PauseInBackground,
    /// Keep playback running in the background (for audio-capable or Picture-in-Picture sessions).
    ContinueInBackground,
    /// Stop playback completely when backgrounded and do not auto-resume on return.
    StopInBackground,
}

/// Keeps the device screen awake while `player` is actively playing and lets it sleep immediately
/// when paused, stopped, or ended.
#[composable]
pub fn KeepScreenOnWhilePlaying(player: Player) {
    let playing = rememberIsPlaying(&player).value();
    crate::KeepScreenOn(playing);
}

/// Connects `player` to application foreground/background lifecycle transitions and automatically
/// keeps the screen awake while actively playing.
#[composable]
pub fn PlayerLifecycleEffect(player: Player, policy: PlayerLifecyclePolicy) {
    KeepScreenOnWhilePlaying(player.clone());
    let paused_by_lifecycle = rememberKeyed(player.id(), |_| mutableStateOf(false));
    let lifecycle = rememberLifecycleState().value();
    let handle = player.clone();
    cranpose_core::LaunchedEffect(
        (player.id(), lifecycle, policy),
        async move |_scope| match (policy, lifecycle) {
            (PlayerLifecyclePolicy::ContinueInBackground, _) => {}
            (
                PlayerLifecyclePolicy::PauseInBackground,
                LifecycleState::Paused | LifecycleState::Stopped,
            ) => {
                if handle.play_when_ready() {
                    paused_by_lifecycle.set(true);
                    handle.pause();
                }
            }
            (PlayerLifecyclePolicy::PauseInBackground, LifecycleState::Resumed) => {
                if paused_by_lifecycle.value() {
                    paused_by_lifecycle.set(false);
                    handle.play();
                }
            }
            (
                PlayerLifecyclePolicy::StopInBackground,
                LifecycleState::Paused | LifecycleState::Stopped,
            ) => {
                paused_by_lifecycle.set(false);
                handle.stop();
            }
            (_, LifecycleState::Destroyed) => {
                paused_by_lifecycle.set(false);
                handle.stop();
            }
            _ => {}
        },
    );
}

/// Presentation display mode of a video player surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlayerDisplayMode {
    /// Standard inline layout inside the application window.
    #[default]
    Normal,
    /// Edge-to-edge fullscreen presentation.
    Fullscreen,
    /// Floating OS Picture-in-Picture window.
    PictureInPicture,
}

/// Which player display modes the current platform host supports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PlayerDisplayCapabilities {
    /// Whether fullscreen mode transitions are supported.
    pub fullscreen: bool,
    /// Whether OS Picture-in-Picture mode transitions are supported.
    pub picture_in_picture: bool,
}

/// Error returned when requesting a [`PlayerDisplayMode`] transition fails.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DisplayModeError {
    /// This platform or host does not support the requested display mode.
    #[error("requested player display mode is not supported on this platform")]
    Unsupported,
    /// The platform refused the transition (for example PiP disabled in system settings).
    #[error("platform refused display mode change: {0}")]
    Refused(String),
}

/// Platform integration contract for fullscreen and Picture-in-Picture video presentation.
pub trait PlayerDisplayController: Send + Sync {
    /// Reports which display modes the host can enter.
    fn capabilities(&self) -> PlayerDisplayCapabilities;

    /// Requests transitioning the host window/activity to `mode`.
    fn request_mode(&self, mode: PlayerDisplayMode) -> Result<(), DisplayModeError>;
}

type DisplayObserverFn = dyn Fn(PlayerDisplayMode) + Send + Sync;

struct DisplayState {
    controller: Option<Arc<dyn PlayerDisplayController>>,
    mode: PlayerDisplayMode,
    observers: Vec<(u64, Arc<DisplayObserverFn>)>,
}

static NEXT_DISPLAY_OBSERVER_ID: ProcessIds = ProcessIds::starting_at(1);

fn display_state() -> &'static Mutex<DisplayState> {
    static STATE: OnceLock<Mutex<DisplayState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(DisplayState {
            controller: None,
            mode: PlayerDisplayMode::Normal,
            observers: Vec::new(),
        })
    })
}

/// RAII handle keeping a [`observe_player_display_mode`] subscription active until dropped.
pub struct PlayerDisplayModeObserver {
    id: u64,
}

impl Drop for PlayerDisplayModeObserver {
    fn drop(&mut self) {
        let mut guard = display_state()
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        guard.observers.retain(|(entry_id, _)| *entry_id != self.id);
    }
}

/// Installs the platform [`PlayerDisplayController`] for fullscreen and Picture-in-Picture hooks.
pub fn set_platform_player_display_controller(controller: Arc<dyn PlayerDisplayController>) {
    let mut guard = display_state()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    guard.controller = Some(controller);
}

/// Removes any installed platform [`PlayerDisplayController`] and resets mode to `Normal`.
pub fn clear_platform_player_display_controller() {
    let mut guard = display_state()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    guard.controller = None;
    guard.mode = PlayerDisplayMode::Normal;
}

/// Returns fullscreen and Picture-in-Picture capabilities supported by the active platform.
pub fn player_display_capabilities() -> PlayerDisplayCapabilities {
    let controller = display_state()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .controller
        .clone();
    controller.map_or_else(PlayerDisplayCapabilities::default, |c| c.capabilities())
}

/// Returns the current [`PlayerDisplayMode`].
pub fn player_display_mode() -> PlayerDisplayMode {
    display_state()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .mode
}

/// Requests transitioning to `mode` through the installed platform [`PlayerDisplayController`].
pub fn request_player_display_mode(mode: PlayerDisplayMode) -> Result<(), DisplayModeError> {
    let controller = display_state()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .controller
        .clone();
    let Some(controller) = controller else {
        return Err(DisplayModeError::Unsupported);
    };
    let caps = controller.capabilities();
    let supported = match mode {
        PlayerDisplayMode::Normal => true,
        PlayerDisplayMode::Fullscreen => caps.fullscreen,
        PlayerDisplayMode::PictureInPicture => caps.picture_in_picture,
    };
    if !supported {
        return Err(DisplayModeError::Unsupported);
    }
    controller.request_mode(mode)?;
    publish_player_display_mode(mode);
    Ok(())
}

/// Publishes a host-initiated [`PlayerDisplayMode`] transition (for example the user exiting
/// Picture-in-Picture from OS chrome).
pub fn publish_player_display_mode(mode: PlayerDisplayMode) {
    let observers = {
        let mut guard = display_state()
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if guard.mode == mode {
            return;
        }
        guard.mode = mode;
        guard
            .observers
            .iter()
            .map(|(_, cb)| Arc::clone(cb))
            .collect::<Vec<_>>()
    };
    for observer in observers {
        observer(mode);
    }
}

/// Registers an observer notified whenever [`player_display_mode`] changes.
pub fn observe_player_display_mode(
    observer: impl Fn(PlayerDisplayMode) + Send + Sync + 'static,
) -> PlayerDisplayModeObserver {
    let id = NEXT_DISPLAY_OBSERVER_ID.next();
    display_state()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .observers
        .push((id, Arc::new(observer)));
    PlayerDisplayModeObserver { id }
}

/// Reactive [`PlayerDisplayMode`] state for Compose screens.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberPlayerDisplayMode() -> State<PlayerDisplayMode> {
    let updates = rememberEventStream((), |sender| {
        observe_player_display_mode(move |mode| sender.send(mode))
    });
    collectAsState(updates, (), player_display_mode())
}

/// Unstyled state holder for a fullscreen toggle button.
#[derive(Clone, Copy)]
pub struct FullscreenButtonState {
    is_enabled: bool,
    mode: MutableState<PlayerDisplayMode>,
}

impl FullscreenButtonState {
    /// Whether fullscreen mode is supported on this host.
    pub fn is_enabled(&self) -> bool {
        self.is_enabled
    }

    /// Whether the player is currently in fullscreen mode.
    pub fn is_fullscreen(&self) -> bool {
        self.mode.value() == PlayerDisplayMode::Fullscreen
    }

    /// Toggles between [`PlayerDisplayMode::Fullscreen`] and [`PlayerDisplayMode::Normal`].
    pub fn on_click(&self) {
        if !self.is_enabled {
            return;
        }
        let target = if self.is_fullscreen() {
            PlayerDisplayMode::Normal
        } else {
            PlayerDisplayMode::Fullscreen
        };
        drop(request_player_display_mode(target));
    }
}

/// Remembers an event-driven [`FullscreenButtonState`].
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberFullscreenButtonState() -> FullscreenButtonState {
    let caps = player_display_capabilities();
    let mode = remember_display_mode_mutable_state();
    FullscreenButtonState {
        is_enabled: caps.fullscreen,
        mode,
    }
}

/// Unstyled state holder for a Picture-in-Picture toggle button.
#[derive(Clone, Copy)]
pub struct PictureInPictureButtonState {
    is_enabled: bool,
    mode: MutableState<PlayerDisplayMode>,
}

impl PictureInPictureButtonState {
    /// Whether Picture-in-Picture is supported on this host.
    pub fn is_enabled(&self) -> bool {
        self.is_enabled
    }

    /// Whether the player is currently in Picture-in-Picture mode.
    pub fn is_in_picture_in_picture(&self) -> bool {
        self.mode.value() == PlayerDisplayMode::PictureInPicture
    }

    /// Toggles between [`PlayerDisplayMode::PictureInPicture`] and [`PlayerDisplayMode::Normal`].
    pub fn on_click(&self) {
        if !self.is_enabled {
            return;
        }
        let target = if self.is_in_picture_in_picture() {
            PlayerDisplayMode::Normal
        } else {
            PlayerDisplayMode::PictureInPicture
        };
        drop(request_player_display_mode(target));
    }
}

/// Remembers an event-driven [`PictureInPictureButtonState`].
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberPictureInPictureButtonState() -> PictureInPictureButtonState {
    let caps = player_display_capabilities();
    let mode = remember_display_mode_mutable_state();
    PictureInPictureButtonState {
        is_enabled: caps.picture_in_picture,
        mode,
    }
}

#[track_caller]
fn remember_display_mode_mutable_state() -> MutableState<PlayerDisplayMode> {
    let mode = rememberKeyed((), |()| mutableStateOf(player_display_mode()));
    let updates = rememberEventStream((), |sender| {
        observe_player_display_mode(move |next| sender.send(next))
    });
    CollectEvents(updates, (), move |next| mode.set(next));
    mode
}
