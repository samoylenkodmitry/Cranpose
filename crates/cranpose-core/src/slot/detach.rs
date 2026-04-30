use super::{
    checked_u32_delta, CheckedU32Delta, ChildCursor, DetachedChild, DetachedSubtree, GroupRecord,
    SlotTable, SlotWriteSessionState, SubtreeRange,
};
use crate::{remove_child_and_cleanup_now, AnchorId, Applier, NodeError, NodeId};

impl SlotTable {
    // Keep detach and restore ordering aligned with docs/SLOT_TABLE_LIFECYCLE.md.
    // Indexes, scopes, payload locations, and ancestor spans are deliberately
    // updated in separate steps so validation can pinpoint broken transitions.
    fn flush_group_index_refresh_from(&mut self, start: Option<usize>) {
        if let Some(start) = start {
            self.refresh_group_indexes_from(start);
        }
    }

    fn detach_range(&mut self, range: SubtreeRange) -> Vec<GroupRecord> {
        self.groups.drain(range.as_range()).collect::<Vec<_>>()
    }

    fn assert_detached_subtree_restore_ready(&self, subtree: &DetachedSubtree) {
        #[cfg(any(test, debug_assertions))]
        subtree
            .validate_detached()
            .expect("detached subtree must validate before restore");
        for group in &subtree.groups {
            assert!(
                self.anchors.is_detached(group.anchor),
                "restored group anchors must be detached before restore"
            );
        }
        for payload in &subtree.payloads {
            assert!(
                self.payload_anchors.is_detached(payload.anchor),
                "restored payload anchors must be detached before restore"
            );
        }
    }

    fn detach_subtree_at_index_internal(
        &mut self,
        root_index: usize,
        refresh_indexes: bool,
    ) -> DetachedSubtree {
        let root_parent_anchor = self.groups[root_index].parent_anchor;
        let root_subtree_len = self.groups[root_index].subtree_len;
        let root_subtree_node_count = self.groups[root_index].subtree_node_count;
        let removed_group_range = self.group_subtree_range_at_index(root_index);
        let mut removed_groups = self.detach_range(removed_group_range);
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
        if refresh_indexes {
            self.refresh_group_indexes_from(root_index);
        }

        self.adjust_ancestor_group_spans(
            root_parent_anchor,
            -i64::from(root_subtree_len),
            -i64::from(root_subtree_node_count),
        );

        let subtree = DetachedSubtree {
            groups: removed_groups,
            payloads: removed_payloads,
            nodes: removed_nodes,
        };
        #[cfg(any(test, debug_assertions))]
        if refresh_indexes {
            self.debug_assert_valid_after("detach_subtree");
        }
        #[cfg(any(test, debug_assertions))]
        subtree
            .validate_detached()
            .expect("detached subtree must validate after detach");
        subtree
    }

    pub(in crate::slot) fn detach_subtrees_at_cursor(
        &mut self,
        cursor: ChildCursor,
    ) -> Vec<DetachedSubtree> {
        let mut detached = Vec::new();
        let mut dirty_start = None;
        while self.direct_child_anchor_at_cursor(cursor).is_some() {
            dirty_start.get_or_insert(cursor.index());
            detached.push(self.detach_subtree_at_index_internal(cursor.index(), false));
        }
        self.flush_group_index_refresh_from(dirty_start);
        #[cfg(any(test, debug_assertions))]
        if !detached.is_empty() {
            self.debug_assert_valid_after("detach_subtrees_at_cursor");
        }
        detached
    }

    pub(in crate::slot) fn restore_subtree(
        &mut self,
        cursor: ChildCursor,
        detached: DetachedChild,
    ) -> AnchorId {
        self.assert_child_cursor_boundary(cursor);
        let key = detached.expected_key();
        let mut subtree = detached.into_subtree();
        let insert_index = cursor.index();
        let parent_anchor = cursor.parent();
        assert_eq!(
            subtree.root_key(),
            key,
            "restored subtree root key must match the requested group key",
        );
        self.assert_detached_subtree_restore_ready(&subtree);
        let restored_group_count = subtree.groups.len();
        let restored_subtree_len = subtree
            .groups
            .first()
            .map(|group| i64::from(group.subtree_len))
            .expect("detached subtree must contain a root group");
        let restored_subtree_node_count = subtree
            .groups
            .first()
            .map(|group| i64::from(group.subtree_node_count))
            .expect("detached subtree must contain a root group");
        let restored_subtree_len_usize =
            usize::try_from(restored_subtree_len).expect("restored subtree length must fit usize");
        let restored_subtree_node_count_usize = usize::try_from(restored_subtree_node_count)
            .expect("restored subtree node count must fit usize");
        assert_eq!(
            restored_subtree_len_usize, restored_group_count,
            "detached subtree root span must match the stored group slice"
        );
        assert_eq!(
            restored_subtree_node_count_usize,
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

        let target_root_depth = if parent_anchor.is_valid() {
            self.current_group(parent_anchor)
                .depth
                .checked_add(1)
                .expect("restored subtree depth overflow")
        } else {
            0
        };
        let depth_delta = i64::from(target_root_depth) - i64::from(subtree.groups[0].depth);

        subtree.groups[0].key = key;
        subtree.groups[0].parent_anchor = parent_anchor;
        let depth_delta = CheckedU32Delta::from_i64(depth_delta, "group depth");
        for group in &mut subtree.groups {
            group.depth = checked_u32_delta(group.depth, depth_delta, 0, "group depth");
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
        let restored_group_range = SubtreeRange::from_root_len(insert_index, restored_group_count);
        self.refresh_payload_locations_for_group_range(restored_group_range.as_group_range());
        #[cfg(any(test, debug_assertions))]
        self.debug_assert_valid_after("restore_subtree");
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
        let cursor = ChildCursor::new(AnchorId::INVALID, next_child_index);
        self.detach_subtrees_at_cursor(cursor)
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
    let mut root_nodes = Vec::new();
    subtree.collect_root_nodes_into(&mut root_nodes);
    for root in root_nodes {
        let _ = dispose_detached_node_now(applier, root);
    }
}
