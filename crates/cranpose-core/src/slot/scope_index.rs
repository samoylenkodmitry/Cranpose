#[cfg(any(test, debug_assertions))]
use super::SlotInvariantError;
use super::{ActiveGroupId, AnchorRegistry, GroupRecord, SlotTable};
use crate::{collections::map::HashMap, AnchorId, ScopeId};
use std::mem;

#[derive(Default)]
pub(crate) struct ScopeIndex {
    by_scope: HashMap<ScopeId, AnchorId>,
}

impl ScopeIndex {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn clear(&mut self) {
        self.by_scope.clear();
    }

    pub(super) fn shrink_to_fit(&mut self) {
        self.by_scope.shrink_to_fit();
    }

    pub(super) fn len(&self) -> usize {
        self.by_scope.len()
    }

    pub(super) fn capacity(&self) -> usize {
        self.by_scope.capacity()
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.by_scope.capacity() * mem::size_of::<(ScopeId, AnchorId)>()
    }

    pub(super) fn anchor(&self, scope_id: ScopeId) -> Option<AnchorId> {
        self.by_scope.get(&scope_id).copied()
    }

    pub(super) fn entries(&self) -> impl Iterator<Item = (ScopeId, AnchorId)> + '_ {
        self.by_scope
            .iter()
            .map(|(&scope_id, &anchor)| (scope_id, anchor))
    }

    pub(super) fn active_group_index(
        &self,
        scope_id: ScopeId,
        anchors: &AnchorRegistry,
        groups: &[GroupRecord],
    ) -> Option<usize> {
        let anchor = self.anchor(scope_id)?;
        let group_index = anchors.active_index(anchor)?;
        let group = &groups[group_index];
        (group.scope_id == Some(scope_id)).then_some(group_index)
    }

    pub(super) fn assign(&mut self, group: &mut GroupRecord, scope_id: ScopeId) {
        if let Some(existing_anchor) = self.anchor(scope_id) {
            assert_eq!(
                existing_anchor, group.anchor,
                "scope id must resolve to a single active group"
            );
        }

        if let Some(previous) = group.scope_id.replace(scope_id) {
            self.by_scope.remove(&previous);
        }
        self.by_scope.insert(scope_id, group.anchor);
    }

    pub(super) fn remove_groups(&mut self, groups: &[GroupRecord]) {
        for group in groups {
            if let Some(scope_id) = group.scope_id {
                self.by_scope.remove(&scope_id);
            }
        }
    }

    pub(super) fn restore_entries(
        &mut self,
        entries: impl IntoIterator<Item = (ScopeId, AnchorId)>,
    ) {
        for (scope_id, group_anchor) in entries {
            if let Some(existing_anchor) = self.anchor(scope_id) {
                assert_eq!(
                    existing_anchor, group_anchor,
                    "restored scope id must resolve to a single active group"
                );
            }
            self.by_scope.insert(scope_id, group_anchor);
        }
    }

    #[cfg(test)]
    pub(super) fn rebuild(&mut self, groups: &[GroupRecord]) {
        self.clear();
        for group in groups {
            if let Some(scope_id) = group.scope_id {
                self.by_scope.insert(scope_id, group.anchor);
            }
        }
    }

    #[cfg(any(test, debug_assertions))]
    pub(super) fn validate_count(&self, expected: usize) -> Result<(), SlotInvariantError> {
        if self.len() == expected {
            return Ok(());
        }

        Err(SlotInvariantError::ScopeIndexCountMismatch {
            expected,
            actual: self.len(),
        })
    }

    #[cfg(any(test, debug_assertions))]
    pub(super) fn validate_group(&self, group: &GroupRecord) -> Result<(), SlotInvariantError> {
        let Some(scope_id) = group.scope_id else {
            return Ok(());
        };

        let actual = self.anchor(scope_id);
        if actual == Some(group.anchor) {
            return Ok(());
        }

        Err(SlotInvariantError::ScopeIndexMismatch {
            scope_id,
            expected: group.anchor,
            actual,
        })
    }

    #[cfg(test)]
    pub(super) fn insert_for_test(&mut self, scope_id: ScopeId, anchor: AnchorId) {
        self.by_scope.insert(scope_id, anchor);
    }
}

impl SlotTable {
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn scope_index_anchor(&self, scope_id: ScopeId) -> Option<AnchorId> {
        self.scope_index.anchor(scope_id)
    }

    pub(super) fn active_group_for_scope(&self, scope_id: ScopeId) -> Option<ActiveGroupId> {
        let group_index =
            self.scope_index
                .active_group_index(scope_id, &self.anchors, &self.groups)?;
        Some(self.active_group_id_at_index(group_index))
    }

    pub(super) fn assign_active_group_scope(&mut self, group: ActiveGroupId, scope_id: ScopeId) {
        let group_index = self.checked_active_group_index(group);
        self.scope_index
            .assign(&mut self.groups[group_index], scope_id);
    }
}
