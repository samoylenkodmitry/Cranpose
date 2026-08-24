use super::{
    super::{AnchorState, GroupKey, GroupRecord, SlotTable},
    SlotInvariantError,
};
use crate::{collections::map::HashSet, AnchorId};

pub(super) fn validate_active_group_anchor(
    table: &SlotTable,
    group_index: usize,
    group: &GroupRecord,
) -> Result<(), SlotInvariantError> {
    match table.anchors.state(group.anchor) {
        Some(AnchorState::Active(actual)) if actual == group_index => Ok(()),
        actual => Err(SlotInvariantError::AnchorMismatch {
            anchor: group.anchor,
            expected: group_index,
            actual,
        }),
    }
}

pub(super) fn validate_active_group_anchor_count(
    table: &SlotTable,
) -> Result<(), SlotInvariantError> {
    if table.anchors.active_len() == table.groups.len() {
        return Ok(());
    }

    Err(SlotInvariantError::GroupAnchorCountMismatch {
        expected: table.groups.len(),
        actual: table.anchors.active_len(),
    })
}

pub(super) fn validate_anchor_registry_integrity(
    table: &SlotTable,
) -> Result<(), SlotInvariantError> {
    table.anchors.validate_integrity()
}

pub(super) fn validate_active_group_anchor_entries(
    table: &SlotTable,
) -> Result<(), SlotInvariantError> {
    for (anchor, group_index) in table.anchors.active_entries() {
        let actual = table.groups.get(group_index).map(|group| group.anchor);
        if actual == Some(anchor) {
            continue;
        }
        return Err(SlotInvariantError::AnchorTargetMismatch {
            anchor,
            expected_group_index: group_index,
            actual,
        });
    }

    Ok(())
}

pub(super) fn validate_detached_anchor_set(
    root_key: GroupKey,
    groups: &[GroupRecord],
) -> Result<(), SlotInvariantError> {
    let mut anchor_set: HashSet<AnchorId> = HashSet::default();
    for anchor in groups.iter().map(|group| group.anchor) {
        if !anchor_set.insert(anchor) {
            return Err(SlotInvariantError::DetachedDuplicateAnchor { root_key, anchor });
        }
    }

    Ok(())
}
