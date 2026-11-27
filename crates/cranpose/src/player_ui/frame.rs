//! [`rememberPlayer`], [`PresentationState`], and [`ContentFrame`] shutter composables.

use cranpose_core::{DisposableEffect, MutableState, mutableStateOf, remember};
use cranpose_services::{Player, PlayerBuilder, PlayerEvent, PlayerState, VideoSize};
use cranpose_ui::{Alignment, Box, BoxSpec, ContentScale, Modifier, Size, composable};

use super::sync::remember_player_holder;
use crate::PlayerSurface;

/// Creates and remembers a default [`Player`] scoped to this composition, automatically calling
/// [`Player::release`] when the composable leaves the tree.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberPlayer() -> Player {
    rememberPlayerWith(|builder| builder)
}

/// Creates and remembers a configured [`Player`] scoped to this composition, automatically calling
/// [`Player::release`] when the composable leaves the tree.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberPlayerWith(configure: impl FnOnce(PlayerBuilder) -> PlayerBuilder) -> Player {
    let player = remember(|| {
        configure(Player::builder()).build().unwrap_or_else(|err| {
            log::warn!("cranpose: rememberPlayer backend creation failed: {err}");
            Player::builder()
                .build()
                .expect("fallback headless player builds without error")
        })
    })
    .with(Clone::clone);
    let handle = player.clone();
    DisposableEffect(player.id(), move |scope| {
        scope.on_dispose(move || handle.release())
    });
    player
}

/// Reactive presentation metadata for [`ContentFrame`] and custom video layouts, matching
/// `androidx.media3.ui.compose.state.PresentationState`.
#[derive(Clone, Copy)]
pub struct PresentationState {
    video_size_dp: MutableState<Option<Size>>,
    cover_surface: MutableState<bool>,
}

impl PresentationState {
    /// Display size of the active video accounting for pixel aspect ratio and rotation, or `None`
    /// before dimensions are known.
    pub fn video_size_dp(&self) -> Option<Size> {
        self.video_size_dp.value()
    }

    /// Whether a shutter/poster should cover the video surface until the first frame of the active
    /// media item renders.
    pub fn cover_surface(&self) -> bool {
        self.cover_surface.value()
    }

    /// Display aspect ratio (`width / height`) of the active video, if known.
    pub fn aspect_ratio(&self) -> Option<f32> {
        let size = self.video_size_dp.value()?;
        (size.width > 0.0 && size.height > 0.0).then_some(size.width / size.height)
    }
}

/// Remembers an event-driven [`PresentationState`] for `player`, recomposing readers only when
/// video size changes or the shutter opens/closes.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberPresentationState(player: &Player) -> PresentationState {
    remember_player_holder(
        player,
        |p| PresentationState {
            video_size_dp: mutableStateOf(video_size_to_dp(p.video_size())),
            cover_surface: mutableStateOf(!p.rendered_first_frame()),
        },
        |p, event| match event {
            PlayerEvent::VideoSizeChanged(size) => {
                Some((video_size_to_dp(*size), !p.rendered_first_frame()))
            }
            PlayerEvent::RenderedFirstFrame => Some((video_size_to_dp(p.video_size()), false)),
            PlayerEvent::MediaItemTransition { .. }
            | PlayerEvent::PlaybackStateChanged(PlayerState::Idle) => {
                Some((video_size_to_dp(p.video_size()), true))
            }
            _ => None,
        },
        |holder, (size, cover)| {
            holder.video_size_dp.set(size);
            holder.cover_surface.set(cover);
        },
    )
}

/// Renders a [`Player`]'s zero-copy GPU video surface and covers it with `shutter` (poster or
/// placeholder) until the first GPU video frame of the active media item renders, matching
/// `androidx.media3.ui.compose.ContentFrame`.
#[composable]
pub fn ContentFrame(
    player: Player,
    modifier: Modifier,
    content_scale: ContentScale,
    shutter: impl Fn() + 'static,
) {
    let presentation = rememberPresentationState(&player);
    Box(
        modifier,
        BoxSpec::new().content_alignment(Alignment::CENTER),
        move || {
            PlayerSurface(
                player.clone(),
                Modifier::empty().fill_max_size(),
                content_scale,
            );
            if presentation.cover_surface() {
                shutter();
            }
        },
    );
}

fn video_size_to_dp(size: VideoSize) -> Option<Size> {
    if size.width == 0 || size.height == 0 {
        return None;
    }
    let par = if size.pixel_width_height_ratio.is_finite() && size.pixel_width_height_ratio > 0.0 {
        size.pixel_width_height_ratio
    } else {
        1.0
    };
    let (w, h) = if size.unapplied_rotation_degrees % 180 == 90 {
        (size.height as f32, size.width as f32 * par)
    } else {
        (size.width as f32 * par, size.height as f32)
    };
    Some(Size::new(w, h))
}
