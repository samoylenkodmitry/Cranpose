use super::super::{
    collect_root_node_ids_from_records_into, NodeRecord, NodeSlotUpdate, SlotTable,
    SlotWriteSession,
};
use crate::{AnchorId, NodeId};

impl SlotTable {
    fn collect_subtree_node_records(&self, group_anchor: AnchorId) -> Vec<NodeRecord> {
        let group_index = self.current_group_index(group_anchor);
        let subtree_end = self.group_subtree_end_at_index(group_index);
        let mut nodes = Vec::with_capacity(self.group_subtree_node_count_at_index(group_index));
        for index in group_index..subtree_end {
            nodes.extend(self.group_node_records_at(index).iter().copied());
        }
        nodes
    }

    pub(super) fn collect_subtree_root_node_ids(&self, group_anchor: AnchorId) -> Vec<NodeId> {
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
    ) -> NodeSlotUpdate {
        let frame = self
            .state
            .group_stack
            .last_mut()
            .expect("node records require an active group");
        let group_anchor = frame.group_anchor;
        let result = self.table.record_node_at_cursor(
            group_anchor,
            frame.node_cursor,
            id,
            parent_id,
            generation,
        );

        frame.advance_node_cursor();
        result
    }

    pub(crate) fn current_node_record(&self) -> Option<(NodeId, u32)> {
        let frame = self.state.group_stack.last()?;
        self.table
            .node_identity_at_cursor(frame.group_anchor, frame.node_cursor)
    }
}
