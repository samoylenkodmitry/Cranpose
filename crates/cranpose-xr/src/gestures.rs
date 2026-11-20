//! What the pointers of both hands did in one frame.

use crate::{
    space::{PanelMove, PanelResize},
    surface_input::{HandGesture, SurfaceGesture},
};

// The frame's gestures for the composition and for the surfaces, the steps of the carries and the
// steps of the resizes. The pointers fill them each frame, and the frame loop reads them. They keep
// their storage, so a frame does not allocate.
#[derive(Debug, Default)]
pub(crate) struct Gestures {
    pub(crate) atlas: Vec<HandGesture>,
    pub(crate) surfaces: Vec<SurfaceGesture>,
    pub(crate) moves: Vec<PanelMove>,
    pub(crate) resizes: Vec<PanelResize>,
}

impl Gestures {
    pub(crate) fn clear(&mut self) {
        self.atlas.clear();
        self.surfaces.clear();
        self.moves.clear();
        self.resizes.clear();
    }
}
