//! What a streamed scene gives the headset's backend.

use std::rc::Rc;

use cranpose::DecodedFrames;

use crate::StreamedViews;

// The frames of a scene and the function that says each frame's views.
pub(crate) struct StreamSource {
    #[cfg_attr(
        not(all(headset, target_os = "android")),
        expect(dead_code, reason = "the headset backend reads it")
    )]
    pub(crate) frames: DecodedFrames,
    #[cfg_attr(
        not(all(headset, target_os = "android")),
        expect(dead_code, reason = "the headset backend reads it")
    )]
    pub(crate) views: Rc<dyn Fn(u64) -> StreamedViews>,
}
