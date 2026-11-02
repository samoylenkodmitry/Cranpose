use std::cell::Cell;

use crate::NodeId;

/// A node id or none in one word, where `Cell<Option<NodeId>>` takes two: a
/// layout node keeps three, and the 24 bytes kept it out of jemalloc's
/// 640-byte size class.
pub struct NodeIdCell(Cell<NodeId>);

// The id a cell holds for none: no node gets it, as ids index the applier's
// nodes.
const NONE: NodeId = NodeId(usize::MAX);

impl NodeIdCell {
    /// A cell holding `id`.
    pub const fn new(id: Option<NodeId>) -> Self {
        Self(Cell::new(match id {
            Some(id) => id,
            None => NONE,
        }))
    }

    /// The id the cell holds.
    pub fn get(&self) -> Option<NodeId> {
        // With `then_some`, LLVM compiles the composer's group opening five
        // instructions longer a group.
        Some(self.0.get()).filter(|&id| id != NONE)
    }

    /// Puts `id` in the cell.
    pub fn set(&self, id: Option<NodeId>) {
        debug_assert_ne!(id, Some(NONE), "no node takes the none id");
        self.0.set(id.unwrap_or(NONE));
    }

    /// Puts `id` in the cell and returns the id it held.
    pub fn replace(&self, id: Option<NodeId>) -> Option<NodeId> {
        let previous = self.get();
        self.set(id);
        previous
    }
}
