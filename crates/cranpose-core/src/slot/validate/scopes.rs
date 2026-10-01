use super::{super::GroupRecord, SlotInvariantError};

pub(super) fn validate_active_group_scope(group: &GroupRecord) -> Result<(), SlotInvariantError> {
    let Some(scope) = group.scope.as_ref() else {
        return Ok(());
    };
    let scope_anchor = scope.group_anchor();
    if scope_anchor == group.anchor {
        return Ok(());
    }
    Err(SlotInvariantError::ScopeAnchorMismatch {
        scope_id: scope.id(),
        group_anchor: group.anchor,
        scope_anchor,
    })
}
