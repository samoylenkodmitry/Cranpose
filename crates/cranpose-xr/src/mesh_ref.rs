//! Which mesh of a volume's content a pointer meets.

use cranpose_core::NodeId;

// A mesh of a volume's content: the node whose layer carries it, and which of the layer's meshes it
// is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MeshRef {
    pub(crate) node: NodeId,
    pub(crate) index: usize,
}
