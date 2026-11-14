use std::cell::Cell;

use crate::{NodeId, PackedNodeId};

/// A node id or none in one word, where `Cell<Option<NodeId>>` takes two: a
/// layout node keeps three, and the 24 bytes kept it out of jemalloc's
/// 640-byte size class.
pub struct NodeIdCell(Cell<PackedNodeId>);

impl NodeIdCell {
    /// A cell holding `id`.
    pub const fn new(id: Option<NodeId>) -> Self {
        Self(Cell::new(PackedNodeId::new(id)))
    }

    /// The id the cell holds.
    #[inline]
    pub fn get(&self) -> Option<NodeId> {
        self.0.get().get()
    }

    /// Puts `id` in the cell.
    pub fn set(&self, id: Option<NodeId>) {
        self.0.set(PackedNodeId::from(id));
    }

    /// Puts `id` in the cell and returns the id it held.
    pub fn replace(&self, id: Option<NodeId>) -> Option<NodeId> {
        let previous = self.get();
        self.set(id);
        previous
    }
}
