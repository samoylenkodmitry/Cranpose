use super::super::{NodeRecord, NodeSlotUpdate, RootNodeIds, SlotTable, SlotWriteSession};
use crate::{AnchorId, NodeId};

impl SlotTable {
    fn subtree_node_records(&self, group_anchor: AnchorId) -> &[NodeRecord] {
        let Some(group_index) = self.active_group_index(group_anchor) else {
            log::error!(
                "slot table ignored root-node collection for stale group anchor {group_anchor:?}"
            );
            return &[];
        };
        let group = &self.groups[group_index];
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

    pub(in crate::slot) fn collect_subtree_root_node_ids(
        &self,
        group_anchor: AnchorId,
    ) -> RootNodeIds {
        let records = self.subtree_node_records(group_anchor);
        let parent = records.first().map(|record| record.parent_id);
        let roots: RootNodeIds = records
            .iter()
            .filter(|record| Some(record.parent_id) == parent)
            .map(|record| record.id)
            .collect();
        #[cfg(any(test, debug_assertions))]
        if crate::slot_validation_diagnostics_enabled() {
            let expected: RootNodeIds = super::super::types::root_node_ids(records).collect();
            assert_eq!(
                roots, expected,
                "the roots of a group subtree must be the records sharing its first record's parent"
            );
        }
        roots
    }

    pub(in crate::slot) fn first_subtree_root_node_id(
        &self,
        group_anchor: AnchorId,
    ) -> Option<NodeId> {
        let first = self
            .subtree_node_records(group_anchor)
            .first()
            .map(|record| record.id);
        #[cfg(any(test, debug_assertions))]
        if crate::slot_validation_diagnostics_enabled() {
            assert_eq!(
                first,
                self.collect_subtree_root_node_ids(group_anchor)
                    .first()
                    .copied(),
                "the first record of a group subtree must be one of its roots"
            );
        }
        first
    }
}

impl SlotWriteSession<'_> {
    pub(crate) fn record_node_with_parent(
        &mut self,
        id: NodeId,
        generation: u32,
        parent_id: Option<NodeId>,
        source: crate::Key,
    ) -> NodeSlotUpdate {
        let source = self.state.mix_branch_fold(source);
        let Some(frame) = self.state.group_stack.last_mut() else {
            log::error!(
                "slot writer record_node_with_parent called with an empty group stack; id={id}"
            );
            return NodeSlotUpdate::Inserted { id, generation };
        };
        let group_anchor = frame.group_anchor;
        let result = self.table.record_node_at_cursor(
            group_anchor,
            frame.node_cursor,
            id,
            parent_id,
            generation,
            source,
        );

        frame.advance_node_cursor();
        result
    }

    fn locate_node_record_by_source(
        &mut self,
        source: crate::Key,
        skip_matches: usize,
    ) -> Option<(usize, usize, NodeId, u32)> {
        let mixed = self.state.mix_branch_fold(source);
        let frame = self.state.group_stack.last()?;
        let (group_anchor, cursor) = (frame.group_anchor, frame.node_cursor);
        let mut from = cursor;
        let mut remaining = skip_matches;
        loop {
            let (found, id, generation) =
                self.table
                    .find_node_record_by_source(group_anchor, from, mixed)?;
            if remaining == 0 {
                return Some((found, cursor, id, generation));
            }
            remaining -= 1;
            from = found + 1;
        }
    }

    pub(crate) fn peek_node_record_by_source(
        &mut self,
        source: crate::Key,
        skip_matches: usize,
    ) -> Option<(NodeId, u32)> {
        self.locate_node_record_by_source(source, skip_matches)
            .map(|(_, _, id, generation)| (id, generation))
    }

    pub(crate) fn adopt_node_record_by_source(
        &mut self,
        source: crate::Key,
        skip_matches: usize,
    ) -> Option<(NodeId, u32)> {
        let (found, cursor, id, generation) =
            self.locate_node_record_by_source(source, skip_matches)?;
        if found > cursor {
            let group_anchor = self.state.group_stack.last()?.group_anchor;
            self.table
                .rotate_node_record_to_cursor(group_anchor, found, cursor);
        }
        Some((id, generation))
    }

    #[cfg(test)]
    pub(crate) fn current_node_record(&mut self) -> Option<(NodeId, u32, crate::Key)> {
        let frame = self.state.group_stack.last()?;
        self.table
            .node_identity_at_cursor(frame.group_anchor, frame.node_cursor)
    }
}
