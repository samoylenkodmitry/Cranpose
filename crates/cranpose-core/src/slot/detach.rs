use super::{DetachedAnchorSet, DetachedSubtree, GroupRecord, SlotTable, SlotWriteSessionState};
use crate::{
    remove_child_and_cleanup_now, slot_storage::GroupKey, AnchorId, Applier, NodeError, NodeId,
};

impl SlotTable {
    fn allocate_detached_generation(&mut self) -> u64 {
        let generation = self.next_detached_generation;
        self.next_detached_generation = self
            .next_detached_generation
            .checked_add(1)
            .expect("detached subtree generation overflow");
        generation
    }

    pub(in crate::slot) fn detach_range(
        &mut self,
        start_index: usize,
        subtree_len: usize,
    ) -> Vec<GroupRecord> {
        self.groups
            .drain(start_index..start_index + subtree_len)
            .collect::<Vec<_>>()
    }

    pub(in crate::slot) fn detach_subtree(&mut self, anchor: AnchorId) -> DetachedSubtree {
        let root_index = self.current_group_index(anchor);
        let root_key = self.groups[root_index].key;
        let root_scope_id = self.groups[root_index].scope_id;
        let root_parent_anchor = self.groups[root_index].parent_anchor;
        let root_subtree_len = self.groups[root_index].subtree_len;
        let root_subtree_node_count = self.groups[root_index].subtree_node_count;
        let subtree_len = root_subtree_len as usize;
        let mut removed_groups = self.detach_range(root_index, subtree_len);
        let detached_root_depth = removed_groups
            .first()
            .map(|group| group.depth)
            .expect("detached subtree must contain a root group");
        for group in &mut removed_groups {
            group.depth = group
                .depth
                .checked_sub(detached_root_depth)
                .expect("detached subtree depths must stay relative to the root");
        }
        removed_groups[0].parent_anchor = AnchorId::INVALID;
        let removed_payloads = self.detach_payloads_for_groups(root_index, &mut removed_groups);
        let removed_nodes = self.detach_nodes_for_groups(root_index, &mut removed_groups);
        self.clear_group_indexes(&removed_groups);
        self.clear_scope_index_for_groups(&removed_groups);
        self.refresh_group_indexes_from(root_index);

        self.adjust_ancestor_group_spans(
            root_parent_anchor,
            -(root_subtree_len as i32),
            -(root_subtree_node_count as i32),
        );

        let scope_ids = removed_groups
            .iter()
            .filter_map(|group| group.scope_id)
            .collect::<Vec<_>>();
        let anchors = DetachedAnchorSet {
            group_anchors: removed_groups.iter().map(|group| group.anchor).collect(),
        };
        let root_nodes = Self::root_node_ids_from_records(&removed_nodes);
        DetachedSubtree {
            root_key,
            root_scope_id,
            groups: removed_groups,
            payloads: removed_payloads,
            nodes: removed_nodes,
            root_nodes,
            scope_ids,
            anchors,
            generation: self.allocate_detached_generation(),
        }
    }

    pub(in crate::slot) fn restore_subtree(
        &mut self,
        insert_index: usize,
        parent_anchor: AnchorId,
        key: GroupKey,
        mut subtree: DetachedSubtree,
    ) -> AnchorId {
        assert_eq!(
            subtree.root_key(),
            key,
            "restored subtree root key must match the requested group key",
        );
        let restored_group_count = subtree.groups.len();
        let restored_subtree_len = subtree
            .groups
            .first()
            .map(|group| group.subtree_len as i32)
            .expect("detached subtree must contain a root group");
        let restored_subtree_node_count = subtree
            .groups
            .first()
            .map(|group| group.subtree_node_count as i32)
            .expect("detached subtree must contain a root group");
        debug_assert_eq!(
            restored_subtree_len as usize, restored_group_count,
            "detached subtree root span must match the stored group slice"
        );
        debug_assert_eq!(
            restored_subtree_node_count as usize,
            subtree.nodes.len(),
            "detached subtree root node count must match the stored node slice"
        );
        let root_anchor = subtree
            .groups
            .first()
            .map(|group| group.anchor)
            .expect("detached subtree must contain a root group");
        let restored_scope_entries = subtree
            .groups
            .iter()
            .filter_map(|group| group.scope_id.map(|scope_id| (scope_id, group.anchor)))
            .collect::<Vec<_>>();

        let depth_delta = if parent_anchor.is_valid() {
            self.current_group(parent_anchor).depth + 1
        } else {
            0
        } as i32
            - subtree.groups[0].depth as i32;

        subtree.groups[0].key = key;
        subtree.groups[0].parent_anchor = parent_anchor;
        for group in &mut subtree.groups {
            group.depth = ((group.depth as i32) + depth_delta) as u32;
        }

        subtree.mark_nodes_active();
        self.restore_payloads_for_groups(insert_index, &mut subtree.groups, subtree.payloads);
        self.restore_nodes_for_groups(insert_index, &mut subtree.groups, subtree.nodes);
        self.groups
            .splice(insert_index..insert_index, subtree.groups);
        self.refresh_group_indexes_from(insert_index);
        self.restore_scope_index_entries(restored_scope_entries);
        self.adjust_ancestor_group_spans(
            parent_anchor,
            restored_subtree_len,
            restored_subtree_node_count,
        );
        self.rebuild_payload_locations_for_group_range(
            insert_index,
            insert_index + restored_group_count,
        );
        root_anchor
    }

    pub(in crate::slot) fn root_finish_result(
        &mut self,
        state: &mut SlotWriteSessionState,
    ) -> Vec<DetachedSubtree> {
        if !state.root.detach_remaining_children {
            return Vec::new();
        }

        let next_child_index = state.root.next_child_index;
        let mut detached = Vec::new();
        while let Some(anchor) = self.direct_child_anchor_at(AnchorId::INVALID, next_child_index) {
            detached.push(self.detach_subtree(anchor));
        }
        detached
    }
}

pub(crate) fn dispose_detached_node_now(
    applier: &mut dyn Applier,
    node_id: NodeId,
) -> Result<(), NodeError> {
    let parent_id = applier.get_mut(node_id).ok().and_then(|node| node.parent());
    if let Some(parent_id) = parent_id {
        return remove_child_and_cleanup_now(applier, parent_id, node_id);
    }
    if let Ok(node) = applier.get_mut(node_id) {
        node.on_removed_from_parent();
        node.unmount();
    }
    match applier.remove(node_id) {
        Ok(()) | Err(NodeError::Missing { .. }) => Ok(()),
        Err(err) => Err(err),
    }
}

pub(crate) fn dispose_detached_subtree_now(applier: &mut dyn Applier, subtree: &DetachedSubtree) {
    let roots = if subtree.root_nodes().is_empty() && !subtree.nodes.is_empty() {
        SlotTable::root_node_ids_from_records(&subtree.nodes)
    } else {
        subtree.root_nodes().to_vec()
    };

    for root in roots {
        let _ = dispose_detached_node_now(applier, root);
    }
}
