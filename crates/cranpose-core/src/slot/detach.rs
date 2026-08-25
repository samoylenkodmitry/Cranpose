use super::{
    checked_u32_delta, checked_usize_to_i64, CheckedU32Delta, ChildCursor, DetachedSubtree,
    GroupKey, GroupRecord, SlotTable, SlotWriteSessionState, SubtreeRange,
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

    pub(in crate::slot) fn subtree_restore_ready(
        &self,
        cursor: ChildCursor,
        key: super::GroupKey,
        subtree: &DetachedSubtree,
    ) -> bool {
        if !self.child_cursor_boundary_is_valid(cursor) {
            log::error!(
                "slot table rejected detached subtree restore for invalid child cursor parent={:?} index={}",
                cursor.parent(),
                cursor.index()
            );
            return false;
        }
        let Some(root) = subtree.groups.first() else {
            log::error!("slot table rejected empty detached subtree restore");
            return false;
        };
        if root.key != key {
            log::error!(
                "slot table rejected detached subtree restore for root key mismatch: expected {:?}, actual {:?}",
                key,
                root.key
            );
            return false;
        }
        #[cfg(any(test, debug_assertions))]
        if let Err(error) = subtree.validate_detached() {
            log::error!("slot table rejected invalid detached subtree restore: {error:?}");
            return false;
        }
        if root.parent_anchor.is_valid() {
            log::error!(
                "slot table rejected detached subtree restore with attached root parent {:?}",
                root.parent_anchor
            );
            return false;
        }
        if let Some(node) = subtree.nodes.iter().find(|node| {
            !matches!(
                node.lifecycle,
                super::NodeLifecycle::Active | super::NodeLifecycle::RetainedDetached
            )
        }) {
            log::error!(
                "slot table rejected detached subtree restore with invalid node lifecycle for node {}: {:?}",
                node.id,
                node.lifecycle
            );
            return false;
        }
        for group in &subtree.groups {
            if !self.anchors.is_detached(group.anchor) {
                log::error!(
                    "slot table rejected detached subtree restore because group anchor {:?} is not detached",
                    group.anchor
                );
                return false;
            }
        }
        for payload in &subtree.payloads {
            if !self.payload_anchors.is_detached(payload.anchor) {
                log::error!(
                    "slot table rejected detached subtree restore because payload anchor {:?} is not detached",
                    payload.anchor
                );
                return false;
            }
        }
        let restored_group_count = subtree.groups.len();
        let restored_subtree_len_usize = root.subtree_len as usize;
        let restored_subtree_node_count_usize = root.subtree_node_count as usize;
        if restored_subtree_len_usize != restored_group_count {
            log::error!(
                "slot table rejected detached subtree restore with root span {} over {} stored groups",
                restored_subtree_len_usize,
                restored_group_count
            );
            return false;
        }
        if restored_subtree_node_count_usize != subtree.nodes.len() {
            log::error!(
                "slot table rejected detached subtree restore with root node count {} over {} stored nodes",
                restored_subtree_node_count_usize,
                subtree.nodes.len()
            );
            return false;
        }
        true
    }

    fn clear_conflicting_restored_scope_entries(&self, subtree: &mut DetachedSubtree) {
        let restored_scope_entries = subtree
            .groups
            .iter()
            .filter_map(|group| group.scope_id.map(|scope_id| (scope_id, group.anchor)))
            .collect::<Vec<_>>();
        if self
            .scope_index
            .restore_entries_available(&restored_scope_entries)
        {
            return;
        }

        for group in &mut subtree.groups {
            let Some(scope_id) = group.scope_id else {
                continue;
            };
            if self
                .scope_index
                .anchor(scope_id)
                .is_some_and(|active_anchor| active_anchor != group.anchor)
            {
                log::error!(
                    "clearing restored scope id {scope_id:?} from group anchor {:?} because it is already active elsewhere",
                    group.anchor
                );
                group.scope_id = None;
            }
        }
    }

    fn detach_subtree_at_index_internal(
        &mut self,
        root_index: usize,
        refresh_indexes: bool,
    ) -> DetachedSubtree {
        // Detach recipe:
        // drain the group segment, normalize detached depths and root parent,
        // extract payload and node segments, clear active indexes, refresh the
        // active suffix when requested, then shrink ancestor spans.
        let root_parent_anchor = self.groups[root_index].parent_anchor;
        let Some(removed_group_range) =
            self.repair_group_subtree_range_at_index(root_index, "subtree detach")
        else {
            log::error!(
                "slot table rejected detached subtree extraction for missing group index {root_index}"
            );
            return DetachedSubtree {
                groups: Vec::new(),
                payloads: Vec::new(),
                nodes: Vec::new(),
            };
        };
        let root_subtree_len = removed_group_range.len();
        let root_subtree_node_count = self.groups[root_index].subtree_node_count;
        let mut removed_groups = self.detach_range(removed_group_range);
        let Some(detached_root_depth) = removed_groups.first().map(|group| group.depth) else {
            log::error!("slot table detached an empty subtree for group index {root_index}");
            return DetachedSubtree {
                groups: removed_groups,
                payloads: Vec::new(),
                nodes: Vec::new(),
            };
        };
        for group in &mut removed_groups {
            group.depth = group.depth.checked_sub(detached_root_depth).unwrap_or_else(|| {
                log::error!(
                    "slot table repaired detached subtree group depth below root depth during detach"
                );
                0
            });
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
            -checked_usize_to_i64(root_subtree_len, "detached subtree length"),
            -i64::from(root_subtree_node_count),
        );

        let subtree = DetachedSubtree {
            groups: removed_groups,
            payloads: removed_payloads,
            nodes: removed_nodes,
        };
        #[cfg(any(test, debug_assertions))]
        subtree
            .validate_detached()
            .expect("detached subtree must validate after detach");
        #[cfg(any(test, debug_assertions))]
        if refresh_indexes {
            self.debug_assert_valid_after("detach_subtree");
        }
        subtree
    }

    pub(in crate::slot) fn detach_subtrees_at_cursor(
        &mut self,
        cursor: ChildCursor,
    ) -> Vec<DetachedSubtree> {
        if !self.repair_child_cursor_parent_subtree(cursor, "subtree detach cursor") {
            log::error!(
                "slot table rejected subtree detach for unrecoverable child cursor parent={:?} index={}",
                cursor.parent(),
                cursor.index()
            );
            return Vec::new();
        }
        let mut detached = Vec::new();
        let dirty_start = self
            .direct_child_anchor_at_cursor(cursor)
            .is_some()
            .then(|| cursor.index());
        self.detach_children_at_cursor_into(cursor, &mut detached);
        self.flush_group_index_refresh_from(dirty_start);
        #[cfg(any(test, debug_assertions))]
        if !detached.is_empty() {
            self.debug_assert_valid_after("detach_subtrees_at_cursor");
        }
        detached
    }

    fn detach_children_at_cursor_into(
        &mut self,
        cursor: ChildCursor,
        detached: &mut Vec<DetachedSubtree>,
    ) {
        while let Some(child_anchor) = self.direct_child_anchor_at_cursor(cursor) {
            // A transparent group is a conditional branch's bracket: structure,
            // not a lifecycle unit. Detach through it so every group that owns
            // state carries its own retention decision — a retain-marked group
            // inside a departed branch must be retained, not buried in a shell
            // whose missing scope reads as dispose-by-default. The bare shell
            // follows as its own subtree.
            if self
                .groups
                .get(cursor.index())
                .is_some_and(|group| group.transparent && group.subtree_len > 1)
            {
                let shell_children = ChildCursor::new(child_anchor, cursor.index() + 1);
                if self.repair_child_cursor_parent_subtree(shell_children, "branch shell detach") {
                    self.detach_children_at_cursor_into(shell_children, detached);
                } else {
                    log::error!(
                        "slot table detached a branch shell whole after rejecting its child cursor at index {}",
                        cursor.index()
                    );
                }
            }
            let subtree = self.detach_subtree_at_index_internal(cursor.index(), false);
            if subtree.group_count() == 0 {
                log::error!(
                    "slot table stopped detaching children at cursor parent={:?} index={} after empty subtree extraction",
                    cursor.parent(),
                    cursor.index()
                );
                break;
            }
            detached.push(subtree);
        }
    }

    pub(in crate::slot) fn restore_subtree(
        &mut self,
        cursor: ChildCursor,
        key: GroupKey,
        mut subtree: DetachedSubtree,
    ) -> Result<AnchorId, DetachedSubtree> {
        if !self.repair_child_cursor_parent_subtree(cursor, "detached subtree restore cursor") {
            log::error!(
                "slot table rejected detached subtree restore for unrecoverable child cursor parent={:?} index={}",
                cursor.parent(),
                cursor.index()
            );
            return Err(subtree);
        }
        if !self.subtree_restore_ready(cursor, key, &subtree) {
            return Err(subtree);
        }
        let insert_index = cursor.index();
        let parent_anchor = cursor.parent();
        let restored_group_count = subtree.groups.len();
        let Some(root) = subtree.groups.first() else {
            log::error!("slot table rejected empty detached subtree after restore preflight");
            return Err(subtree);
        };
        let restored_subtree_len = i64::from(root.subtree_len);
        let restored_subtree_node_count = i64::from(root.subtree_node_count);
        let root_anchor = root.anchor;
        self.clear_conflicting_restored_scope_entries(&mut subtree);
        let restored_scope_entries = subtree
            .groups
            .iter()
            .filter_map(|group| group.scope_id.map(|scope_id| (scope_id, group.anchor)))
            .collect::<Vec<_>>();

        // Restore recipe:
        // retarget root parent/depth, mark nodes active, restore payload and
        // node segments, splice groups, refresh active indexes, restore scopes,
        // update ancestor spans, then refresh payload anchor locations.
        let target_root_depth = if parent_anchor.is_valid() {
            let Some(parent_index) = self.active_group_index(parent_anchor) else {
                log::error!(
                    "slot table rejected detached subtree restore for stale parent anchor {parent_anchor:?}"
                );
                return Err(subtree);
            };
            let Some(depth) = self.groups[parent_index].depth.checked_add(1) else {
                log::error!(
                    "slot table rejected detached subtree restore because parent depth overflowed"
                );
                return Err(subtree);
            };
            depth
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
        Ok(root_anchor)
    }

    /// The nearest ancestor that is not a transparent branch bracket —
    /// the group whose finish is where a departed branch's content would
    /// have been handled before brackets existed. `AnchorId::INVALID`
    /// names the root.
    pub(in crate::slot) fn nearest_non_transparent_ancestor(&self, anchor: AnchorId) -> AnchorId {
        let mut current = anchor;
        loop {
            if !current.is_valid() {
                return AnchorId::INVALID;
            }
            let Some(index) = self.active_group_index(current) else {
                return AnchorId::INVALID;
            };
            let Some(record) = self.groups.get(index) else {
                return AnchorId::INVALID;
            };
            if !record.transparent {
                return current;
            }
            current = record.parent_anchor;
        }
    }

    /// Detach `key`'s subtree out of a later transparent sibling of `shell`,
    /// searching only brackets from the same branch site (same static key):
    /// the removal of an earlier loop iteration shifts every later bracket by
    /// one, and the keyed content inside must move rather than be recomposed.
    pub(in crate::slot) fn steal_keyed_subtree_from_later_transparent_siblings(
        &mut self,
        shell_anchor: AnchorId,
        key: GroupKey,
    ) -> Option<DetachedSubtree> {
        let shell_index = self.active_group_index(shell_anchor)?;
        let shell = self.groups.get(shell_index)?;
        let shell_static_key = shell.key.static_key;
        let parent_anchor = shell.parent_anchor;
        let siblings = self.direct_child_range(parent_anchor);
        let mut index = shell_index + self.group_subtree_len_at_index(shell_index);
        while index < siblings.end() {
            let record = self.groups.get(index)?;
            if record.parent_anchor != parent_anchor {
                return None;
            }
            let subtree_len = self.group_subtree_len_at_index(index);
            if record.transparent && record.key.static_key == shell_static_key {
                if let Some(found) = self.find_keyed_child_in_transparent_subtree(index, key) {
                    let subtree = self.detach_subtree_at_index_internal(found, false);
                    if subtree.group_count() == 0 {
                        return None;
                    }
                    return Some(subtree);
                }
            }
            index += subtree_len;
        }
        None
    }

    /// A direct child of `shell_index` matching `key`, descending through
    /// nested transparent brackets only.
    fn find_keyed_child_in_transparent_subtree(
        &self,
        shell_index: usize,
        key: GroupKey,
    ) -> Option<usize> {
        let shell_anchor = self.groups.get(shell_index)?.anchor;
        let end = shell_index + self.group_subtree_len_at_index(shell_index);
        let mut index = shell_index + 1;
        while index < end {
            let record = self.groups.get(index)?;
            if record.parent_anchor != shell_anchor {
                log::error!(
                    "slot table stopped a keyed steal scan at index {index}: expected a direct child of {shell_anchor:?}, found a child of {:?}",
                    record.parent_anchor
                );
                return None;
            }
            if record.key == key {
                return Some(index);
            }
            let subtree_len = self.group_subtree_len_at_index(index);
            if record.transparent {
                if let Some(found) = self.find_keyed_child_in_transparent_subtree(index, key) {
                    return Some(found);
                }
            }
            index += subtree_len;
        }
        None
    }

    pub(in crate::slot) fn root_finish_result(
        &mut self,
        state: &mut SlotWriteSessionState,
    ) -> Vec<DetachedSubtree> {
        let mut detached = state.drain_orphaned_keyed_for_owner(AnchorId::INVALID);
        if !state.root.detach_remaining_children {
            return detached;
        }

        let next_child_index = state.root.next_child_index;
        let cursor = ChildCursor::new(AnchorId::INVALID, next_child_index);
        detached.extend(self.detach_subtrees_at_cursor(cursor));
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

pub(crate) fn dispose_detached_subtree_now(
    applier: &mut dyn Applier,
    subtree: &DetachedSubtree,
) -> Result<(), NodeError> {
    let mut root_nodes = Vec::new();
    subtree.collect_root_nodes_checked_into(&mut root_nodes, "immediate disposal");
    for root in root_nodes {
        dispose_detached_node_now(applier, root)?;
    }
    Ok(())
}
