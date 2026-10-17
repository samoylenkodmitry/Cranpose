//! The nodes the layout dirty bubbles started from since the last layout
//! pass.
//!
//! A bubble marks a node as needing measure or layout and its ancestors as
//! having a dirty descendant. A pass that walks down from the root finds the
//! node through those marks; a pass that starts from the dirty nodes reads
//! them here. The list keeps a bounded number of nodes: past it, a pass walks
//! down from the root.

use crate::NodeId;

/// The most nodes the list keeps between passes.
const CAPACITY: usize = 32;

#[derive(Default)]
pub(crate) struct LayoutDirtRoots {
    ids: Vec<NodeId>,
    overflowed: bool,
}

impl LayoutDirtRoots {
    /// Notes that a bubble started from `id`.
    pub(crate) fn note(&mut self, id: NodeId) {
        if self.overflowed || self.ids.contains(&id) {
            return;
        }
        if self.ids.len() == CAPACITY {
            self.ids.clear();
            self.overflowed = true;
            return;
        }
        self.ids.push(id);
    }

    /// Moves the noted nodes into `out` and forgets them. `false` when more
    /// were noted than the list keeps; `out` is then empty.
    pub(crate) fn take_into(&mut self, out: &mut Vec<NodeId>) -> bool {
        out.clear();
        std::mem::swap(out, &mut self.ids);
        !std::mem::take(&mut self.overflowed)
    }
}
