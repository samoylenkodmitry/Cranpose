use std::mem;

#[cfg(any(test, debug_assertions))]
use super::SlotInvariantError;
use super::{ActiveGroupId, AnchorRegistry, GroupRecord, SlotTable};
use crate::{collections::map::HashMap, AnchorId, ScopeId};

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
        let Some(group) = groups.get(group_index) else {
            log::error!(
                "scope id {scope_id:?} points to active group index {group_index}, but the slot table has only {} active groups",
                groups.len()
            );
            return None;
        };
        (group.scope_id == Some(scope_id)).then_some(group_index)
    }

    pub(super) fn assign(&mut self, group: &mut GroupRecord, scope_id: ScopeId) -> bool {
        if let Some(existing_anchor) = self.anchor(scope_id) {
            if existing_anchor != group.anchor {
                log::error!(
                    "scope id {scope_id:?} is already assigned to group anchor {existing_anchor:?}; rejecting assignment to {:?}",
                    group.anchor
                );
                return false;
            }
        }

        if let Some(previous) = group.scope_id.replace(scope_id) {
            self.by_scope.remove(&previous);
        }
        self.by_scope.insert(scope_id, group.anchor);
        true
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
                if existing_anchor != group_anchor {
                    log::error!(
                        "restored scope id {scope_id:?} is already assigned to active group anchor {existing_anchor:?}; skipping restored anchor {group_anchor:?}"
                    );
                    continue;
                }
            }
            self.by_scope.insert(scope_id, group_anchor);
        }
    }

    pub(super) fn restore_entries_available(&self, entries: &[(ScopeId, AnchorId)]) -> bool {
        let mut available = true;
        for &(scope_id, group_anchor) in entries {
            if let Some(existing_anchor) = self.anchor(scope_id) {
                if existing_anchor != group_anchor {
                    available = false;
                    log::error!(
                        "restored scope id {scope_id:?} is already assigned to active group anchor {existing_anchor:?}; restored anchor {group_anchor:?} needs a fresh scope"
                    );
                }
            }
        }
        available
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
        self.active_group_id_at_index(group_index)
    }

    pub(super) fn assign_active_group_scope(
        &mut self,
        group: ActiveGroupId,
        scope_id: ScopeId,
    ) -> bool {
        let group_index = group.index();
        let Some(record) = self.groups.get(group_index) else {
            log::error!(
                "scope id {scope_id:?} assignment ignored for missing active group index {group_index}"
            );
            return false;
        };
        if record.generation != group.generation() {
            log::error!(
                "scope id {scope_id:?} assignment ignored for stale active group handle at index {group_index}: handle generation {:?}, current generation {:?}",
                group.generation(),
                record.generation
            );
            return false;
        }
        if record.transparent {
            log::error!(
                "scope id {scope_id:?} assignment ignored for transparent branch group at index {group_index}"
            );
            return false;
        }
        self.scope_index
            .assign(&mut self.groups[group_index], scope_id)
    }

    /// Marks an active group as a lifecycle-transparent branch bracket.
    ///
    /// Refused for groups that carry a scope: a scoped group owns its
    /// children's lifecycle, which is exactly what a transparent bracket
    /// disclaims.
    pub(super) fn mark_group_transparent(&mut self, group: ActiveGroupId) -> bool {
        let group_index = group.index();
        let Some(record) = self.groups.get_mut(group_index) else {
            log::error!("transparent mark ignored for missing active group index {group_index}");
            return false;
        };
        if record.generation != group.generation() {
            log::error!(
                "transparent mark ignored for stale active group handle at index {group_index}: handle generation {:?}, current generation {:?}",
                group.generation(),
                record.generation
            );
            return false;
        }
        if record.scope_id.is_some() {
            log::error!(
                "transparent mark ignored for scoped group at index {group_index}: scope {:?}",
                record.scope_id
            );
            return false;
        }
        record.transparent = true;
        true
    }
}
