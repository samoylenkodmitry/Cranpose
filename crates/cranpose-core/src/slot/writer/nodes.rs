use super::super::{
    GroupRecord, NodeRecord, NodeSlotUpdate, RootNodeIds, SlotTable, SlotWriteSession,
};
use crate::NodeId;

impl SlotTable {
    fn subtree_node_records_at(&self, group_index: usize) -> &[NodeRecord] {
        self.subtree_node_records_of(&self.groups[group_index], group_index)
    }

    fn subtree_node_records_of(&self, group: &GroupRecord, group_index: usize) -> &[NodeRecord] {
        let start = group.node_start as usize;
        let records = start
            .checked_add(group.subtree_node_count as usize)
            .and_then(|end| self.nodes.get(start..end));
        records.unwrap_or_else(|| {
            log::error!(
                "slot table ignored root-node collection past node storage at group index {group_index}"
            );
            &[]
        })
    }

    pub(in crate::slot) fn subtree_root_node_ids_at(&self, group_index: usize) -> RootNodeIds {
        self.subtree_root_node_id_iter_at(group_index).collect()
    }

    /// The ids of the root nodes of the subtree at `group_index`: the
    /// records sharing its first record's parent, in order.
    pub(in crate::slot) fn subtree_root_node_id_iter_at(
        &self,
        group_index: usize,
    ) -> impl Iterator<Item = NodeId> + '_ {
        root_node_id_iter(self.subtree_node_records_at(group_index))
    }

    /// The end of the subtree at `group_index` and the ids of its root
    /// nodes, from one read of its record. `None` when its stored span
    /// reaches past the groups.
    pub(in crate::slot) fn subtree_end_and_root_node_ids_at(
        &self,
        group_index: usize,
    ) -> Option<(usize, impl Iterator<Item = NodeId> + '_)> {
        let group = self.groups.get(group_index)?;
        let len = group.subtree_len as usize;
        let end = group_index
            .checked_add(len)
            .filter(|&end| len > 0 && end <= self.groups.len())?;
        Some((
            end,
            root_node_id_iter(self.subtree_node_records_of(group, group_index)),
        ))
    }

    pub(in crate::slot) fn first_subtree_root_node_id_at(
        &self,
        group_index: usize,
    ) -> Option<NodeId> {
        let records = self.subtree_node_records_at(group_index);
        let first = records.first().map(|record| record.id);
        #[cfg(any(test, debug_assertions))]
        if crate::slot_validation_diagnostics_enabled() {
            assert_eq!(
                first,
                self.subtree_root_node_id_iter_at(group_index).next(),
                "the first record of a group subtree must be one of its roots"
            );
        }
        first
    }
}

/// A node record of the open group that a node emitted again may adopt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FoundNodeRecord {
    index: usize,
    pub(crate) id: NodeId,
    pub(crate) generation: u32,
}

impl SlotWriteSession<'_> {
    fn open_node_group(&self) -> Option<(usize, usize)> {
        let frame = self.state.group_stack.last()?;
        let group_index = self
            .table
            .open_group_index(frame.group_anchor, frame.group_index)?;
        Some((group_index, frame.node_cursor))
    }

    pub(crate) fn record_node_with_parent(
        &mut self,
        id: NodeId,
        generation: u32,
        parent_id: Option<NodeId>,
        source: crate::Key,
    ) -> NodeSlotUpdate {
        let source = self.state.mix_branch_fold(source);
        let Some(frame) = self.state.group_stack.last() else {
            log::error!(
                "slot writer record_node_with_parent called with an empty group stack; id={id}"
            );
            return NodeSlotUpdate::Inserted { id, generation };
        };
        let (group_anchor, node_cursor) = (frame.group_anchor, frame.node_cursor);
        let result = match self.table.open_group_index(group_anchor, frame.group_index) {
            Some(group_index) => self.table.record_node_at(
                group_anchor,
                group_index,
                node_cursor,
                id,
                parent_id,
                generation,
                source,
            ),
            None => {
                log::error!(
                    "slot table ignored node record for stale owner anchor {group_anchor:?}; node id={id}"
                );
                NodeSlotUpdate::Inserted { id, generation }
            }
        };
        if let Some(frame) = self.state.group_stack.last_mut() {
            frame.advance_node_cursor();
        }
        result
    }

    /// The `skip_matches`th node record at or after the node cursor that a
    /// node emitted from `source` left.
    pub(crate) fn peek_node_record_by_source(
        &mut self,
        source: crate::Key,
        skip_matches: usize,
    ) -> Option<FoundNodeRecord> {
        let mixed = self.state.mix_branch_fold(source);
        let (group_index, cursor) = self.open_node_group()?;
        let mut from = cursor;
        let mut remaining = skip_matches;
        loop {
            let (index, id, generation) =
                self.table
                    .find_node_record_by_source_at(group_index, from, mixed)?;
            if remaining == 0 {
                return Some(FoundNodeRecord {
                    index,
                    id,
                    generation,
                });
            }
            remaining -= 1;
            from = index + 1;
        }
    }

    /// Moves `found` to the node cursor and records its node there.
    pub(crate) fn adopt_node_record(
        &mut self,
        found: FoundNodeRecord,
        parent_id: Option<NodeId>,
        source: crate::Key,
    ) -> NodeSlotUpdate {
        if let Some((group_index, cursor)) = self.open_node_group()
            && found.index > cursor
        {
            self.table
                .rotate_node_record_to_cursor_at(group_index, found.index, cursor);
        }
        self.record_node_with_parent(found.id, found.generation, parent_id, source)
    }

    #[cfg(test)]
    pub(crate) fn current_node_record(&mut self) -> Option<(NodeId, u32, crate::Key)> {
        let frame = self.state.group_stack.last()?;
        self.table
            .node_identity_at_cursor(frame.group_anchor, frame.node_cursor)
    }
}

/// The ids of the root nodes among a subtree's node `records`: those sharing
/// the first record's parent, in order.
fn root_node_id_iter(records: &[NodeRecord]) -> impl Iterator<Item = NodeId> + '_ {
    #[cfg(any(test, debug_assertions))]
    if crate::slot_validation_diagnostics_enabled() {
        let parent = records.first().map(|record| record.parent_id);
        assert!(
            records
                .iter()
                .filter(|record| Some(record.parent_id) == parent)
                .map(|record| record.id)
                .eq(super::super::types::root_node_ids(records)),
            "the roots of a group subtree must be the records sharing its first record's parent"
        );
    }
    let parent = records.first().map(|record| record.parent_id);
    records
        .iter()
        .filter(move |record| Some(record.parent_id) == parent)
        .map(|record| record.id)
}
