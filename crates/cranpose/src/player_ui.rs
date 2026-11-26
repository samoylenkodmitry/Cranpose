//! Media3-aligned Compose UI layer: [`ContentFrame`], [`rememberPlayer`], presentation shutter
//! state, unstyled event-driven control state holders, track/cue state, and lifecycle/display-mode
//! hooks.

mod controls;
mod feed;
mod frame;
mod lifecycle;
mod progress;
mod sync;
mod tracks_cues;

pub use controls::{
    MuteButtonState, NextButtonState, PlayPauseButtonState, PlaybackSpeedState,
    PreviousButtonState, RepeatButtonState, SeekBackButtonState, SeekForwardButtonState,
    ShuffleButtonState, rememberMuteButtonState, rememberNextButtonState,
    rememberPlayPauseButtonState, rememberPlaybackSpeedState, rememberPreviousButtonState,
    rememberRepeatButtonState, rememberSeekBackButtonState, rememberSeekForwardButtonState,
    rememberShuffleButtonState,
};
pub use feed::rememberPlayerPool;
pub use frame::{
    ContentFrame, PresentationState, rememberPlayer, rememberPlayerWith, rememberPresentationState,
};
pub use lifecycle::{
    DisplayModeError, FullscreenButtonState, KeepScreenOnWhilePlaying, PictureInPictureButtonState,
    PlayerDisplayCapabilities, PlayerDisplayController, PlayerDisplayMode,
    PlayerDisplayModeObserver, PlayerLifecycleEffect, PlayerLifecyclePolicy,
    clear_platform_player_display_controller, observe_player_display_mode,
    player_display_capabilities, player_display_mode, publish_player_display_mode,
    rememberFullscreenButtonState, rememberPictureInPictureButtonState, rememberPlayerDisplayMode,
    request_player_display_mode, set_platform_player_display_controller,
};
pub use progress::{
    ProgressStateWithTickInterval, rememberProgressStateWithTickCount,
    rememberProgressStateWithTickInterval,
};
pub use tracks_cues::{CuesState, TracksState, rememberCuesState, rememberTracksState};
