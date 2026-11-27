//! Unstyled event-driven control state holders matching `androidx.media3.ui.compose.state`.

use std::time::Duration;

use cranpose_core::{MutableState, mutableStateOf};
use cranpose_services::{
    PlaybackParameters, Player, PlayerCommand, PlayerEvent, PlayerState, RepeatMode,
};

use super::sync::remember_player_holder;

/// Unstyled state holder for a play/pause button, matching Media3 `PlayPauseButtonState`.
#[derive(Clone)]
pub struct PlayPauseButtonState {
    player: Player,
    is_enabled: MutableState<bool>,
    show_play: MutableState<bool>,
}

impl PlayPauseButtonState {
    /// Whether the play/pause control can currently be invoked.
    pub fn is_enabled(&self) -> bool {
        self.is_enabled.value()
    }

    /// `true` when the button should display a play icon, `false` for pause.
    pub fn show_play(&self) -> bool {
        self.show_play.value()
    }

    /// Toggles playback (preparing first if the player is idle).
    pub fn on_click(&self) {
        if !self.is_enabled.value() {
            return;
        }
        if self.show_play.value() {
            if self.player.playback_state() == PlayerState::Idle {
                self.player.prepare();
            }
            self.player.play();
        } else {
            self.player.pause();
        }
    }
}

/// Remembers an event-driven [`PlayPauseButtonState`] for `player`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberPlayPauseButtonState(player: &Player) -> PlayPauseButtonState {
    remember_player_holder(
        player,
        |p| PlayPauseButtonState {
            is_enabled: mutableStateOf(p.is_command_available(PlayerCommand::PlayPause)),
            show_play: mutableStateOf(should_show_play(&p)),
            player: p,
        },
        |p, event| match event {
            PlayerEvent::PlaybackStateChanged(_)
            | PlayerEvent::PlayWhenReadyChanged(_)
            | PlayerEvent::IsPlayingChanged(_)
            | PlayerEvent::AvailableCommandsChanged(_) => Some((
                p.is_command_available(PlayerCommand::PlayPause),
                should_show_play(p),
            )),
            _ => None,
        },
        |holder, (enabled, show)| {
            holder.is_enabled.set(enabled);
            holder.show_play.set(show);
        },
    )
}

fn should_show_play(player: &Player) -> bool {
    !player.play_when_ready()
        || matches!(
            player.playback_state(),
            PlayerState::Idle | PlayerState::Ended
        )
}

/// Unstyled state holder for a mute/volume button, matching Media3 `MuteButtonState`.
#[derive(Clone)]
pub struct MuteButtonState {
    player: Player,
    is_enabled: MutableState<bool>,
    is_muted: MutableState<bool>,
    volume: MutableState<f32>,
}

impl MuteButtonState {
    /// Whether volume and mute adjustments are currently available.
    pub fn is_enabled(&self) -> bool {
        self.is_enabled.value()
    }

    /// Whether audio output is muted.
    pub fn is_muted(&self) -> bool {
        self.is_muted.value()
    }

    /// Current linear volume in `[0.0, 1.0]`.
    pub fn volume(&self) -> f32 {
        self.volume.value()
    }

    /// Toggles the player's muted state.
    pub fn on_click(&self) {
        if self.is_enabled.value() {
            self.player.set_muted(!self.player.is_muted());
        }
    }

    /// Sets linear volume in `[0.0, 1.0]`.
    pub fn set_volume(&self, volume: f32) {
        if self.is_enabled.value() {
            self.player.set_volume(volume);
        }
    }
}

/// Remembers an event-driven [`MuteButtonState`] for `player`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberMuteButtonState(player: &Player) -> MuteButtonState {
    remember_player_holder(
        player,
        |p| MuteButtonState {
            is_enabled: mutableStateOf(p.is_command_available(PlayerCommand::SetVolume)),
            is_muted: mutableStateOf(p.is_muted()),
            volume: mutableStateOf(p.volume()),
            player: p,
        },
        |p, event| match event {
            PlayerEvent::MutedChanged(_)
            | PlayerEvent::VolumeChanged(_)
            | PlayerEvent::AvailableCommandsChanged(_) => Some((
                p.is_command_available(PlayerCommand::SetVolume),
                p.is_muted(),
                p.volume(),
            )),
            _ => None,
        },
        |holder, (enabled, muted, volume)| {
            holder.is_enabled.set(enabled);
            holder.is_muted.set(muted);
            holder.volume.set(volume);
        },
    )
}

/// Unstyled state holder for playback speed controls, matching Media3 `PlaybackSpeedState`.
#[derive(Clone)]
pub struct PlaybackSpeedState {
    player: Player,
    is_enabled: MutableState<bool>,
    playback_speed: MutableState<f32>,
}

impl PlaybackSpeedState {
    /// Whether changing playback speed is currently available.
    pub fn is_enabled(&self) -> bool {
        self.is_enabled.value()
    }

