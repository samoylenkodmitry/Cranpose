#[cfg(any(test, debug_assertions))]
use super::AnchorState;
use super::{ActiveGroupId, ChildCursor, DirectChildRange, GroupKey, SlotTable, SubtreeRange};
use crate::{AnchorId, RecomposeScope};

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
    pub(super) scope: Option<RecomposeScope>,
}

#[derive(Clone, Copy)]
pub(in crate::slot) struct GroupSiblingRecord {
    pub(in crate::slot) key: GroupKey,
    pub(in crate::slot) parent_anchor: AnchorId,
    pub(in crate::slot) anchor: AnchorId,
}

impl SlotTable {
    pub(in crate::slot) fn group_count(&self) -> usize {
        self.groups.len()
    }

    #[inline]
    pub(in crate::slot) fn active_group_index(&self, anchor: AnchorId) -> Option<usize> {
        let stored = self.anchors.active_index(anchor)?;
        if stored < self.groups.len() && !self.group_index_may_be_stale(stored) {
            return Some(stored);
        }
        self.locate_stale_group_index(stored, anchor)
    }

    /// The index of an open writer frame's group: the index the frame opened
    /// with while that group still sits there, else the anchor's lookup.
    #[inline]
    pub(in crate::slot) fn open_group_index(
        &self,
        anchor: AnchorId,
        index: usize,
    ) -> Option<usize> {
        if self
            .groups
            .get(index)
            .is_some_and(|group| group.anchor == anchor)
        {
            return Some(index);
        }
        self.moved_open_group_index(anchor)
    }

    #[cold]
    #[inline(never)]
    fn moved_open_group_index(&self, anchor: AnchorId) -> Option<usize> {
        self.active_group_index(anchor)
    }

    #[cold]
    #[inline(never)]
    fn locate_stale_group_index(&self, stored: usize, anchor: AnchorId) -> Option<usize> {
        if stored >= self.groups.len() {
            log::error!(
                "group anchor {anchor:?} points to active group index {stored}, but the slot table has only {} active groups",
                self.groups.len()
            );
            return None;
        }
        let group_index = self.resolve_group_index(stored, anchor);
        if group_index.is_none() {
            log::error!(
                "group anchor {anchor:?} holds stale index {stored}, but no group within {} places after it has that anchor",
                self.stale_group_inserts()
            );
        }
        group_index
    }

    #[cfg(any(test, debug_assertions))]
    pub(in crate::slot) fn group_anchor_state(&self, anchor: AnchorId) -> Option<AnchorState> {
        self.anchors.state(anchor)
    }

    #[cfg(test)]
    pub(in crate::slot) fn current_group_index(&self, anchor: AnchorId) -> usize {
        self.active_group_index(anchor)
            .expect("group anchor should resolve to an active group")
    }

