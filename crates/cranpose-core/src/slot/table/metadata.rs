use super::{super::GroupRecord, SlotTable};

/// How many inserts may leave group indexes stale before an insert
/// rewrites them.
const MAX_STALE_GROUP_INSERTS: usize = 32;

/// Group inserts not yet written into the later groups' anchor indexes.
///
/// Each insert moves every later group one place on. Rewriting their anchors
/// at once costs the rest of the table per insert, and new content inserts
/// one group at a time. Instead, an anchor at or after `from` holds an index
/// at most `inserts` places before its group, and
/// [`SlotTable::active_group_index`] finds the group within those places.
#[derive(Clone, Copy, Debug, Default)]
pub(in crate::slot) struct StaleGroupIndexes {
    from: usize,
    inserts: usize,
}

impl SlotTable {
    pub(in crate::slot) fn refresh_group_indexes_from(&mut self, start: usize) {
        self.refresh_group_indexes_in_range(start, self.groups.len());
    }

    pub(in crate::slot) fn refresh_group_indexes_in_range(&mut self, start: usize, end: usize) {
        if self.stale_group_indexes.inserts == 0 {
            self.write_group_indexes(start, end);
            return;
        }
        let from = self.stale_group_indexes.from.min(start);
        self.stale_group_indexes = StaleGroupIndexes::default();
        self.write_group_indexes(from, self.groups.len());
    }

    /// How many inserts the stale indexes trail by.
    pub(in crate::slot) fn stale_group_inserts(&self) -> usize {
        self.stale_group_indexes.inserts
    }

    /// Notes a group inserted at `index`, whose anchor already holds it.
    pub(in crate::slot) fn note_group_insert(&mut self, index: usize) {
        let stale = &mut self.stale_group_indexes;
        stale.from = if stale.inserts == 0 {
            index
        } else {
            stale.from.min(index)
        };
        stale.inserts += 1;
        if stale.inserts >= MAX_STALE_GROUP_INSERTS {
            self.flush_stale_group_indexes();
        }
    }

    /// Writes the indexes inserts left stale, before any other change moves
    /// groups.
    pub(in crate::slot) fn flush_stale_group_indexes(&mut self) {
        if self.stale_group_indexes.inserts > 0 {
            let from = self.stale_group_indexes.from;
            self.stale_group_indexes = StaleGroupIndexes::default();
            self.write_group_indexes(from, self.groups.len());
        }
    }

    /// The group index `anchor` holds, checked against the group there.
    /// Between inserts and their flush, a stale index lies at most the
    /// pending insert count before its group.
    pub(in crate::slot) fn resolve_group_index(
        &self,
        stored: usize,
        anchor: crate::AnchorId,
    ) -> Option<usize> {
        let stale = self.stale_group_indexes;
        if stale.inserts == 0 || stored < stale.from {
            return Some(stored);
        }
        let end = stored
            .saturating_add(stale.inserts + 1)
            .min(self.groups.len());
        self.groups
            .get(stored..end)?
            .iter()
            .position(|group| group.anchor == anchor)
            .map(|offset| stored + offset)
    }

    fn write_group_indexes(&mut self, start: usize, end: usize) {
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
