use super::{GroupFlags, SlotTable};
use crate::{
    slot_storage::{GroupId, GroupKey},
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
    pub(super) flags: GroupFlags,
}

impl SlotTable {
    pub(in crate::slot) fn current_group_index(&self, anchor: AnchorId) -> usize {
        self.anchors
            .active_index(anchor)
            .expect("group anchor should resolve to an active group")
    }

    pub(in crate::slot) fn current_group(&self, anchor: AnchorId) -> &GroupRecord {
        &self.groups[self.current_group_index(anchor)]
    }

    pub(in crate::slot) fn checked_group_index(&self, group: GroupId) -> usize {
        let group_index = group.index();
        let record = self
            .groups
            .get(group_index)
            .expect("group handle index missing");
        assert_eq!(
            record.generation,
            group.generation(),
            "group handle generation mismatch"
        );
        group_index
    }

    pub(in crate::slot) fn group_id_at_index(&self, group_index: usize) -> GroupId {
        let record = self.groups.get(group_index).expect("group index missing");
        GroupId::new(group_index, record.generation)
    }

    pub(in crate::slot) fn group_anchor(&self, group: GroupId) -> AnchorId {
        let group_index = self.checked_group_index(group);
        self.groups[group_index].anchor
    }

    pub(in crate::slot) fn direct_child_anchors(&self, parent_anchor: AnchorId) -> Vec<AnchorId> {
        let mut children = Vec::new();
        if !parent_anchor.is_valid() {
            let mut index = 0;
            while index < self.groups.len() {
                let group = &self.groups[index];
                debug_assert!(
                    !group.parent_anchor.is_valid(),
                    "root-level traversal should only step through root groups"
                );
                children.push(group.anchor);
                index += group.subtree_len as usize;
            }
            return children;
        }

        let parent_index = self.current_group_index(parent_anchor);
        let end = parent_index + self.groups[parent_index].subtree_len as usize;
        let mut index = parent_index + 1;
        while index < end {
            let group = &self.groups[index];
            debug_assert_eq!(
                group.parent_anchor, parent_anchor,
                "direct child traversal must land on immediate children only"
            );
            children.push(group.anchor);
            index += group.subtree_len as usize;
        }
        children
    }
}
