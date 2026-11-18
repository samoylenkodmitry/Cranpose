//! What a streamed scene gives the headset's backend.

use std::rc::Rc;

use cranpose::DecodedFrames;

use crate::{StreamStats, StreamedViews};

// The frames of a scene and the function that says each frame's views.
#[cfg_attr(
    all(target_arch = "wasm32", not(test)),
    expect(
        dead_code,
        reason = "a web page has no decoder, so no flat scene reads it"
    )
)]
pub(crate) struct StreamSource {
    pub(crate) frames: DecodedFrames,
    pub(crate) views: Rc<dyn Fn(u64) -> StreamedViews>,
    pub(crate) stats: StreamStats,
}
