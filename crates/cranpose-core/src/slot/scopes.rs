use super::{ActiveGroupId, SlotTable, SlotWriteSession};
use crate::RecomposeScope;

impl SlotTable {
    pub(crate) fn active_group_for_scope(&self, scope: &RecomposeScope) -> Option<ActiveGroupId> {
        let group_index = self.anchors.active_index(scope.group_anchor())?;
        let Some(group) = self.groups.get(group_index) else {
            log::error!(
                "scope {} points to active group index {group_index}, but the slot table has only {} active groups",
                scope.id(),
                self.groups.len()
            );
            return None;
        };
        if group.scope.as_ref() != Some(scope) {
            return None;
        }
        self.active_group_id_at_index(group_index)
    }

    pub(super) fn assign_active_group_scope(
        &mut self,
        group: ActiveGroupId,
        scope: RecomposeScope,
    ) -> bool {
        if let Some(holder) = self.active_group_for_scope(&scope)
            && holder != group
        {
            log::error!(
                "scope {} already belongs to active group {holder:?}; rejecting assignment to {group:?}",
                scope.id()
            );
            return false;
        }
        let Some(record) = self
            .active_group_index_for_handle(group)
            .and_then(|group_index| self.groups.get_mut(group_index))
        else {
            log::error!(
                "scope {} assignment ignored for stale group {group:?}",
                scope.id()
            );
            return false;
        };
        scope.set_group_anchor(record.anchor);
        record.scope = Some(scope);
        true
    }

    pub(crate) fn release_scopes(&mut self) {
        for group in &mut self.groups {
            if let Some(scope) = group.scope.take() {
                scope.deactivate();
            }
        }
    }

    pub(crate) fn scopes(&self) -> impl Iterator<Item = &RecomposeScope> + '_ {
        self.groups.iter().filter_map(|group| group.scope.as_ref())
    }
}

impl SlotWriteSession<'_> {
    pub(crate) fn for_each_subtree_scope(
        &self,
        group: ActiveGroupId,
        f: impl FnMut(&RecomposeScope),
    ) {
        let Some(root) = self.table.active_group_index_for_handle(group) else {
            return;
        };
        let end = root.saturating_add(self.table.group_subtree_len_at_index(root));
        self.table
            .groups
            .get(root..end)
            .into_iter()
            .flatten()
            .filter_map(|group| group.scope.as_ref())
            .for_each(f);
    }
}
