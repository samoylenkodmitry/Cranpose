use super::super::GroupRecord;
use super::SlotTable;
use crate::{AnchorId, ScopeId};

impl SlotTable {
    pub(in crate::slot) fn recompute_scope_index(&mut self) {
        self.scope_anchor_to_group.clear();
        for group in &self.groups {
            if let Some(scope_id) = group.scope_id {
                self.scope_anchor_to_group.insert(scope_id, group.anchor);
            }
        }
    }

    pub(in crate::slot) fn refresh_group_indexes_from(&mut self, start: usize) {
        let span = self.groups.len().saturating_sub(start);
        self.mutation_debug_stats.record_group_index_refresh(span);

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

    pub(in crate::slot) fn restore_scope_index_entries(
        &mut self,
        entries: impl IntoIterator<Item = (ScopeId, AnchorId)>,
    ) {
        for (scope_id, group_anchor) in entries {
            if let Some(existing_anchor) = self.scope_anchor_to_group.get(&scope_id).copied() {
                assert_eq!(
                    existing_anchor, group_anchor,
                    "restored scope id must resolve to a single active group"
                );
            }
            self.scope_anchor_to_group.insert(scope_id, group_anchor);
        }
    }
}
