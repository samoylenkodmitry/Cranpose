use super::SlotTable;
use crate::{slot_storage::GroupId, ScopeId};

impl SlotTable {
    pub(super) fn group_for_scope(&self, scope_id: ScopeId) -> Option<GroupId> {
        let anchor = self.scope_anchor_to_group.get(&scope_id).copied()?;
        let group_index = self.anchors.active_index(anchor)?;
        let group = &self.groups[group_index];
        (group.scope_id == Some(scope_id)).then(|| self.group_id_at_index(group_index))
    }

    pub(super) fn assign_group_scope(&mut self, group: GroupId, scope_id: ScopeId) {
        let group_index = self.checked_group_index(group);
        let group_anchor = self.groups[group_index].anchor;
        if let Some(existing_anchor) = self.scope_anchor_to_group.get(&scope_id).copied() {
            assert_eq!(
                existing_anchor, group_anchor,
                "scope id must resolve to a single active group"
            );
        }

        let record = &mut self.groups[group_index];
        if let Some(previous) = record.scope_id.replace(scope_id) {
            self.scope_anchor_to_group.remove(&previous);
        }
        self.scope_anchor_to_group.insert(scope_id, group_anchor);
    }
}
