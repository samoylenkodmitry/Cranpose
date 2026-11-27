//! Event-driven track selection and subtitle cue state holders for Compose UIs.

use cranpose_core::{MutableState, mutableStateOf};
use cranpose_services::{
    CueGroup, Player, PlayerCommand, PlayerEvent, TrackSelectionParameters, TrackType, Tracks,
};

use super::sync::remember_player_holder;

/// Unstyled state holder for runtime video quality, audio language, and subtitle track selection.
#[derive(Clone)]
pub struct TracksState {
    player: Player,
    is_enabled: MutableState<bool>,
    tracks: MutableState<Tracks>,
    selection: MutableState<TrackSelectionParameters>,
}

impl TracksState {
    /// Whether runtime track selection is currently available.
    pub fn is_enabled(&self) -> bool {
        self.is_enabled.value()
    }

    /// Snapshot of all discovered tracks and current selections.
    pub fn tracks(&self) -> Tracks {
        self.tracks.value()
    }

    /// Active track selection constraints and explicit overrides.
    pub fn selection(&self) -> TrackSelectionParameters {
        self.selection.value()
    }

    /// Replaces the player's [`TrackSelectionParameters`].
    pub fn select_parameters(&self, parameters: TrackSelectionParameters) {
        if !self.is_enabled.value() {
            return;
        }
        self.player
            .set_track_selection_parameters(parameters.clone());
        self.selection.set(parameters);
    }

    /// Sets the preferred audio BCP-47 language tag (for example `"en"`, `"ja"`).
    pub fn set_preferred_audio_language(&self, language: impl Into<String>) {
        let updated = self
            .player
            .track_selection_parameters()
            .with_preferred_audio_language(language);
        self.select_parameters(updated);
    }

    /// Sets the preferred subtitle/caption BCP-47 language tag and enables text tracks.
    pub fn set_preferred_text_language(&self, language: impl Into<String>) {
        let updated = self
            .player
            .track_selection_parameters()
            .with_enabled_type(TrackType::Text)
            .with_preferred_text_language(language);
        self.select_parameters(updated);
    }

    /// Enables or disables subtitle/caption rendering.
    pub fn set_subtitles_enabled(&self, enabled: bool) {
        let base = self.player.track_selection_parameters();
        let updated = if enabled {
            base.with_enabled_type(TrackType::Text)
        } else {
            base.with_disabled_type(TrackType::Text)
        };
        self.select_parameters(updated);
    }

    /// Caps selected adaptive video renditions to `width` x `height` pixels.
    pub fn set_max_video_resolution(&self, width: u32, height: u32) {
        let updated = self
            .player
            .track_selection_parameters()
            .with_max_video_size(width, height);
        self.select_parameters(updated);
    }
}

/// Remembers an event-driven [`TracksState`] for `player`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberTracksState(player: &Player) -> TracksState {
    remember_player_holder(
        player,
        |p| TracksState {
            is_enabled: mutableStateOf(
                p.is_command_available(PlayerCommand::SetTrackSelectionParameters),
            ),
            tracks: mutableStateOf(p.current_tracks()),
            selection: mutableStateOf(p.track_selection_parameters()),
            player: p,
        },
        |p, event| match event {
            PlayerEvent::TracksChanged(_)
            | PlayerEvent::MediaItemTransition { .. }
            | PlayerEvent::AvailableCommandsChanged(_) => Some((
                p.is_command_available(PlayerCommand::SetTrackSelectionParameters),
                p.current_tracks(),
                p.track_selection_parameters(),
            )),
            _ => None,
        },
        |holder, (enabled, tracks, selection)| {
            holder.is_enabled.set(enabled);
            holder.tracks.set(tracks);
            holder.selection.set(selection);
        },
    )
}

/// Unstyled state holder delivering active subtitle and closed-caption [`CueGroup`]s so
/// applications render captions with standard Cranpose composables.
#[derive(Clone, Copy)]
pub struct CuesState {
    cues: MutableState<CueGroup>,
}

impl CuesState {
    /// Currently active subtitle/caption cues.
    pub fn cues(&self) -> CueGroup {
        self.cues.value()
    }

    /// Whether no subtitle cues are currently active.
    pub fn is_empty(&self) -> bool {
        self.cues.value().is_empty()
    }
}

/// Remembers an event-driven [`CuesState`] for `player`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberCuesState(player: &Player) -> CuesState {
    remember_player_holder(
        player,
        |p| CuesState {
            cues: mutableStateOf(p.current_cues()),
        },
        |p, event| match event {
            PlayerEvent::CuesChanged(cues) => Some(cues.clone()),
            PlayerEvent::MediaItemTransition { .. } => Some(p.current_cues()),
            _ => None,
        },
        |holder, cues| holder.cues.set(cues),
    )
}
