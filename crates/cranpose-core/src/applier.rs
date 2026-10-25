use std::any::{Any, TypeId};

use crate::{Node, NodeError, NodeId, RecycledNode, RecycledNodeInsertion};

/// Stores the nodes that a composition builds and applies changes to them. The UI layer supplies an
/// applier; [`MemoryApplier`] is the stock one.
pub trait Applier: Any {
    /// Stores `node` and returns its new id.
    fn create(&mut self, node: Box<dyn Node>) -> NodeId;
    /// Returns the node `id` for change, or an error if there is none.
    fn get_mut(&mut self, id: NodeId) -> Result<&mut dyn Node, NodeError>;
    /// Returns the node `id` to read, or an error if there is none.
    fn get_ref(&self, id: NodeId) -> Result<&dyn Node, NodeError>;
    /// Removes the node `id` and the nodes below it.
    fn remove(&mut self, id: NodeId) -> Result<(), NodeError>;

    /// Opens a batch of commands, in which no layout pass runs: within it a
    /// dirty bubble may stop at an ancestor an earlier one passed
    /// ([`Self::note_bubble_pass`]). Batches nest.
    fn begin_bubble_batch(&mut self) {}

    /// Notes that a dirty bubble marked `id` as needing measure or layout,
    /// or carried the dirt of `id` to its ancestors, for a layout pass that
    /// starts from the dirty nodes. An applier that keeps no notes ignores
    /// it.
    fn note_layout_dirt(&mut self, _id: NodeId) {}

    /// Closes the batch [`Self::begin_bubble_batch`] opened.
    fn end_bubble_batch(&mut self) {}

    /// Forgets what the open batch's bubbles passed: a node changed its
    /// parent, or other code ran, so their marks may no longer reach the root.
    fn forget_bubble_passes(&mut self) {}

    /// Notes that a dirty bubble carrying `flags` passed the ancestor `id`.
    /// `true` when one carrying all of them passed it earlier in the open
    /// batch: that one marked every ancestor above `id`, so this one may
    /// stop. An applier that keeps no notes answers `false`.
    fn note_bubble_pass(&mut self, _id: NodeId, _flags: u8) -> bool {
        false
    }

    /// The node `id` for a dirty bubble to mark, or `None` when it is
    /// missing or, with `note`, when [`Self::note_bubble_pass`] says a
    /// bubble carrying `flags` passed it earlier in the open batch.
    fn bubble_node(&mut self, id: NodeId, flags: u8, note: bool) -> Option<&mut dyn Node> {
        if note && self.note_bubble_pass(id, flags) {
            return None;
        }
        self.get_mut(id).ok()
    }

    /// Records that `parent_id`'s child list changed structurally this frame
    /// (insert, remove, move, or reparent). Incremental scene consumers drain
    /// the recorded parents and re-patch those subtrees so removed nodes are
    /// evicted from a persistent render graph even when the frame's scene
    /// update is otherwise scoped to unrelated dirty nodes.
    fn record_structural_change(&mut self, _parent_id: NodeId) {}

    /// Returns the current generation for a node index.
    /// Generation is incremented when an index is reused from the freelist,
    /// preventing stale slot entries from matching recycled nodes.
    fn node_generation(&self, id: NodeId) -> u32;

    /// Stores `node` as a virtual node, one that lays out nothing itself
    /// while its parent lays out its children, and returns its new id.
    fn create_virtual(&mut self, node: Box<dyn Node>) -> NodeId;

    /// Reinserts a recycled node at its retained stable ID, or creates a fresh ID if that
    /// retained ID is no longer available.
    fn insert_recycled_node_or_create(
        &mut self,
        stable_id: NodeId,
        node: Box<dyn Node>,
    ) -> RecycledNodeInsertion {
        let id = self.create(node);
        RecycledNodeInsertion::fresh(id, Some(NodeError::AlreadyExists { id: stable_id }))
    }

    /// Returns the applier as `Any`, for downcasting to its concrete type.
    fn as_any(&self) -> &dyn Any
    where
        Self: Sized,
    {
        self
    }

    /// Returns the applier as `Any`, for downcasting to its concrete type.
    fn as_any_mut(&mut self) -> &mut dyn Any
    where
        Self: Sized,
    {
        self
    }

    /// Trim trailing tombstones/unused capacity after structural changes.
    fn compact(&mut self) {}

    /// Returns a previously recycled node shell and its stable ID for the requested concrete type.
    fn take_recycled_node(&mut self, _key: TypeId) -> Option<RecycledNode> {
        None
    }

    /// Marks whether a reinserted recycled node originated from the warm recycle path.
    fn set_recycled_node_origin(&mut self, _id: NodeId, _warm_origin: bool) {}

    /// Seeds a warm recyclable shell for future reuse without requiring a prior removal.
    fn seed_recycled_node_shell(
        &mut self,
        _key: TypeId,
        _recycle_pool_limit: Option<usize>,
        _shell: Box<dyn Node>,
    ) {
    }

    /// Records that the current apply pass had to allocate a fresh recyclable shell for this type.
    fn record_fresh_recyclable_creation(&mut self, _key: TypeId) {}

    /// Drops any recyclable shells that should not survive beyond the current apply pass.
    fn clear_recycled_nodes(&mut self) {}
}
