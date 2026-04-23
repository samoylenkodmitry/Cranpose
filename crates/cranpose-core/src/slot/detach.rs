use super::{DetachedSubtree, GroupRecord, SlotTable, SlotWriteSessionState};
use crate::{remove_child_and_cleanup_now, slot_storage::GroupKey, AnchorId, Applier, NodeId};

impl SlotTable {
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

        DetachedSubtree {
            root_key,
            root_scope_id,
            groups: removed_groups,
            payloads: removed_payloads,
            nodes: removed_nodes,
        }
    }

    pub(in crate::slot) fn restore_subtree(
        &mut self,
        insert_index: usize,
        parent_anchor: AnchorId,
        key: GroupKey,
        mut subtree: DetachedSubtree,
    ) -> AnchorId {
        let root_anchor = subtree
            .groups
            .first()
            .map(|group| group.anchor)
            .expect("detached subtree must contain a root group");

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

        let subtree_len = subtree.groups.len();
        subtree.mark_nodes_active();
        self.restore_payloads_for_groups(insert_index, &mut subtree.groups, subtree.payloads);
        self.restore_nodes_for_groups(insert_index, &mut subtree.groups, subtree.nodes);
        self.groups
            .splice(insert_index..insert_index, subtree.groups);
        self.recompute_all_metadata();
        self.rebuild_payload_locations_for_group_range(insert_index, insert_index + subtree_len);
        root_anchor
    }

    pub(in crate::slot) fn root_finish_result(
        &mut self,
        state: &mut SlotWriteSessionState,
    ) -> Vec<DetachedSubtree> {
        let remaining = state.take_remaining_root_children();
        let mut detached = Vec::new();
        for anchor in remaining.into_iter().rev() {
            if self.anchors.contains_active(anchor) {
                detached.push(self.detach_subtree(anchor));
            }
        }
        detached.reverse();
        detached
    }
}

pub(in crate::slot) fn dispose_detached_node_now(applier: &mut dyn Applier, node_id: NodeId) {
    let parent_id = applier.get_mut(node_id).ok().and_then(|node| node.parent());
    if let Some(parent_id) = parent_id {
        let _ = remove_child_and_cleanup_now(applier, parent_id, node_id);
        return;
    }
    if let Ok(node) = applier.get_mut(node_id) {
        node.on_removed_from_parent();
        node.unmount();
    }
    let _ = applier.remove(node_id);
}

pub(crate) fn dispose_detached_subtree_now(applier: &mut dyn Applier, subtree: &DetachedSubtree) {
    let node_set = subtree
        .nodes
        .iter()
        .map(|node| node.id)
        .collect::<std::collections::HashSet<_>>();
    let roots = subtree
        .nodes
        .iter()
        .map(|node| node.id)
        .filter(|id| {
            let parent = applier.get_mut(*id).ok().and_then(|node| node.parent());
            parent.is_none_or(|parent_id| !node_set.contains(&parent_id))
        })
        .collect::<Vec<_>>();

    for root in roots {
        dispose_detached_node_now(applier, root);
    }
}
