// The record each layout node draws through, its layer or its leaf, kept by
// the store's inserts, removals and kind changes: a reader finds the record
// of a node without a walk over the records.

use std::mem::size_of;

use cranpose_core::{
    NodeId, RecordId,
    collections::{growth_slack::GrowthSlack, map::HashMap},
};

use super::RecordKind;

#[derive(Clone, Default)]
pub(super) struct NodeRecords {
    records: HashMap<NodeId, RecordId>,
}

impl NodeRecords {
    pub(super) fn get(&self, node: NodeId) -> Option<RecordId> {
        self.records.get(&node).copied()
    }

    // Notes that `record`, of kind `kind`, draws for `node`.
    pub(super) fn claim(&mut self, record: RecordId, kind: RecordKind, node: Option<NodeId>) {
        if let Some(node) = own_node(kind, node) {
            self.records.insert(node, record);
        }
    }

    // Notes that `record`, of kind `kind`, no longer draws for `node`. A
    // record the node took since stays its record.
    pub(super) fn release(&mut self, record: RecordId, kind: RecordKind, node: Option<NodeId>) {
        if let Some(node) = own_node(kind, node)
            && self.records.get(&node) == Some(&record)
        {
            self.records.remove(&node);
        }
    }

    pub(super) fn clear(&mut self) {
        self.records.clear();
    }

    pub(super) fn trim_growth_slack(&mut self) {
        self.records.trim_growth_slack();
    }

    pub(super) fn heap_bytes(&self) -> usize {
        // A table keeps one control byte beside each slot.
        self.records.capacity() * (size_of::<(NodeId, RecordId)>() + 1)
    }
}

// The node a record of `kind` draws for as that node's own record: a wrapper
// or a draw group of a node is not.
fn own_node(kind: RecordKind, node: Option<NodeId>) -> Option<NodeId> {
    match kind {
        RecordKind::Layer | RecordKind::Leaf => node,
        RecordKind::Wrapper | RecordKind::DrawGroup => None,
    }
}
