//! A scene that another machine renders and streams to the headset.

use std::rc::Rc;

use cranpose::{DecodedFrames, prelude::*};

#[cfg(not(target_arch = "wasm32"))]
use crate::flat_stream::FlatStream;
use crate::{StreamStats, StreamedViews, space::local_space, stream_source::StreamSource};

/// A scene that another machine renders for both eyes, streams to the headset and a
/// [`VideoDecoder`](cranpose::VideoDecoder) decodes into `frames`. The headset's compositor
/// shows each frame from the head pose that it was rendered for, and corrects the picture for
/// the head's newer pose. The composable has no Jetpack XR counterpart.
///
/// The name has no `Spatial` prefix, because the `Spatial` composables place content in a
/// [`Subspace`](crate::Subspace), and a streamed scene fills the viewer's whole view. Its
/// picture lies under the panels of the app, and over the room.
///
/// `views` gives the views of a frame from the metadata that the app submitted with the
/// frame's access unit, such as a frame index that maps to a render pose. The frames must come
/// from a decoder whose output is [`DecoderOutput::Frames`](cranpose::DecoderOutput::Frames).
///
/// The headset keeps up to three decoded frames in a queue. Each frame is held by an image of the
/// decoder, so the decoder's `depth` must be at least 5: the queue's three frames, the frame
/// shown last, and one for the decoder to write. At each of its own frames the headset shows the
/// frame whose [`StreamedViews::target_display_time`] lies closest to the predicted display time
/// of the frame that it submits. It never goes back to a frame older than the one shown, and it
/// drops the frames that came before the one it picked. While no waiting frame is closer than the
/// frame shown last, the layer shows that frame's picture again. When the queue is full, the
/// frame shown last no longer competes, so a host whose targets run far ahead cannot freeze the
/// picture. A frame whose views give no target follows the newest-frame rule: the headset shows
/// the newest decoded frame. Frames with views that the compositor cannot show are dropped, and
/// the log says why. `stats` receives how many frames the scene showed, skipped and repeated.
///
/// Outside a headset, in a desktop window or on the screen of a phone or a tablet, the scene
/// fills the box that the layout gives it. It shows the left eye's picture of the newest decoded
/// frame, scaled as [`ContentScale::Fit`](cranpose::ContentScale::Fit) scales it, and shows that
/// picture again while no newer frame comes. It takes the left eye's rectangle from the frame's
/// views and ignores their poses, their fields of view and their target display time. It skips a
/// frame whose views are refused, as the headset does. The headset's compositor unwarps a
/// foveated picture. A flat scene has no unwarp, so it shows such a picture as the host encoded
/// it and logs that once. A host that streams to a flat viewer must not foveate. The decoder's
/// `depth` must be at least 6 there, as for [`PlayerSurface`](cranpose::PlayerSurface), which
/// draws the pictures. The scene draws nothing where the platform has no decoder or no GPU import
/// for its frames, and counts each frame that it takes as skipped.
///
/// Outside a headset, `stats` counts a frame as shown when the scene hands it to the renderer. It
/// counts a frame as skipped when a newer frame replaced it, when the views refused it or when no
/// GPU import was possible. A window draws a frame only when something on it changes, and the
/// system shows the window's last frame again by itself. The scene sees no repeat, so `repeated`
/// stays 0.
#[composable]
pub fn StreamedScene(
    frames: DecodedFrames,
    views: impl Fn(u64) -> StreamedViews + 'static,
    stats: StreamStats,
) {
    let Some(space) = local_space().current() else {
        #[cfg(not(target_arch = "wasm32"))]
        FlatStream(frames, views, stats);
        return;
    };
    let id = remember({
        let space = space.clone();
        move || space.new_stream_id()
    })
    .with(|id| *id);
    space.set_stream(
        id,
        Rc::new(StreamSource {
            frames,
            views: Rc::new(views),
            stats,
        }),
    );
    DisposableEffect(id, move |_| {
        DisposableEffectResult::new(move || space.remove_stream(id))
    });
}
