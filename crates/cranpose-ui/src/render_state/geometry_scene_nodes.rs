//! The nodes whose geometry a layout pass changed, as the scene takes them.

use cranpose_core::NodeId;

/// The nodes whose geometry a layout pass changed, by what the scene redoes
/// for them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeometrySceneNodes {
    /// Nodes that changed size or were placed anew: the scene draws them
    /// again.
    pub reshaped: Vec<NodeId>,
    /// Nodes that moved in their parent: the scene moves what it drew for
    /// them, unless they are also reshaped or drawn again.
    pub moved: Vec<MovedSceneNode>,
}

/// A node a layout pass moved, with the node whose placement put it there:
/// its nearest ancestor that is not virtual, the parent of its layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MovedSceneNode {
    /// The node that moved.
    pub node: NodeId,
    /// The node that placed it; `None` for a root.
    pub placer: Option<NodeId>,
}

impl GeometrySceneNodes {
    /// Whether the pass changed no node's geometry.
    pub fn is_empty(&self) -> bool {
        self.reshaped.is_empty() && self.moved.is_empty()
    }
}
