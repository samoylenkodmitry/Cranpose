//! A node id or none in one word.

use crate::NodeId;

/// A node id or none in one word, where `Option<NodeId>` takes two: no node
/// takes the id `usize::MAX`, as ids index the applier's nodes, and the
/// value keeps it for none.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PackedNodeId(NodeId);

// The id that stands for none.
const NONE: NodeId = NodeId(usize::MAX);

impl PackedNodeId {
    /// No node.
    pub const NONE: Self = Self(NONE);

    /// `id` in one word.
    pub const fn new(id: Option<NodeId>) -> Self {
        match id {
            Some(id) => Self(id),
            None => Self(NONE),
        }
    }

    /// The id, `None` for no node.
    #[inline]
    pub fn get(self) -> Option<NodeId> {
        // With `then_some`, LLVM compiles the composer's group opening five
        // instructions longer a group.
        Some(self.0).filter(|&id| id != NONE)
    }
}

impl From<Option<NodeId>> for PackedNodeId {
    fn from(id: Option<NodeId>) -> Self {
        debug_assert_ne!(id, Some(NONE), "no node takes the none id");
        Self::new(id)
    }
}

impl std::fmt::Debug for PackedNodeId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.get().fmt(formatter)
    }
}
