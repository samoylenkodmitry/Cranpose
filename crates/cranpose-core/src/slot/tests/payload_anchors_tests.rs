use super::*;

#[test]
fn payload_anchor_registry_lifecycle_reuses_invalidated_id_with_generation_bump() {
    let mut registry = PayloadAnchorRegistry::new();
    let owner = AnchorId::new(10);
    let stale_anchor = registry.allocate();

    assert_eq!(
        registry.state_kind(stale_anchor),
        Some(PayloadAnchorLifecycle::Detached)
    );
    assert_eq!(registry.active_len(), 0);
    assert_eq!(registry.detached_len(), 1);
    assert_eq!(registry.invalidated_len(), 0);
    assert_eq!(registry.free_len(), 0);

    registry.set_active(stale_anchor, owner, 3);
    assert_eq!(registry.active_location(stale_anchor), Some((owner, 3)));
    assert_eq!(
        registry.state_kind(stale_anchor),
        Some(PayloadAnchorLifecycle::Active)
    );
    assert_eq!(registry.active_len(), 1);
    assert_eq!(registry.detached_len(), 0);

    registry.mark_detached(stale_anchor);
    assert!(registry.is_detached(stale_anchor));
    assert_eq!(registry.active_location(stale_anchor), None);
    assert_eq!(registry.active_len(), 0);
    assert_eq!(registry.detached_len(), 1);

    assert!(registry.invalidate(stale_anchor));
    assert_eq!(
        registry.state_kind(stale_anchor),
        Some(PayloadAnchorLifecycle::Invalidated)
    );
    assert_eq!(registry.active_location(stale_anchor), None);
    assert_eq!(registry.active_len(), 0);
    assert_eq!(registry.detached_len(), 0);
    assert_eq!(registry.invalidated_len(), 0);
    assert_eq!(registry.free_len(), 1);
    assert_eq!(registry.validate_integrity(), Ok(()));

    let reused_anchor = registry.allocate();
    assert_eq!(reused_anchor.id(), stale_anchor.id());
    assert_eq!(reused_anchor.generation(), stale_anchor.generation() + 1);
    assert_eq!(registry.state_kind(stale_anchor), None);
    assert_eq!(
        registry.state_kind(reused_anchor),
        Some(PayloadAnchorLifecycle::Detached)
    );
    assert_eq!(registry.invalidated_len(), 0);
    assert_eq!(registry.free_len(), 0);

    registry.set_active(stale_anchor, owner, 99);
    assert_eq!(
        registry.state_kind(reused_anchor),
        Some(PayloadAnchorLifecycle::Detached),
        "stale generation rejection must preserve the reused payload anchor state"
    );
    registry.mark_detached(stale_anchor);
    assert_eq!(
        registry.state_kind(reused_anchor),
        Some(PayloadAnchorLifecycle::Detached),
        "stale generation must not detach a reused payload anchor id"
    );

    registry.set_active(reused_anchor, owner, 5);
    assert_eq!(registry.active_location(reused_anchor), Some((owner, 5)));
    assert_eq!(registry.active_location(stale_anchor), None);
    assert_eq!(registry.active_len(), 1);
    assert_eq!(registry.detached_len(), 0);
    assert_eq!(registry.invalidated_len(), 0);
    assert_eq!(registry.free_len(), 0);
    assert_eq!(registry.validate_integrity(), Ok(()));
}

#[test]
fn registry_integrity_rejects_free_active_payload_anchor_id() {
    let mut registry = PayloadAnchorRegistry::new();
    let anchor = registry.allocate();
    let id = u32::try_from(anchor.id()).unwrap();
    registry
        .free_ids
        .push(Reverse(FreePayloadAnchorIdRange::singleton(id)));
    registry.free_count += 1;

    assert_eq!(
        registry.validate_integrity(),
        Err(SlotInvariantError::PayloadAnchorRegistryInternalMismatch {
            detail: "free payload anchor id must not be active or detached",
            payload_anchor_id: Some(anchor.id()),
            expected: 0,
            actual: 1,
        })
    );
}

