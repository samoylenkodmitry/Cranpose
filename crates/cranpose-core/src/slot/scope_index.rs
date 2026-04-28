use super::SlotTable;
use crate::{collections::map::HashMap, slot_storage::ActiveGroupId, AnchorId, ScopeId};

#[derive(Default)]
pub(crate) struct ScopeIndex {
    pub(super) by_scope: HashMap<ScopeId, AnchorId>,
}

impl ScopeIndex {
    pub(super) fn new() -> Self {
        Self::default()
    }
}

impl SlotTable {
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn scope_index_anchor(&self, scope_id: ScopeId) -> Option<AnchorId> {
        self.scope_index.by_scope.get(&scope_id).copied()
    }

    pub(super) fn active_group_for_scope(&self, scope_id: ScopeId) -> Option<ActiveGroupId> {
        let anchor = self.scope_index.by_scope.get(&scope_id).copied()?;
        let group_index = self.anchors.active_index(anchor)?;
        let group = &self.groups[group_index];
        (group.scope_id == Some(scope_id)).then(|| self.active_group_id_at_index(group_index))
    }

    pub(super) fn assign_active_group_scope(&mut self, group: ActiveGroupId, scope_id: ScopeId) {
        let group_index = self.checked_active_group_index(group);
        let group_anchor = self.groups[group_index].anchor;
        if let Some(existing_anchor) = self.scope_index.by_scope.get(&scope_id).copied() {
            assert_eq!(
                existing_anchor, group_anchor,
                "scope id must resolve to a single active group"
            );
        }

        let record = &mut self.groups[group_index];
        if let Some(previous) = record.scope_id.replace(scope_id) {
            self.scope_index.by_scope.remove(&previous);
        }
        self.scope_index.by_scope.insert(scope_id, group_anchor);
    }
}
