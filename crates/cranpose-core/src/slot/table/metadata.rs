use super::super::GroupRecord;
use super::SlotTable;
impl SlotTable {
    pub(super) fn recompute_group_indexes(&mut self) {
        self.anchors.recompute_active(&self.groups);
    }

    pub(super) fn recompute_scope_index(&mut self) {
        self.scope_anchor_to_group.clear();
        for group in &self.groups {
            if let Some(scope_id) = group.scope_id {
                self.scope_anchor_to_group.insert(scope_id, group.anchor);
            }
        }
    }

    pub(super) fn recompute_group_spans(&mut self) {
        for group_index in 0..self.groups.len() {
            let node_count = self.group_node_len_at(group_index) as u32;
            let group = &mut self.groups[group_index];
            group.subtree_len = 1;
            group.subtree_node_count = node_count;
        }

        for index in (0..self.groups.len()).rev() {
            let parent_anchor = self.groups[index].parent_anchor;
            if !parent_anchor.is_valid() {
                continue;
            }
            let Some(parent_index) = self.anchors.active_index(parent_anchor) else {
                continue;
            };
            let subtree_len = self.groups[index].subtree_len;
            let subtree_nodes = self.groups[index].subtree_node_count;
            self.groups[parent_index].subtree_len += subtree_len;
            self.groups[parent_index].subtree_node_count += subtree_nodes;
        }
    }

    pub(in crate::slot) fn recompute_all_metadata(&mut self) {
        self.recompute_group_indexes();
        self.recompute_scope_index();
        self.recompute_group_spans();
    }

    pub(in crate::slot) fn refresh_group_indexes_from(&mut self, start: usize) {
        for index in start..self.groups.len() {
            self.anchors.set_active(self.groups[index].anchor, index);
        }
    }

    pub(in crate::slot) fn clear_group_indexes(&mut self, groups: &[GroupRecord]) {
        self.anchors.mark_detached_groups(groups);
    }

    pub(in crate::slot) fn clear_scope_index_for_groups(&mut self, groups: &[GroupRecord]) {
        for group in groups {
            if let Some(scope_id) = group.scope_id {
                self.scope_anchor_to_group.remove(&scope_id);
            }
        }
    }
}
