#[cfg(any(test, debug_assertions))]
use super::AnchorState;
use super::{ChildCursor, DirectChildRange, SlotTable, SubtreeRange};
use crate::{
    slot_storage::{ActiveGroupId, GroupKey},
    AnchorId, ScopeId,
};

pub(super) struct GroupRecord {
    pub(super) key: GroupKey,
    pub(super) parent_anchor: AnchorId,
    pub(super) depth: u32,
    pub(super) subtree_len: u32,
    pub(super) payload_start: u32,
    pub(super) payload_len: u32,
    pub(super) node_start: u32,
    pub(super) node_len: u32,
    pub(super) subtree_node_count: u32,
    pub(super) generation: u32,
    pub(super) anchor: AnchorId,
    pub(super) scope_id: Option<ScopeId>,
}

#[derive(Clone, Copy)]
pub(in crate::slot) struct GroupSiblingRecord {
    pub(in crate::slot) key: GroupKey,
    pub(in crate::slot) parent_anchor: AnchorId,
    pub(in crate::slot) anchor: AnchorId,
    pub(in crate::slot) subtree_len: usize,
}

impl SlotTable {
    pub(in crate::slot) fn group_count(&self) -> usize {
        self.groups.len()
    }

    pub(in crate::slot) fn active_group_index(&self, anchor: AnchorId) -> Option<usize> {
        self.anchors.active_index(anchor)
    }

    #[cfg(any(test, debug_assertions))]
    pub(in crate::slot) fn group_anchor_state(&self, anchor: AnchorId) -> Option<AnchorState> {
        self.anchors.state(anchor)
    }

    pub(in crate::slot) fn current_group_index(&self, anchor: AnchorId) -> usize {
        self.anchors
            .active_index(anchor)
            .expect("group anchor should resolve to an active group")
    }

    pub(in crate::slot) fn current_group(&self, anchor: AnchorId) -> &GroupRecord {
        &self.groups[self.current_group_index(anchor)]
    }

    #[cfg(test)]
    #[inline(always)]
    pub(in crate::slot) fn group_anchor_at_index(&self, group_index: usize) -> AnchorId {
        self.groups[group_index].anchor
    }

    #[cfg(any(test, debug_assertions))]
    pub(in crate::slot) fn group_parent_anchor_at_index(
        &self,
        group_index: usize,
    ) -> Option<AnchorId> {
        self.groups
            .get(group_index)
            .map(|group| group.parent_anchor)
    }

    #[inline(always)]
    pub(in crate::slot) fn group_scope_id_at_index(&self, group_index: usize) -> Option<ScopeId> {
        self.groups[group_index].scope_id
    }

    #[inline(always)]
    pub(in crate::slot) fn group_subtree_len_at_index(&self, group_index: usize) -> usize {
        self.groups[group_index].subtree_len as usize
    }

    #[inline(always)]
    pub(in crate::slot) fn group_subtree_end_at_index(&self, group_index: usize) -> usize {
        group_index + self.group_subtree_len_at_index(group_index)
    }

    #[inline(always)]
    pub(in crate::slot) fn group_subtree_range_at_index(&self, group_index: usize) -> SubtreeRange {
        SubtreeRange::from_root_len(group_index, self.group_subtree_len_at_index(group_index))
    }

    #[inline(always)]
    pub(in crate::slot) fn group_subtree_range(&self, anchor: AnchorId) -> SubtreeRange {
        self.group_subtree_range_at_index(self.current_group_index(anchor))
    }

    pub(in crate::slot) fn group_subtree_node_count_at_index(&self, group_index: usize) -> usize {
        self.groups[group_index].subtree_node_count as usize
    }

    #[cfg(any(test, debug_assertions))]
    pub(in crate::slot) fn group_depth_at_index(&self, group_index: usize) -> usize {
        self.groups[group_index].depth as usize
    }

    #[inline(always)]
    pub(in crate::slot) fn group_sibling_record_at_index(
        &self,
        group_index: usize,
    ) -> GroupSiblingRecord {
        let group = &self.groups[group_index];
        GroupSiblingRecord {
            key: group.key,
            parent_anchor: group.parent_anchor,
            anchor: group.anchor,
            subtree_len: group.subtree_len as usize,
        }
    }

    #[inline(always)]
    pub(in crate::slot) fn group_sibling_record_at_index_checked(
        &self,
        group_index: usize,
    ) -> Option<GroupSiblingRecord> {
        self.groups
            .get(group_index)
            .map(|group| GroupSiblingRecord {
                key: group.key,
                parent_anchor: group.parent_anchor,
                anchor: group.anchor,
                subtree_len: group.subtree_len as usize,
            })
    }

    pub(in crate::slot) fn checked_active_group_index(&self, group: ActiveGroupId) -> usize {
        let group_index = group.index();
        let record = self
            .groups
            .get(group_index)
            .expect("active group handle index missing");
        assert_eq!(
            record.generation,
            group.generation(),
            "active group handle generation mismatch"
        );
        group_index
    }

    pub(in crate::slot) fn active_group_id_at_index(&self, group_index: usize) -> ActiveGroupId {
        let record = self.groups.get(group_index).expect("group index missing");
        ActiveGroupId::new(group_index, record.generation)
    }

    pub(in crate::slot) fn active_group_anchor(&self, group: ActiveGroupId) -> AnchorId {
        let group_index = self.checked_active_group_index(group);
        self.groups[group_index].anchor
    }

    #[inline(always)]
    pub(in crate::slot) fn direct_child_range(&self, parent_anchor: AnchorId) -> DirectChildRange {
        if !parent_anchor.is_valid() {
            DirectChildRange::new(0, self.group_count())
        } else {
            let parent_index = self.current_group_index(parent_anchor);
            DirectChildRange::new(
                parent_index + 1,
                self.group_subtree_end_at_index(parent_index),
            )
        }
    }

    pub(in crate::slot) fn direct_child_anchor_at_cursor(
        &self,
        cursor: ChildCursor,
    ) -> Option<AnchorId> {
        self.direct_child_sibling_record_at_cursor(cursor)
            .map(|group| group.anchor)
    }

    pub(in crate::slot) fn direct_child_sibling_record_at_cursor(
        &self,
        cursor: ChildCursor,
    ) -> Option<GroupSiblingRecord> {
        self.direct_child_sibling_record_at(cursor.parent(), cursor.index())
    }

    fn direct_child_sibling_record_at(
        &self,
        parent_anchor: AnchorId,
        child_index: usize,
    ) -> Option<GroupSiblingRecord> {
        if !self
            .direct_child_range(parent_anchor)
            .contains_index(child_index)
        {
            return None;
        }
        let group = self.group_sibling_record_at_index_checked(child_index)?;
        (group.parent_anchor == parent_anchor).then_some(group)
    }

    pub(in crate::slot) fn assert_child_cursor_boundary(&self, cursor: ChildCursor) {
        let siblings = self.direct_child_range(cursor.parent());
        assert!(
            siblings.start() <= cursor.index() && cursor.index() <= siblings.end(),
            "child cursor must stay inside the parent child range"
        );
        if cursor.index() == siblings.end() {
            return;
        }

        let group = self.group_sibling_record_at_index(cursor.index());
        assert_eq!(
            group.parent_anchor,
            cursor.parent(),
            "child cursor must point at a direct child boundary"
        );
    }
}
