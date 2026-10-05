use super::{
    super::{GroupRecord, PayloadRecord, SlotTable},
    SlotInvariantError,
    groups::{SlotTreeChecks, SlotTreeView},
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
    if payload_end > view.payloads.item_count() {
        return Err(view.payload_out_of_range(
            group_index,
            payload_start,
            payload_len,
            view.payloads.item_count(),
        ));
    }

    for payload_index in 0..payload_len {
        let Some(payload) = view.payloads.item(payload_start + payload_index) else {
            return Err(view.payload_out_of_range(
                group_index,
                payload_start,
                payload_len,
                view.payloads.item_count(),
            ));
        };
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
        let Some(group_index) = table.active_group_index(owner) else {
            return Err(SlotInvariantError::PayloadAnchorRegistryTargetMismatch {
                payload_anchor,
                expected_owner: owner,
                expected_payload_index: payload_index,
                actual: None,
            });
        };
        let actual = table
            .group_payload_record_at(group_index, payload_index)
            .map(|payload| payload.anchor);
        if actual != Some(payload_anchor) {
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
#[path = "tests/payloads_tests.rs"]
mod tests;