    /// Active playback speed multiplier (`1.0` = normal speed).
    pub fn playback_speed(&self) -> f32 {
        self.playback_speed.value()
    }

    /// Updates the player's speed while preserving current pitch.
    pub fn update_playback_speed(&self, speed: f32) {
        if self.is_enabled.value() {
            let pitch = self.player.playback_parameters().pitch;
            self.player
                .set_playback_parameters(PlaybackParameters::new(speed, pitch));
        }
    }
}

/// Remembers an event-driven [`PlaybackSpeedState`] for `player`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberPlaybackSpeedState(player: &Player) -> PlaybackSpeedState {
    remember_player_holder(
        player,
        |p| PlaybackSpeedState {
            is_enabled: mutableStateOf(p.is_command_available(PlayerCommand::SetSpeedAndPitch)),
            playback_speed: mutableStateOf(p.playback_parameters().speed),
            player: p,
        },
        |p, event| match event {
            PlayerEvent::PlaybackParametersChanged(_)
            | PlayerEvent::AvailableCommandsChanged(_) => Some((
                p.is_command_available(PlayerCommand::SetSpeedAndPitch),
                p.playback_parameters().speed,
            )),
            _ => None,
        },
        |holder, (enabled, speed)| {
            holder.is_enabled.set(enabled);
            holder.playback_speed.set(speed);
        },
    )
}

/// Unstyled state holder for a repeat-mode toggle button, matching Media3 `RepeatButtonState`.
#[derive(Clone)]
pub struct RepeatButtonState {
    player: Player,
    is_enabled: MutableState<bool>,
    repeat_mode: MutableState<RepeatMode>,
}

impl RepeatButtonState {
    /// Whether changing repeat mode is currently available.
    pub fn is_enabled(&self) -> bool {
        self.is_enabled.value()
    }

    /// Active [`RepeatMode`].
    pub fn repeat_mode(&self) -> RepeatMode {
        self.repeat_mode.value()
    }

    /// Advances repeat mode (`Off -> One -> All -> Off`).
    pub fn on_click(&self) {
        if self.is_enabled.value() {
            self.player
                .set_repeat_mode(self.player.repeat_mode().next());
        }
    }
}

/// Remembers an event-driven [`RepeatButtonState`] for `player`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberRepeatButtonState(player: &Player) -> RepeatButtonState {
    remember_player_holder(
        player,
        |p| RepeatButtonState {
            is_enabled: mutableStateOf(p.is_command_available(PlayerCommand::SetRepeatMode)),
            repeat_mode: mutableStateOf(p.repeat_mode()),
            player: p,
        },
        |p, event| match event {
            PlayerEvent::RepeatModeChanged(_) | PlayerEvent::AvailableCommandsChanged(_) => Some((
                p.is_command_available(PlayerCommand::SetRepeatMode),
                p.repeat_mode(),
            )),
            _ => None,
        },
        |holder, (enabled, mode)| {
            holder.is_enabled.set(enabled);
            holder.repeat_mode.set(mode);
        },
    )
}

/// Unstyled state holder for a shuffle toggle button, matching Media3 `ShuffleButtonState`.
#[derive(Clone)]
pub struct ShuffleButtonState {
    player: Player,
    is_enabled: MutableState<bool>,
    shuffle_on: MutableState<bool>,
}

impl ShuffleButtonState {
    /// Whether toggling shuffle mode is currently available.
    pub fn is_enabled(&self) -> bool {
        self.is_enabled.value()
    }

    /// Whether shuffle mode is currently enabled.
    pub fn shuffle_on(&self) -> bool {
        self.shuffle_on.value()
    }

    /// Toggles shuffle mode.
    pub fn on_click(&self) {
        if self.is_enabled.value() {
            self.player
                .set_shuffle_mode(!self.player.shuffle_mode_enabled());
        }
    }
}

/// Remembers an event-driven [`ShuffleButtonState`] for `player`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberShuffleButtonState(player: &Player) -> ShuffleButtonState {
    remember_player_holder(
        player,
        |p| ShuffleButtonState {
            is_enabled: mutableStateOf(p.is_command_available(PlayerCommand::SetShuffleMode)),
            shuffle_on: mutableStateOf(p.shuffle_mode_enabled()),
            player: p,
        },
        |p, event| match event {
            PlayerEvent::ShuffleModeChanged(_) | PlayerEvent::AvailableCommandsChanged(_) => {
                Some((
                    p.is_command_available(PlayerCommand::SetShuffleMode),
                    p.shuffle_mode_enabled(),
                ))
            }
            _ => None,
        },
        |holder, (enabled, on)| {
            holder.is_enabled.set(enabled);
            holder.shuffle_on.set(on);
        },
    )
}

/// Unstyled state holder for a seek-back button, matching Media3 `SeekBackButtonState`.
#[derive(Clone)]
pub struct SeekBackButtonState {
    player: Player,
    is_enabled: MutableState<bool>,
    seek_back_increment: Duration,
}

