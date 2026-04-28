use super::super::{GroupRecord, SlotTable};
use super::SlotInvariantError;

pub(super) fn validate_scope_index_count(table: &SlotTable) -> Result<(), SlotInvariantError> {
    let scope_count = table
        .groups
        .iter()
        .filter(|group| group.scope_id.is_some())
        .count();

    if table.scope_index.by_scope.len() == scope_count {
        return Ok(());
    }

    Err(SlotInvariantError::ScopeIndexCountMismatch {
        expected: scope_count,
        actual: table.scope_index.by_scope.len(),
    })
}

pub(super) fn validate_active_group_scope(
    table: &SlotTable,
    group: &GroupRecord,
) -> Result<(), SlotInvariantError> {
    let Some(scope_id) = group.scope_id else {
        return Ok(());
    };

    let actual = table.scope_index.by_scope.get(&scope_id).copied();
    if actual == Some(group.anchor) {
        return Ok(());
    }

    Err(SlotInvariantError::ScopeIndexMismatch {
        scope_id,
        expected: group.anchor,
        actual,
    })
}
