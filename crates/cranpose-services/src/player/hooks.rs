//! Event-driven Compose reactive hooks for observing [`Player`](super::Player) transitions.

use cranpose_core::{EventStream, State, rememberEventStream};

use super::{Player, PlayerError, PlayerEvent, PlayerState};

/// Observes every [`PlayerEvent`] emitted by `player` as a Compose [`EventStream`].
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberPlayerEvents(player: &Player) -> EventStream<PlayerEvent> {
    let handle = player.clone();
    rememberEventStream(handle.id(), move |sender| {
        handle.observe_events(move |event| sender.send(event))
    })
}

/// Reactive [`PlayerState`] for `player`, recomposing only on state transitions.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberPlayerState(player: &Player) -> State<PlayerState> {
    let handle = player.clone();
    let updates = rememberEventStream(handle.id(), move |sender| {
        handle.observe_events(move |event| {
            if let PlayerEvent::PlaybackStateChanged(state) = event {
                sender.send(state);
            }
        })
    });
    cranpose_core::collectAsState(updates, player.id(), player.playback_state())
}

/// Reactive `is_playing` state for `player`, recomposing only when effective playback flips.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberIsPlaying(player: &Player) -> State<bool> {
    let handle = player.clone();
    let updates = rememberEventStream(handle.id(), move |sender| {
        handle.observe_events(move |event| {
            if let PlayerEvent::IsPlayingChanged(playing) = event {
                sender.send(playing);
            }
        })
    });
    cranpose_core::collectAsState(updates, player.id(), player.is_playing())
}

/// Reactive `is_loading` state for `player`, recomposing only on loading transitions.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberIsLoading(player: &Player) -> State<bool> {
    let handle = player.clone();
    let updates = rememberEventStream(handle.id(), move |sender| {
        handle.observe_events(move |event| {
            if let PlayerEvent::IsLoadingChanged(loading) = event {
                sender.send(loading);
            }
        })
    });
    cranpose_core::collectAsState(updates, player.id(), player.is_loading())
}

/// Reactive fatal error state for `player`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberPlayerError(player: &Player) -> State<Option<PlayerError>> {
    let handle = player.clone();
    let updates = rememberEventStream(handle.id(), move |sender| {
        handle.observe_events(move |event| match event {
            PlayerEvent::Error(error) => sender.send(Some(error)),
            PlayerEvent::PlaybackStateChanged(PlayerState::Buffering | PlayerState::Ready) => {
                sender.send(None);
            }
            _ => {}
        })
    });
    cranpose_core::collectAsState(updates, player.id(), player.player_error())
}