impl SeekBackButtonState {
    /// Whether seeking backward is currently available.
    pub fn is_enabled(&self) -> bool {
        self.is_enabled.value()
    }

    /// Configured backward seek increment.
    pub fn seek_back_increment(&self) -> Duration {
        self.seek_back_increment
    }

    /// Seeks backward by [`seek_back_increment`](Self::seek_back_increment).
    pub fn on_click(&self) {
        if self.is_enabled.value() {
            self.player.seek_back();
        }
    }
}

/// Remembers an event-driven [`SeekBackButtonState`] for `player`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberSeekBackButtonState(player: &Player) -> SeekBackButtonState {
    remember_command_holder(player, PlayerCommand::SeekBack, |p, is_enabled| {
        SeekBackButtonState {
            seek_back_increment: p.seek_back_increment(),
            player: p,
            is_enabled,
        }
    })
}

/// Unstyled state holder for a seek-forward button, matching Media3 `SeekForwardButtonState`.
#[derive(Clone)]
pub struct SeekForwardButtonState {
    player: Player,
    is_enabled: MutableState<bool>,
    seek_forward_increment: Duration,
}

impl SeekForwardButtonState {
    /// Whether seeking forward is currently available.
    pub fn is_enabled(&self) -> bool {
        self.is_enabled.value()
    }

    /// Configured forward seek increment.
    pub fn seek_forward_increment(&self) -> Duration {
        self.seek_forward_increment
    }

    /// Seeks forward by [`seek_forward_increment`](Self::seek_forward_increment).
    pub fn on_click(&self) {
        if self.is_enabled.value() {
            self.player.seek_forward();
        }
    }
}

/// Remembers an event-driven [`SeekForwardButtonState`] for `player`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberSeekForwardButtonState(player: &Player) -> SeekForwardButtonState {
    remember_command_holder(player, PlayerCommand::SeekForward, |p, is_enabled| {
        SeekForwardButtonState {
            seek_forward_increment: p.seek_forward_increment(),
            player: p,
            is_enabled,
        }
    })
}

/// Unstyled state holder for a next-item button, matching Media3 `NextButtonState`.
#[derive(Clone)]
pub struct NextButtonState {
    player: Player,
    is_enabled: MutableState<bool>,
}

impl NextButtonState {
    /// Whether advancing to a next playlist item is currently available.
    pub fn is_enabled(&self) -> bool {
        self.is_enabled.value()
    }

    /// Advances to the next playlist item.
    pub fn on_click(&self) {
        if self.is_enabled.value() {
            self.player.seek_to_next();
        }
    }
}

/// Remembers an event-driven [`NextButtonState`] for `player`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberNextButtonState(player: &Player) -> NextButtonState {
    remember_command_holder(player, PlayerCommand::SeekToNext, |p, is_enabled| {
        NextButtonState {
            player: p,
            is_enabled,
        }
    })
}

/// Unstyled state holder for a previous-item button, matching Media3 `PreviousButtonState`.
#[derive(Clone)]
pub struct PreviousButtonState {
    player: Player,
    is_enabled: MutableState<bool>,
}

impl PreviousButtonState {
    /// Whether returning to the start or previous playlist item is currently available.
    pub fn is_enabled(&self) -> bool {
        self.is_enabled.value()
    }

    /// Restarts the active item or jumps to the previous playlist item.
    pub fn on_click(&self) {
        if self.is_enabled.value() {
            self.player.seek_to_previous();
        }
    }
}

/// Remembers an event-driven [`PreviousButtonState`] for `player`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberPreviousButtonState(player: &Player) -> PreviousButtonState {
    remember_command_holder(player, PlayerCommand::SeekToPrevious, |p, is_enabled| {
        PreviousButtonState {
            player: p,
            is_enabled,
        }
    })
}

#[track_caller]
fn remember_command_holder<H>(
    player: &Player,
    command: PlayerCommand,
    build: impl FnOnce(Player, MutableState<bool>) -> H,
) -> H
where
    H: Clone + HasEnabled + 'static,
{
    remember_player_holder(
        player,
        |p| {
            let enabled = mutableStateOf(p.is_command_available(command));
            build(p, enabled)
        },
        move |p, event| {
            matches!(event, PlayerEvent::AvailableCommandsChanged(_))
                .then(|| p.is_command_available(command))
        },
        |holder, enabled| holder.enabled_state().set(enabled),
    )
}

trait HasEnabled {
    fn enabled_state(&self) -> MutableState<bool>;
}

impl HasEnabled for SeekBackButtonState {
    fn enabled_state(&self) -> MutableState<bool> {
        self.is_enabled
    }
}

impl HasEnabled for SeekForwardButtonState {
    fn enabled_state(&self) -> MutableState<bool> {
        self.is_enabled
    }
}

impl HasEnabled for NextButtonState {
    fn enabled_state(&self) -> MutableState<bool> {
        self.is_enabled
    }
}

impl HasEnabled for PreviousButtonState {
    fn enabled_state(&self) -> MutableState<bool> {
        self.is_enabled
    }
}
