//! Event-driven progress state holders that tick only during active playback and stay completely
//! idle while paused, matching Media3 `ProgressStateWithTickInterval` and
//! `ProgressStateWithTickCount`.

use std::time::Duration;

use cranpose_core::{LaunchedEffect, MutableState, delay, mutableStateOf};
use cranpose_services::{Player, PlayerCommand, PlayerEvent, rememberIsPlaying};

use super::sync::remember_player_holder;

const MIN_TICK_INTERVAL: Duration = Duration::from_millis(16);
const DEFAULT_FALLBACK_TICK: Duration = Duration::from_millis(250);

/// Unstyled state holder exposing playback progress and seek actions, matching Media3
/// `ProgressStateWithTickInterval`.
///
/// Updates at `tick_interval` only while the player is actively playing. While paused or idle, no
/// timers or frame callbacks run; explicit seeks and item transitions still update immediately.
#[derive(Clone)]
pub struct ProgressStateWithTickInterval {
    player: Player,
    current_position: MutableState<Duration>,
    buffered_position: MutableState<Duration>,
    duration: MutableState<Option<Duration>>,
    is_seekable: MutableState<bool>,
}

impl ProgressStateWithTickInterval {
    /// Current playback position.
    pub fn current_position(&self) -> Duration {
        self.current_position.value()
    }

    /// Furthest buffered position ahead of the playhead.
    pub fn buffered_position(&self) -> Duration {
        self.buffered_position.value()
    }

    /// Total duration of the active media item, or `None` for unknown/live streams.
    pub fn duration(&self) -> Option<Duration> {
        self.duration.value()
    }

    /// Whether seeking is supported for the active media item.
    pub fn is_seekable(&self) -> bool {
        self.is_seekable.value()
    }

    /// Normalized playback fraction in `[0.0, 1.0]`, or `None` when duration is unknown.
    pub fn fraction(&self) -> Option<f32> {
        fraction_of(self.current_position.value(), self.duration.value()?)
    }

    /// Normalized buffered fraction in `[0.0, 1.0]`, or `None` when duration is unknown.
    pub fn buffered_fraction(&self) -> Option<f32> {
        fraction_of(self.buffered_position.value(), self.duration.value()?)
    }

    /// Seeks the player to `position`.
    pub fn seek_to(&self, position: Duration) {
        if self.is_seekable.value() {
            self.player.seek_to(position);
        }
    }

    /// Seeks the player to `fraction` in `[0.0, 1.0]` of the known duration.
    pub fn seek_to_fraction(&self, fraction: f32) {
        if !self.is_seekable.value() || !fraction.is_finite() {
            return;
        }
        if let Some(duration) = self.duration.value() {
            let clamped = fraction.clamp(0.0, 1.0);
            let target = Duration::from_secs_f64(duration.as_secs_f64() * f64::from(clamped));
            self.player.seek_to(target);
        }
    }

    fn apply_snapshot(&self, snap: ProgressSnap) {
        self.current_position.set(snap.position);
        self.buffered_position.set(snap.buffered);
        self.duration.set(snap.duration);
        self.is_seekable.set(snap.seekable);
    }
}

#[derive(Clone, Copy)]
struct ProgressSnap {
    position: Duration,
    buffered: Duration,
    duration: Option<Duration>,
    seekable: bool,
}

impl ProgressSnap {
    fn from_player(player: &Player) -> Self {
        Self {
            position: player.current_position(),
            buffered: player.buffered_position(),
            duration: player.duration(),
            seekable: player.is_command_available(PlayerCommand::SeekInCurrentMediaItem),
        }
    }
}

/// Remembers a [`ProgressStateWithTickInterval`] updating every `tick_interval` while `player` is
/// actively playing (and performing zero timer ticks while paused or idle).
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberProgressStateWithTickInterval(
    player: &Player,
    tick_interval: Duration,
) -> ProgressStateWithTickInterval {
    let holder = remember_player_holder(
        player,
        |p| {
            let snap = ProgressSnap::from_player(&p);
            ProgressStateWithTickInterval {
                player: p,
                current_position: mutableStateOf(snap.position),
                buffered_position: mutableStateOf(snap.buffered),
                duration: mutableStateOf(snap.duration),
                is_seekable: mutableStateOf(snap.seekable),
            }
        },
        |p, event| match event {
            PlayerEvent::PositionDiscontinuity { .. }
            | PlayerEvent::MediaItemTransition { .. }
            | PlayerEvent::PlaybackStateChanged(_)
            | PlayerEvent::IsPlayingChanged(_)
            | PlayerEvent::AvailableCommandsChanged(_) => Some(ProgressSnap::from_player(p)),
            _ => None,
        },
        ProgressStateWithTickInterval::apply_snapshot,
    );

    let playing = rememberIsPlaying(player).value();
    let step = tick_interval.max(MIN_TICK_INTERVAL);
    let ticker_holder = holder.clone();
    let ticker_player = player.clone();
    LaunchedEffect((player.id(), playing, step), async move |_scope| {
        if !playing {
            return;
        }
        loop {
            delay(step).await;
            ticker_holder.apply_snapshot(ProgressSnap::from_player(&ticker_player));
        }
    });

    holder
}

/// Remembers a [`ProgressStateWithTickInterval`] whose tick interval divides the active item's
/// duration into `total_ticks` steps (falling back to 250 ms for live or unknown-duration streams).
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberProgressStateWithTickCount(
    player: &Player,
    total_ticks: u32,
) -> ProgressStateWithTickInterval {
    let tick_interval = match (player.duration(), total_ticks) {
        (Some(duration), ticks) if ticks > 0 && !duration.is_zero() => {
            (duration / ticks).max(MIN_TICK_INTERVAL)
        }
        _ => DEFAULT_FALLBACK_TICK,
    };
    rememberProgressStateWithTickInterval(player, tick_interval)
}

fn fraction_of(position: Duration, duration: Duration) -> Option<f32> {
    if duration.is_zero() {
        return Some(0.0);
    }
    Some((position.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0))
}
