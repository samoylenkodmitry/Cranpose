use super::{
    super::{GroupRecord, PayloadRecord, SlotTable},
    groups::{SlotTreeChecks, SlotTreeView},
    PayloadAnchorRecord, SlotInvariantError,
};

pub(super) fn validate_group_payloads(
    view: &SlotTreeView<'_>,
    checks: &mut impl SlotTreeChecks,
    group_index: usize,
    group: &GroupRecord,
    expected_payload_start: usize,
) -> Result<usize, SlotInvariantError> {
    let payload_start = group.payload_start as usize;
    if payload_start != expected_payload_start {
        return Err(view.payload_start_mismatch(
            group_index,
            expected_payload_start,
            payload_start,
        ));
    }

    let payload_len = group.payload_len as usize;
    let payload_end = payload_start.saturating_add(payload_len);
    if payload_end > view.payloads.len() {
        return Err(view.payload_out_of_range(
            group_index,
            payload_start,
            payload_len,
            view.payloads.len(),
        ));
    }

    for (payload_index, payload) in view.payloads[payload_start..payload_end].iter().enumerate() {
        if payload.owner != group.anchor {
            return Err(view.payload_owner_mismatch(
                payload.anchor.id(),
                group.anchor,
                payload.owner,
            ));
        }
        checks.validate_payload(group_index, group, payload_index, payload)?;
    }

    Ok(payload_end)
}

pub(super) fn validate_active_payload_anchor(
    table: &SlotTable,
    group: &GroupRecord,
    payload_index: usize,
    payload: &PayloadRecord,
) -> Result<(), SlotInvariantError> {
    let expected_location = (group.anchor, payload_index);
    let actual = table.payload_anchors.active_location(payload.anchor);
    if actual == Some(expected_location) {
        return Ok(());
    }

    Err(SlotInvariantError::PayloadAnchorRegistryMismatch {
        payload_anchor: payload.anchor,
        expected: expected_location,
        actual,
    })
}

pub(super) fn validate_payload_anchor_registry_count(
    table: &SlotTable,
) -> Result<(), SlotInvariantError> {
    if table.payload_anchors.active_len() == table.payloads.len() {
        return Ok(());
    }

    Err(SlotInvariantError::PayloadAnchorRegistryCountMismatch {
        expected: table.payloads.len(),
        actual: table.payload_anchors.active_len(),
    })
}

pub(super) fn validate_payload_anchor_registry_integrity(
    table: &SlotTable,
) -> Result<(), SlotInvariantError> {
    table.payload_anchors.validate_integrity()
}

pub(super) fn validate_payload_anchor_registry(
    table: &SlotTable,
) -> Result<(), SlotInvariantError> {
    for (payload_anchor, (owner, payload_index)) in table.payload_anchors.active_entries() {
        let Some(group_index) = table.anchors.active_index(owner) else {
            return Err(SlotInvariantError::PayloadAnchorRegistryTargetMismatch {
                payload_anchor,
                expected_owner: owner,
                expected_payload_index: payload_index,
                actual: None,
            });
        };
        let actual = table
            .group_payload_records_at(group_index)
            .get(payload_index)
            .map(|payload| PayloadAnchorRecord {
                owner: payload.owner,
                payload_anchor: payload.anchor,
            });
        if actual
            != Some(PayloadAnchorRecord {
                owner,
                payload_anchor,
            })
        {
            return Err(SlotInvariantError::PayloadAnchorRegistryTargetMismatch {
                payload_anchor,
                expected_owner: owner,
                expected_payload_index: payload_index,
                actual,
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use super::*;
    use crate::{
        slot::{GroupKey, PayloadAnchor, PayloadKind},
        AnchorId,
    };

    fn one_payload_table() -> (SlotTable, AnchorId, PayloadAnchor) {
        let mut table = SlotTable::new();
        let owner = table.anchors.allocate();
        let payload_anchor = table.payload_anchors.allocate();
        table.groups.push(GroupRecord {
            key: GroupKey::new(9_000, None, 0),
            parent_anchor: AnchorId::INVALID,
            depth: 0,
            subtree_len: 1,
            payload_start: 0,
            payload_len: 1,
            node_start: 0,
            node_len: 0,
            subtree_node_count: 0,
            generation: 1,
            anchor: owner,
            scope_id: None,
            transparent: false,
        });
        table.payloads.push(PayloadRecord {
            owner,
            anchor: payload_anchor,
            type_id: TypeId::of::<i32>(),
            type_name: std::any::type_name::<i32>(),
            source: crate::slot::BRANCH_PATH_ROOT,
            kind: PayloadKind::Internal,
            value: Box::new(0_i32),
            fresh: None,
        });
        table.anchors.set_active(owner, 0);
        table.payload_anchors.set_active(payload_anchor, owner, 0);
        (table, owner, payload_anchor)
    }

    #[test]
    fn reverse_payload_anchor_registry_validation_reports_actual_record() {
        let (mut table, owner, actual_payload_anchor) = one_payload_table();
        let stale_payload_anchor = table.payload_anchors.allocate();
        table
            .payload_anchors
            .set_active(stale_payload_anchor, owner, 0);

        assert_eq!(
            validate_payload_anchor_registry(&table),
            Err(SlotInvariantError::PayloadAnchorRegistryTargetMismatch {
                payload_anchor: stale_payload_anchor,
                expected_owner: owner,
                expected_payload_index: 0,
                actual: Some(PayloadAnchorRecord {
                    owner,
                    payload_anchor: actual_payload_anchor,
                }),
            })
        );
    }
}
