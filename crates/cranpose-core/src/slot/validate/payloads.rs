use super::super::GroupRecord;
use super::{
    tree::{SlotTreeChecks, SlotTreeView},
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
