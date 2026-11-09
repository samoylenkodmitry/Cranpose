// Which layout nodes a store draws, as its journal tells the changes: a node
// draws through its layer or leaf record, and stops when an update removes
// that record and gives the node no new one.

use cranpose_core::NodeId;

use crate::scene_store::{
    JournalCursor, JournalEntry, JournalRead, RecordEvent, RecordEvents, RecordKind, SceneStore,
};

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

/// Puts into `left`, sorted, the nodes that left a scene and joined none
/// among `changes`, the changes several scenes read for one frame: a node
/// one scene left and another joined is drawn. `joined` is room for the
/// nodes that joined.
pub fn undrawn_nodes(
    changes: &[DrawOwnerChange],
    left: &mut Vec<NodeId>,
    joined: &mut Vec<NodeId>,
) {
    left.clear();
    joined.clear();
    for &change in changes {
        match change {
            DrawOwnerChange::Left(node) => left.push(node),
            DrawOwnerChange::Joined(node) => joined.push(node),
        }
    }
    left.sort_unstable();
    left.dedup();
    joined.sort_unstable();
    left.retain(|node| joined.binary_search(node).is_err());
}

pub(crate) struct DrawOwners {
    cursor: JournalCursor,
    nodes: UpdateNodes,
}

// The lists a read of the journal sorts nodes in.
#[derive(Default)]
struct UpdateNodes {
    // The nodes one update gave a record and those it took one from, each
    // sorted.
    joined_in_update: Vec<NodeId>,
    left_in_update: Vec<NodeId>,
    // The nodes a later update changed, sorted: their changes stand.
    decided: Vec<NodeId>,
}

impl DrawOwners {
    pub(crate) fn new(store: &mut SceneStore) -> Self {
        Self {
            cursor: store.open_journal(RecordEvents::OWNERS),
            nodes: UpdateNodes::default(),
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
        let Self { cursor, nodes } = self;
        store.read_journal(cursor, |_, read| {
            let JournalRead::Changes(entries) = read else {
                return false;
            };
            if !entries.is_empty() {
                nodes.note_changes(entries, changes);
            }
            true
        })
    }
}

impl UpdateNodes {
    // Adds to `changes` the nodes `entries` gave a record or took one
    // from, one change a node, as its latest update left it. Out of line:
    // most updates add or remove no record.
    #[inline(never)]
    fn note_changes(&mut self, entries: &[JournalEntry], changes: &mut Vec<DrawOwnerChange>) {
        let Self {
            joined_in_update,
            left_in_update,
            decided,
        } = self;
        decided.clear();
        // The updates go newest first: a node's latest change decides.
        let mut updates = entries
            .chunk_by(|a, b| a.update == b.update)
            .rev()
            .peekable();
        while let Some(update) = updates.next() {
            sorted_nodes(update, RecordEvent::joins, joined_in_update);
            sorted_nodes(update, RecordEvent::leaves, left_in_update);
            let first = changes.len();
            let undecided = |node: &NodeId| decided.binary_search(node).is_err();
            changes.extend(
                left_in_update
                    .iter()
                    .filter(|node| joined_in_update.binary_search(node).is_err())
                    .filter(|node| undecided(node))
                    .copied()
                    .map(DrawOwnerChange::Left),
            );
            changes.extend(
                joined_in_update
                    .iter()
                    .filter(|node| undecided(node))
                    .copied()
                    .map(DrawOwnerChange::Joined),
            );
            if updates.peek().is_some() {
                decided.extend(changes[first..].iter().map(|change| change.node()));
                decided.sort_unstable();
            }
        }
    }
}

// Puts into `nodes`, sorted and once each, the nodes whose own records
// `update`'s entries of the events `takes` keeps name.
fn sorted_nodes(update: &[JournalEntry], takes: fn(RecordEvent) -> bool, nodes: &mut Vec<NodeId>) {
    nodes.clear();
    nodes.extend(
        update
            .iter()
            .filter(|entry| takes(entry.event))
            .filter_map(drawn_node),
    );
    nodes.sort_unstable();
    nodes.dedup();
}

// The node the record of `entry` draws, when it is the node's own record:
// a wrapper or a draw group of a node is not.
fn drawn_node(entry: &JournalEntry) -> Option<NodeId> {
    match entry.kind {
        RecordKind::Layer | RecordKind::Leaf => entry.node,
        RecordKind::Wrapper | RecordKind::DrawGroup => None,
    }
}
