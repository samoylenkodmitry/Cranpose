use super::super::GroupRecord;
use super::SlotTable;
use crate::{AnchorId, ScopeId};

impl SlotTable {
    #[cfg(test)]
    pub(in crate::slot) fn recompute_scope_index(&mut self) {
        let scope_count = self
            .groups
            .iter()
            .filter(|group| group.scope_id.is_some())
            .count();
        self.mutation_debug_stats
            .record_scope_index_rebuild(scope_count);
        self.scope_index.rebuild(&self.groups);
    }

    pub(in crate::slot) fn refresh_group_indexes_from(&mut self, start: usize) {
        let span = self.groups.len().saturating_sub(start);
        self.mutation_debug_stats.record_group_index_refresh(span);

        for index in start..self.groups.len() {
            let generation = self.allocate_group_generation();
            self.groups[index].generation = generation;
            self.anchors.set_active(self.groups[index].anchor, index);
        }
    }

    pub(in crate::slot) fn clear_group_indexes(&mut self, groups: &[GroupRecord]) {
        self.anchors.mark_detached_groups(groups);
    }

    pub(in crate::slot) fn clear_scope_index_for_groups(&mut self, groups: &[GroupRecord]) {
        self.scope_index.remove_groups(groups);
    }

    pub(in crate::slot) fn restore_scope_index_entries(
        &mut self,
        entries: impl IntoIterator<Item = (ScopeId, AnchorId)>,
    ) {
        self.scope_index.restore_entries(entries);
    }
}
