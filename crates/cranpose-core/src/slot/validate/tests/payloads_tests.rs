use std::any::TypeId;

use super::*;
use crate::{
    AnchorId,
    slot::{GroupKey, PayloadAnchor, PayloadKind},
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
