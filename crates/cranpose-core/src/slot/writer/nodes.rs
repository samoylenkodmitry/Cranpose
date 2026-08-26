use super::super::{
    NodeRecord, NodeSlotUpdate, SlotTable, SlotWriteSession,
    collect_root_node_ids_from_records_into,
};
use crate::{AnchorId, NodeId};

impl SlotTable {
    fn collect_subtree_node_records(&mut self, group_anchor: AnchorId) -> Vec<NodeRecord> {
        let Some(group_index) = self.active_group_index(group_anchor) else {
            log::error!(
                "slot table ignored root-node collection for stale group anchor {group_anchor:?}"
            );
            return Vec::new();
        };
        let Some(subtree_range) =
            self.repair_group_subtree_range_at_index(group_index, "root-node collection")
        else {
            log::error!(
                "slot table ignored root-node collection for malformed subtree at group index {group_index}"
            );
            return Vec::new();
        };

        for index in subtree_range.as_range() {
            self.repair_group_node_len_to_storage(index, "root-node collection");
        }

        let mut nodes = Vec::new();
        for index in subtree_range.as_range() {
            nodes.extend(self.group_node_records_at(index).iter().copied());
        }
        self.repair_group_subtree_node_count_from_storage(group_index, "root-node collection");
        nodes
    }

    pub(in crate::slot) fn collect_subtree_root_node_ids(
        &mut self,
        group_anchor: AnchorId,
    ) -> Vec<NodeId> {
        let nodes = self.collect_subtree_node_records(group_anchor);
        let mut root_nodes = Vec::new();
        collect_root_node_ids_from_records_into(&nodes, &mut root_nodes);
        root_nodes
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

    pub(crate) fn mixed_node_source(&mut self, source: crate::Key) -> crate::Key {
        self.state.mix_branch_fold(source)
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

    pub(crate) fn current_node_record(&mut self) -> Option<(NodeId, u32, crate::Key)> {
        let frame = self.state.group_stack.last()?;
        self.table
            .node_identity_at_cursor(frame.group_anchor, frame.node_cursor)
    }
}
