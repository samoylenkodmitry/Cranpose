use super::{
    super::{GroupRecord, SlotTable},
    SlotInvariantError,
};

pub(super) fn validate_scope_index_count(table: &SlotTable) -> Result<(), SlotInvariantError> {
    let scope_count = table
        .groups
        .iter()
        .filter(|group| group.scope_id.is_some())
        .count();

    table.scope_index.validate_count(scope_count)
}

pub(super) fn validate_active_group_scope(
    table: &SlotTable,
    group: &GroupRecord,
) -> Result<(), SlotInvariantError> {
    table.scope_index.validate_group(group)
}
