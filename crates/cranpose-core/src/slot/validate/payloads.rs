use super::super::{GroupRecord, PayloadRecord, SlotTable};
use super::{
    groups::{SlotTreeChecks, SlotTreeView},
    SlotInvariantError,
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
        return Err(view.kind.payload_start_mismatch(
            group_index,
            expected_payload_start,
            payload_start,
        ));
    }

    let payload_len = group.payload_len as usize;
    let payload_end = payload_start.saturating_add(payload_len);
    if payload_end > view.payloads.len() {
        return Err(view.kind.payload_out_of_range(
            group_index,
            payload_start,
            payload_len,
            view.payloads.len(),
        ));
    }

    for (payload_index, payload) in view.payloads[payload_start..payload_end].iter().enumerate() {
        if payload.owner != group.anchor {
            return Err(view.kind.payload_owner_mismatch(
                payload.anchor,
                group.anchor,
                payload.owner,
            ));
        }
        checks.validate_payload(group_index, group, payload_index, payload)?;
    }

    Ok(payload_end)
}

pub(super) fn validate_payload_location_count(table: &SlotTable) -> Result<(), SlotInvariantError> {
    if table.payload_locations.len() == table.payloads.len() {
        return Ok(());
    }

    Err(SlotInvariantError::PayloadAnchorCountMismatch {
        expected: table.payloads.len(),
        actual: table.payload_locations.len(),
    })
}

pub(super) fn validate_active_payload_location(
    table: &SlotTable,
    group: &GroupRecord,
    payload_index: usize,
    payload: &PayloadRecord,
) -> Result<(), SlotInvariantError> {
    let expected_location = (group.anchor, payload_index);
    let actual = table.payload_locations.get(payload.anchor);
    if actual == Some(expected_location) {
        return Ok(());
    }

    Err(SlotInvariantError::PayloadLocationMismatch {
        payload_anchor: payload.anchor,
        expected: expected_location,
        actual,
    })
}

pub(super) fn validate_payload_locations(table: &SlotTable) -> Result<(), SlotInvariantError> {
    for (payload_anchor, (owner, payload_index)) in table.payload_locations.iter() {
        let Some(group_index) = table.anchors.active_index(owner) else {
            return Err(SlotInvariantError::PayloadLocationMismatch {
                payload_anchor,
                expected: (owner, payload_index),
                actual: None,
            });
        };
        let actual = table
            .group_payload_records_at(group_index)
            .get(payload_index)
            .map(|payload| (payload.owner, payload.anchor));
        if actual != Some((owner, payload_anchor)) {
            return Err(SlotInvariantError::PayloadLocationMismatch {
                payload_anchor,
                expected: (owner, payload_index),
                actual: Some((owner, payload_index)),
            });
        }
    }

    Ok(())
}
