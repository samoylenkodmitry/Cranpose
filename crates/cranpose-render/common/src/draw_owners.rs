// Which layout nodes a store draws, as its journal tells the changes: a node
// draws through its layer or leaf record, and stops when an update removes
// that record and gives the node no new one.

use cranpose_core::NodeId;

use crate::scene_store::{JournalCursor, JournalEntry, JournalRead, RecordKind, SceneStore};

/// A layout node a scene started or stopped drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawOwnerChange {
    /// The scene draws the node.
    Joined(NodeId),
    /// The scene no longer draws the node.
    Left(NodeId),
}

impl DrawOwnerChange {
    /// The node the change is about.
    pub fn node(self) -> NodeId {
        match self {
            Self::Joined(node) | Self::Left(node) => node,
        }
    }
}

pub(crate) struct DrawOwners {
    cursor: JournalCursor,
    // The nodes one update gave a record, sorted.
    joined_in_update: Vec<NodeId>,
}

impl DrawOwners {
    pub(crate) fn new(store: &mut SceneStore) -> Self {
        Self {
            cursor: store.open_journal(),
            joined_in_update: Vec::new(),
        }
    }

    // Adds to `changes` the nodes `store` started or stopped drawing since
    // the last read, one change a node, as its latest update left it.
    // `false` when the store cannot tell.
    pub(crate) fn read(
        &mut self,
        store: &mut SceneStore,
        changes: &mut Vec<DrawOwnerChange>,
    ) -> bool {
        let JournalRead::Changes(entries) = store.read_journal(&mut self.cursor) else {
            return false;
        };
        let first = changes.len();
        for update in entries.chunk_by(|a, b| a.update == b.update) {
            self.joined_in_update.clear();
            self.joined_in_update.extend(
                update
                    .iter()
                    .filter(|entry| entry.event.joins())
                    .filter_map(drawn_node),
            );
            self.joined_in_update.sort_unstable();
            self.joined_in_update.dedup();
            changes.extend(
                update
                    .iter()
                    .filter(|entry| entry.event.leaves())
                    .filter_map(drawn_node)
                    .filter(|node| self.joined_in_update.binary_search(node).is_err())
                    .map(DrawOwnerChange::Left),
            );
            changes.extend(
                self.joined_in_update
                    .iter()
                    .copied()
                    .map(DrawOwnerChange::Joined),
            );
        }
        // The latest change of each node stands.
        let read = &mut changes[first..];
        read.sort_by_key(|change| change.node());
        let mut kept = first;
        for index in first..changes.len() {
            if kept > first && changes[kept - 1].node() == changes[index].node() {
                changes[kept - 1] = changes[index];
            } else {
                changes[kept] = changes[index];
                kept += 1;
            }
        }
        changes.truncate(kept);
        true
    }
}

// The node the record of `entry` draws, when it is the node's own record:
// a wrapper or a draw group of a node is not.
fn drawn_node(entry: &JournalEntry) -> Option<NodeId> {
    match entry.kind {
        RecordKind::Layer | RecordKind::Leaf => entry.node,
        RecordKind::Wrapper | RecordKind::DrawGroup => None,
    }
}
