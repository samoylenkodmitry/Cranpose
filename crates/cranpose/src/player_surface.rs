//! `PlayerSurface`: zero-copy GPU video pictures drawn in Compose layout for a [`Player`] or
//! raw [`DecodedFrames`].

use cranpose_core::{
    CollectEvents, MutableState, mutableStateOf, rememberEventStream, rememberKeyed,
    rememberMutableStateOf,
};
use cranpose_services::{DecodedFrames, Player, PlayerEvent};
use cranpose_ui::{
    Alignment, Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, ContentScale, Image,
    Modifier, Painter, composable,
};

/// Source of zero-copy GPU video frames rendered by [`PlayerSurface`].
#[derive(Clone, PartialEq)]
pub enum VideoSurfaceSource {
    /// High-level [`Player`] whose active zero-copy GPU frame stream follows clip transitions.
    Player(Player),
    /// Low-level [`DecodedFrames`] queue from a [`VideoDecoder`](crate::VideoDecoder).
    Frames(DecodedFrames),
}

impl From<Player> for VideoSurfaceSource {
    fn from(player: Player) -> Self {
        Self::Player(player)
    }
}

impl From<DecodedFrames> for VideoSurfaceSource {
    fn from(frames: DecodedFrames) -> Self {
        Self::Frames(frames)
    }
}

/// Renders zero-copy GPU video frames from a [`Player`] or raw [`DecodedFrames`] inside layout,
/// scaled by `content_scale`, matching Media3's `PlayerSurface`.
///
/// The surface presents each newest decoded picture on the GPU without CPU pixel copies, holds
/// the last presented picture while paused, and waits on decoder/player events so idle or paused
/// frames do zero work. When given a [`Player`], it automatically switches streams across playlist
/// transitions and marks the player's first rendered frame so [`ContentFrame`](crate::ContentFrame)
/// shutters dismiss on the exact frame the first GPU picture appears.
///
/// On CPU framebuffer targets (such as watchOS and `renderer-pixels`) or platforms without a GPU
/// video importer (`video_surface_supported() == false`), the surface draws nothing and keeps its
/// layout size.
#[composable]
pub fn PlayerSurface<S: Into<VideoSurfaceSource> + Clone + PartialEq + 'static>(
    source: S,
    modifier: Modifier,
    content_scale: ContentScale,
) {
    match source.into() {
        VideoSurfaceSource::Frames(frames) => {
            let shown = rememberMutableStateOf(|| None::<Painter>);
            pump(frames, None, shown);
            Picture(shown, modifier, content_scale);
        }
        VideoSurfaceSource::Player(player) => {
            PlayerBoundSurface(player, modifier, content_scale);
        }
    }
}

/// Tracks the active zero-copy [`DecodedFrames`] queue produced by `player`, updating
/// automatically across playlist item transitions and playback state changes.
///
/// Use this composable when feeding a [`Player`]'s decoded frames into a custom
/// [`PlayerSurfaceIntake`](crate::PlayerSurfaceIntake) pipeline.
#[composable]
pub fn rememberPlayerDecodedFrames(player: &Player) -> Option<DecodedFrames> {
    let active_frames = rememberKeyed(player.id(), |_| mutableStateOf(player.decoded_frames()));
    let handle = player.clone();
    let updates = rememberEventStream(player.id(), move |sender| {
        let p = handle.clone();
        handle.observe_events(move |event| {
            if matches!(
                event,
                PlayerEvent::VideoFramesChanged
                    | PlayerEvent::MediaItemTransition { .. }
                    | PlayerEvent::PlaybackStateChanged(_)
            ) {
                sender.send(p.decoded_frames());
            }
        })
    });
    CollectEvents(updates, player.id(), move |frames| {
        active_frames.set(frames);
    });
    active_frames.value()
}

#[composable]
fn PlayerBoundSurface(player: Player, modifier: Modifier, content_scale: ContentScale) {
    let shown = rememberMutableStateOf(|| None::<Painter>);
    let active_frames = rememberPlayerDecodedFrames(&player);
    if let Some(frames) = active_frames {
        pump(frames, Some(player), shown);
    } else if shown.value().is_some() {
        shown.set(None);
    }
    Picture(shown, modifier, content_scale);
}

// Draws the picture that `shown` holds, or an empty box of the same size.
#[composable]
pub(crate) fn Picture(
    shown: MutableState<Option<Painter>>,
    modifier: Modifier,
    content_scale: ContentScale,
) {
    BoxWithConstraints(modifier, move |scope| {
        let constraints = scope.constraints();
        let bounded = constraints.max_width.is_finite() && constraints.max_height.is_finite();
        let fill = if bounded {
            Modifier::empty().fill_max_size()
        } else {
            Modifier::empty()
        };
        if let Some(painter) = shown.value() {
            Image(
                painter,
                None,
                fill,
                Alignment::CENTER,
                content_scale,
                1.0,
                None,
            );
        } else {
            Box(fill, BoxSpec::default(), || {});
        }
    });
}

// Takes each picture that the decoder outputs and puts its painter in `shown`.
#[composable]
fn pump(frames: DecodedFrames, player: Option<Player>, shown: MutableState<Option<Painter>>) {
    use crate::player_surface_intake::pump_with;

    pump_with(frames, shown, move |frames, presenter| {
        // A picture that is not shown drops, and the last one stays on the screen
        if let Some(frame) = frames.latest_frame()
            && presenter.present(frame, None).is_ok()
            && let Some(player) = &player
        {
            player.notify_rendered_first_frame();
        }
    });
}