    pub(in crate::slot) fn recover_group_index_from_recorded_anchor(
        &mut self,
        anchor: AnchorId,
    ) -> Option<usize> {
        if !anchor.is_valid() {
            return None;
        }
        let group_index = self
            .groups
            .iter()
            .position(|group| group.anchor == anchor)?;
        if self.anchors.state(anchor).is_none() {
            log::error!(
                "group anchor {anchor:?} exists in the active group table at index {group_index}, but the anchor registry has no matching generation"
            );
            return None;
        }
        log::error!(
            "repairing group anchor {anchor:?} active index from group table record at index {group_index}"
        );
        self.anchors.set_active(anchor, group_index);
        Some(group_index)
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
    pub(in crate::slot) fn group_scope_at_index(
        &self,
        group_index: usize,
    ) -> Option<RecomposeScope> {
        let Some(group) = self.groups.get(group_index) else {
            log::error!("slot table ignored scope lookup for missing group index {group_index}");
            return None;
        };
        group.scope.clone()
    }

    #[inline(always)]
    pub(in crate::slot) fn group_subtree_len_at_index(&self, group_index: usize) -> usize {
        let Some(group) = self.groups.get(group_index) else {
            log::error!(
                "slot table returned zero subtree length for missing group index {group_index}"
            );
            return 0;
        };
        group.subtree_len as usize
    }

    #[cfg(any(test, debug_assertions))]
    #[inline(always)]
    pub(in crate::slot) fn group_subtree_end_at_index(&self, group_index: usize) -> usize {
        group_index + self.group_subtree_len_at_index(group_index)
    }

    #[inline(always)]
    pub(in crate::slot) fn group_subtree_range_at_index(
        &self,
        root_index: usize,
    ) -> Option<SubtreeRange> {
        let len = self.groups.get(root_index)?.subtree_len as usize;
        let end = root_index.checked_add(len)?;
        (len > 0 && end <= self.groups.len()).then(|| SubtreeRange::from_root_len(root_index, len))
    }

    #[cfg(test)]
    pub(in crate::slot) fn group_subtree_node_count_at_index(&self, group_index: usize) -> usize {
        self.groups[group_index].subtree_node_count as usize
    }

    #[cfg(any(test, debug_assertions))]
    pub(in crate::slot) fn group_depth_at_index(&self, group_index: usize) -> usize {
        self.groups[group_index].depth as usize
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
            })
    }

    pub(in crate::slot) fn active_group_index_for_handle(
        &self,
        group: ActiveGroupId,
    ) -> Option<usize> {
        let group_index = group.index();
        let Some(record) = self.groups.get(group_index) else {
            log::error!("active group handle {group:?} points past active group storage");
            return None;
        };
        if record.generation != group.generation() {
            log::error!(
                "active group handle {group:?} generation does not match active group generation {:?}",
                record.generation
            );
            return None;
        }
        Some(group_index)
    }

    pub(in crate::slot) fn active_group_id_at_index(
        &self,
        group_index: usize,
    ) -> Option<ActiveGroupId> {
        let Some(record) = self.groups.get(group_index) else {
            log::error!("active group index {group_index} is outside active group storage");
            return None;
        };
        Some(ActiveGroupId::new(group_index, record.generation))
    }

    pub(in crate::slot) fn try_active_group_anchor(
        &self,
        group: ActiveGroupId,
    ) -> Option<AnchorId> {
        let group_index = self.active_group_index_for_handle(group)?;
        self.groups.get(group_index).map(|group| group.anchor)
    }

    #[cfg(test)]
    pub(in crate::slot) fn active_group_anchor(&self, group: ActiveGroupId) -> AnchorId {
        self.try_active_group_anchor(group)
            .expect("active group handle should resolve to an anchor")
    }

    #[inline(always)]
    pub(in crate::slot) fn direct_child_range(&self, parent_anchor: AnchorId) -> DirectChildRange {
        if !parent_anchor.is_valid() {
            DirectChildRange::new(0, self.group_count())
        } else {
            let Some(parent_index) = self.active_group_index(parent_anchor) else {
                let end = self.group_count();
                return DirectChildRange::new(end, end);
            };
            let start = parent_index + 1;
            let declared_end =
                parent_index.checked_add(self.group_subtree_len_at_index(parent_index));
            let end = match declared_end {
                Some(end) if start <= end && end <= self.group_count() => end,
                _ => {
                    log::error!(
                        "slot table clamped the child range of parent {parent_anchor:?}: declared end {declared_end:?} lies outside {} active groups",
                        self.group_count()
                    );
                    self.group_count()
                }
            };
            DirectChildRange::new(start, end)
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

    pub(in crate::slot) fn child_cursor_boundary_is_valid(&self, cursor: ChildCursor) -> bool {
        if cursor.parent().is_valid() && self.active_group_index(cursor.parent()).is_none() {
            return false;
        }
        let siblings = self.direct_child_range(cursor.parent());
        if !(siblings.start() <= cursor.index() && cursor.index() <= siblings.end()) {
            return false;
        }
        if cursor.index() == siblings.end() {
            return true;
        }
        self.group_sibling_record_at_index_checked(cursor.index())
            .is_some_and(|group| group.parent_anchor == cursor.parent())
    }
}
