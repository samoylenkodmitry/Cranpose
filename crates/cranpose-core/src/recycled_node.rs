use crate::{Node, NodeError, NodeId};

/// A removed node kept in a pool with its old id, so that a later node of the same type can reuse
/// it.
pub struct RecycledNode {
    stable_id: NodeId,
    pub(crate) node: Box<dyn Node>,
    warm_origin: bool,
}

impl RecycledNode {
    /// A removed node as a pool shell: the fresh box it rehouses into, or
    /// the node itself cleared in place when it offers none. A rehoused
    /// node is dropped as it is, so it is not cleared first.
    pub(crate) fn new(stable_id: NodeId, mut node: Box<dyn Node>, warm_origin: bool) -> Self {
        let node = node.rehouse_for_recycle().unwrap_or_else(|| {
            node.prepare_for_recycle();
            node
        });
        Self {
            stable_id,
            node,
            warm_origin,
        }
    }

    pub(crate) fn from_shell(stable_id: NodeId, node: Box<dyn Node>, warm_origin: bool) -> Self {
        Self {
            stable_id,
            node,
            warm_origin,
        }
    }

    /// Returns the id the node had before it was recycled.
    pub fn stable_id(&self) -> NodeId {
        self.stable_id
    }

    pub(crate) fn warm_origin(&self) -> bool {
        self.warm_origin
    }

    pub(crate) fn set_warm_origin(&mut self, warm_origin: bool) {
        self.warm_origin = warm_origin;
    }

    /// Returns the pooled node, so that it can be reset before reuse.
    pub fn node_mut(&mut self) -> &mut dyn Node {
        self.node.as_mut()
    }

    /// Splits the shell into its stable id and its node, with a flag that tells whether it came
    /// from the warm recycle path.
    pub fn into_parts(self) -> (NodeId, Box<dyn Node>, bool) {
        (self.stable_id, self.node, self.warm_origin)
    }
}

/// The result of putting a recycled node back into an applier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecycledNodeInsertion {
    /// The id the node has now.
    pub id: NodeId,
    /// Whether the node got its old stable id back.
    pub stable_id_reused: bool,
    /// The error that forced a new id, or `None` if the old id was reused.
    pub fallback_error: Option<NodeError>,
}

impl RecycledNodeInsertion {
    pub(crate) fn reused(stable_id: NodeId) -> Self {
        Self {
            id: stable_id,
            stable_id_reused: true,
            fallback_error: None,
        }
    }

    pub(crate) fn fresh(id: NodeId, fallback_error: Option<NodeError>) -> Self {
        Self {
            id,
            stable_id_reused: false,
            fallback_error,
        }
    }
}
