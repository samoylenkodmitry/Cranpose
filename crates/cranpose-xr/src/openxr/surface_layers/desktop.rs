//! The surface panels on a desktop: they have no layers.

use crate::{
    openxr::session::{QuadLayer, XrSession},
    space::{PanelSlot, Space},
};

// A desktop has no surfaces of an external producer: only an Android headset's compositor shows
// one. A surface panel takes its place in the room and shows nothing.
pub(crate) struct SurfaceLayers;

impl SurfaceLayers {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn sync(&mut self, _xr_session: &XrSession, _space: &Space, _panels: &[PanelSlot]) {}

    pub(crate) fn layers<'a>(
        &'a self,
        _panels: &'a [PanelSlot],
    ) -> impl Iterator<Item = QuadLayer<'a>> + use<'a> {
        std::iter::empty()
    }
}
