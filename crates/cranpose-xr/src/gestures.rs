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
    // The hands whose press took or ended in this frame, left then right, each of which gets a
    // light vibration
    pub(crate) ticks: [bool; 2],
}

impl Gestures {
    pub(crate) fn clear(&mut self) {
        self.atlas.clear();
        self.surfaces.clear();
        self.moves.clear();
        self.resizes.clear();
        self.ticks = [false; 2];
    }
}
