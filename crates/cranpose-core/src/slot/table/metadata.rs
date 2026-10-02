use super::{super::GroupRecord, SlotTable};

impl SlotTable {
    pub(in crate::slot) fn refresh_group_indexes_from(&mut self, start: usize) {
        self.refresh_group_indexes_in_range(start, self.groups.len());
    }

    pub(in crate::slot) fn refresh_group_indexes_in_range(&mut self, start: usize, end: usize) {
        assert!(start <= end, "group index refresh range must be ordered");
        assert!(
            end <= self.groups.len(),
            "group index refresh range must stay inside groups"
        );
        let span = end - start;
        self.diagnostics.record_group_index_refresh(span);

        for (index, group) in (start..end).zip(&self.groups[start..end]) {
            self.anchors.move_active(group.anchor, index);
        }
    }

    pub(in crate::slot) fn clear_group_indexes(&mut self, groups: &[GroupRecord]) {
        self.anchors.mark_detached_groups(groups);
        self.movables.forget_groups(groups);
    }
}
