use cranpose_core::{Node, NodeId};

use crate::ordered_parent::OrderedParent;

// An `OrderedParent` that tracks its own parent, so it can sit inside
// another parent: a structural change below it marks its ancestors, and a
// removal finds the parent to leave.
#[derive(Default)]
pub(crate) struct TrackedParent {
    pub(crate) inner: OrderedParent,
    parent: Option<NodeId>,
}

impl Node for TrackedParent {
    fn insert_child(&mut self, child: NodeId) -> bool {
        self.inner.insert_child(child)
    }

    fn remove_child(&mut self, child: NodeId) -> bool {
        self.inner.remove_child(child)
    }

    fn move_child(&mut self, from: usize, to: usize) {
        self.inner.move_child(from, to);
    }

    fn collect_children_into(&self, out: &mut smallvec::SmallVec<[NodeId; 8]>) {
        self.inner.collect_children_into(out);
    }

    fn collect_owned_children_into(&self, out: &mut smallvec::SmallVec<[NodeId; 8]>) {
        self.inner.collect_owned_children_into(out);
    }

    fn owned_child_index(&self, child: NodeId) -> Option<usize> {
        self.inner.owned_child_index(child)
    }

    fn on_attached_to_parent(&mut self, parent: NodeId) {
        self.parent = Some(parent);
    }

    fn on_removed_from_parent(&mut self) {
        self.parent = None;
    }

    fn parent(&self) -> Option<NodeId> {
        self.parent
    }
}