#[test]
fn registry_integrity_rejects_reused_generation_without_free_id() {
    let mut registry = PayloadAnchorRegistry::new();
    registry.reused_generations.insert(7, 3);

    assert_eq!(
        registry.validate_integrity(),
        Err(SlotInvariantError::PayloadAnchorRegistryInternalMismatch {
            detail: "reused payload anchor generation must belong to a free id",
            payload_anchor_id: Some(7),
            expected: 1,
            actual: 0,
        })
    );
}

#[test]
fn invalidated_payload_anchor_ids_coalesce_into_compact_free_ranges() {
    let mut registry = PayloadAnchorRegistry::new();
    let anchors = (0..16).map(|_| registry.allocate()).collect::<Vec<_>>();
    for anchor in &anchors {
        assert!(registry.invalidate(*anchor));
    }

    registry.shrink_to_fit();

    assert_eq!(registry.slot_len(), 0);
    assert_eq!(registry.invalidated_len(), 0);
    assert_eq!(registry.free_len(), anchors.len());
    assert_eq!(registry.free_ids.len(), 1);
    assert_eq!(registry.validate_integrity(), Ok(()));

    let reused = registry.allocate();
    assert_eq!(reused, anchors[0].with_generation(2));
    assert_eq!(registry.invalidated_len(), 0);
    assert_eq!(registry.free_len(), anchors.len() - 1);
    assert_eq!(
        registry.state_kind(anchors[1]),
        Some(PayloadAnchorLifecycle::Invalidated)
    );
}

#[test]
fn payload_anchor_disposal_keeps_dense_storage_for_hot_path_reuse() {
    let mut table = SlotTable::new();
    let owner = table.anchors.allocate();
    table.anchors.set_active(owner, 0);

    let mut payloads = Vec::new();
    for value in 0..32_i32 {
        let anchor = table.payload_anchors.allocate();
        table
            .payload_anchors
            .set_active(anchor, owner, value as usize);
        payloads.push(PayloadRecord {
            owner,
            anchor,
            type_id: std::any::TypeId::of::<i32>(),
            type_name: std::any::type_name::<i32>(),
            source: crate::slot::BRANCH_PATH_ROOT,
            kind: crate::slot::PayloadKind::Internal,
            value: Box::new(value),
            fresh: None,
        });
    }

    let dense_capacity_before = table.payload_anchors.storage.dense_capacity();
    assert!(dense_capacity_before >= payloads.len());

    table.invalidate_payload_anchors(&payloads);

    assert_eq!(table.payload_anchors.active_len(), 0);
    assert_eq!(table.payload_anchors.invalidated_len(), 0);
    assert_eq!(table.payload_anchors.free_len(), payloads.len());
    assert_eq!(
        table.payload_anchors.storage.dense_capacity(),
        dense_capacity_before,
        "payload disposal must not compact dense anchor storage on the mutation hot path"
    );
    assert_eq!(table.payload_anchors.validate_integrity(), Ok(()));
}

#[test]
fn sparse_payload_anchor_ids_do_not_grow_dense_registry_storage() {
    let mut registry = PayloadAnchorRegistry::new();
    let anchor = PayloadAnchor::new(2_500_000, 1);
    registry.next_id = anchor.id() + 1;

    assert!(
        registry
            .set_state(anchor, PayloadAnchorState::Detached)
            .is_none()
    );
    registry.set_active(anchor, AnchorId::new(1), 0);

    assert_eq!(
        registry.active_location(anchor),
        Some((AnchorId::new(1), 0))
    );
    assert_eq!(registry.slot_len(), 1);
    assert!(
        registry.capacity() < 128,
        "sparse payload ids must not allocate dense registry storage: capacity={}",
        registry.capacity()
    );
    assert_eq!(registry.validate_integrity(), Ok(()));
}
