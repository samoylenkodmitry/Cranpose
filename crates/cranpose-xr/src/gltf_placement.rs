//! Where a model's units stand in the content space that holds it.

#[cfg(any(test, headset))]
use cranpose::MutableState;
use cranpose::space::Mat4;

// The map from a model's glTF units to the content space of the node that draws it.
#[derive(Clone, PartialEq)]
pub(crate) enum GltfPlacement {
    // A map that stays: a model in the box of a volume
    Fixed(Mat4),
    // A map that a state holds, which the draw reads, so a model that moves composes nothing: a
    // controller's model, which the frame loop moves with the controller
    #[cfg(any(test, headset))]
    Moving(MutableState<Mat4>),
}

impl GltfPlacement {
    // The map now. A draw that calls it draws again when a moving map changes.
    pub(crate) fn map(&self) -> Mat4 {
        match self {
            Self::Fixed(map) => *map,
            #[cfg(any(test, headset))]
            Self::Moving(state) => state.get(),
        }
    }
}
