use std::{ops::Deref, rc::Rc};

use cranpose_core::NodeId;

/// The children a subcompose node placed in its last layout, in placement
/// order. Changes through a node handle while this value lives become
/// visible to the next read; this one keeps the children it took.
pub struct ActiveChildren(Rc<Vec<NodeId>>);

impl ActiveChildren {
    pub(super) fn new(placements: Rc<Vec<NodeId>>) -> Self {
        Self(placements)
    }
}

impl Deref for ActiveChildren {
    type Target = [NodeId];

    fn deref(&self) -> &[NodeId] {
        &self.0
    }
}
