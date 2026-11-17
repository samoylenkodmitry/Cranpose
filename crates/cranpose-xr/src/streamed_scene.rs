//! A scene that another machine renders and streams to the headset.

use std::rc::Rc;

use cranpose::{DecodedFrames, prelude::*};

use crate::{StreamedViews, space::local_space, stream_source::StreamSource};

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
/// The headset takes the newest frame at each of its own frames, and shows the last one again
/// while no newer frame comes. It drops the frames that it did not show. Frames with views that
/// the compositor cannot show are dropped, and the log says why.
///
/// Outside a headset the composable shows nothing.
#[composable]
pub fn StreamedScene(frames: DecodedFrames, views: impl Fn(u64) -> StreamedViews + 'static) {
    let Some(space) = local_space().current() else {
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
        }),
    );
    DisposableEffect(id, move |_| {
        DisposableEffectResult::new(move || space.remove_stream(id))
    });
}
