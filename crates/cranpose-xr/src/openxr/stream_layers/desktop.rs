//! The streamed scenes on a desktop: they have no layers.

use crate::{
    openxr::{
        graphics::Graphics,
        session::{ProjectionLayer, XrSession},
    },
    space::Space,
};

// A streamed scene shows the frames of a hardware decoder, which an Android headset imports into
// GL as an EGL image from an `AHardwareBuffer`. A desktop headset would import the platform's
// decoded frames into Vulkan, which is not written yet, and no decoder gives a desktop frames
// now. A streamed scene takes its place in the room and shows nothing.
pub(crate) struct StreamLayers;

impl StreamLayers {
    pub(crate) fn new(_graphics: &Graphics, _xr_session: &XrSession) -> Self {
        Self
    }

    pub(crate) fn skip(&self, _space: &Space) {}

    pub(crate) fn update(&mut self, _xr_session: &XrSession, _space: &Space) {}

    pub(crate) fn layers(&self) -> Vec<ProjectionLayer<'_>> {
        Vec::new()
    }
}
