use super::*;

#[test]
fn group_anchor_registry_lifecycle_reuses_invalidated_id_with_generation_bump() {
    let mut registry = AnchorRegistry::new();
    let stale_anchor = registry.allocate();

    assert_eq!(registry.state(stale_anchor), Some(AnchorState::Invalidated));
    assert_eq!(registry.active_len(), 0);
    assert_eq!(registry.detached_len(), 0);
    assert_eq!(registry.invalidated_len(), 1);
    assert_eq!(registry.free_len(), 0);

    registry.set_active(stale_anchor, 12);
    assert_eq!(registry.active_index(stale_anchor), Some(12));
    assert_eq!(registry.active_len(), 1);
    assert_eq!(registry.detached_len(), 0);
    assert_eq!(registry.invalidated_len(), 0);

    registry.mark_detached(stale_anchor);
    assert_eq!(registry.state(stale_anchor), Some(AnchorState::Detached));
    assert_eq!(registry.active_len(), 0);
    assert_eq!(registry.detached_len(), 1);
    assert_eq!(registry.free_len(), 0);

    assert!(registry.invalidate_state(stale_anchor));
    assert_eq!(registry.state(stale_anchor), None);
    assert_eq!(registry.active_index(stale_anchor), None);
    assert_eq!(registry.active_len(), 0);
    assert_eq!(registry.detached_len(), 0);
    assert_eq!(registry.invalidated_len(), 0);
    assert_eq!(registry.free_len(), 1);
    assert_eq!(registry.validate_integrity(), Ok(()));

    let reused_anchor = registry.allocate();
    assert_eq!(reused_anchor.id, stale_anchor.id);
    assert_eq!(reused_anchor.generation, stale_anchor.generation + 1);
    assert_eq!(registry.state(stale_anchor), None);
    assert_eq!(
        registry.state(reused_anchor),
        Some(AnchorState::Invalidated)
    );
    assert_eq!(registry.free_len(), 0);
    assert_eq!(registry.invalidated_len(), 1);

    registry.set_active(stale_anchor, 99);
    assert_eq!(
        registry.state(reused_anchor),
        Some(AnchorState::Invalidated),
        "stale generation must not reactivate a reused anchor id"
    );
    registry.mark_detached(stale_anchor);
    assert_eq!(
        registry.state(reused_anchor),
        Some(AnchorState::Invalidated),
        "stale generation must not detach a reused anchor id"
    );

    registry.set_active(reused_anchor, 7);
    assert_eq!(registry.active_index(reused_anchor), Some(7));
    assert_eq!(registry.active_index(stale_anchor), None);
    assert_eq!(registry.active_len(), 1);
    assert_eq!(registry.detached_len(), 0);
    assert_eq!(registry.invalidated_len(), 0);
    assert_eq!(registry.free_len(), 0);
    assert_eq!(registry.validate_integrity(), Ok(()));
}

#[test]
fn group_anchor_disposal_keeps_dense_storage_for_hot_path_reuse() {
    let mut registry = AnchorRegistry::new();
    let anchors = (0..32)
        .map(|index| {
            let anchor = registry.allocate();
            registry.set_active(anchor, index);
            anchor
        })
        .collect::<Vec<_>>();
    let dense_capacity_before = registry.storage.dense_capacity();
    assert!(dense_capacity_before >= anchors.len());

    for anchor in &anchors {
        registry.mark_detached(*anchor);
        assert!(registry.invalidate_state(*anchor));
    }

    assert_eq!(registry.active_len(), 0);
    assert_eq!(registry.detached_len(), 0);
    assert_eq!(registry.invalidated_len(), 0);
    assert_eq!(registry.free_len(), anchors.len());
    assert_eq!(
        registry.storage.dense_capacity(),
        dense_capacity_before,
        "group anchor disposal must not compact dense storage on the mutation hot path"
    );
    assert_eq!(registry.validate_integrity(), Ok(()));
}

#[test]
fn sparse_group_anchor_ids_do_not_grow_dense_registry_storage() {
    let mut registry = AnchorRegistry::new();
    let anchor = AnchorId {
        id: 2_500_000,
        generation: 1,
    };
    registry.next_anchor = anchor.id as usize + 1;

    assert!(
        registry
            .set_state(anchor, AnchorState::Invalidated)
            .is_none()
    );
    registry.set_active(anchor, 4);

    assert_eq!(registry.active_index(anchor), Some(4));
    assert_eq!(registry.slot_len(), 1);
    assert_eq!(registry.active_len(), 1);
    assert_eq!(registry.detached_len(), 0);
    assert_eq!(registry.invalidated_len(), 0);
    assert!(
        registry.capacity() < 128,
        "sparse group ids must not allocate dense registry storage: capacity={}",
        registry.capacity()
    );
    assert_eq!(registry.validate_integrity(), Ok(()));
}

#[test]
fn registry_integrity_rejects_free_active_anchor_id() {
    let mut registry = AnchorRegistry::new();
    let anchor = registry.allocate();
    registry.set_active(anchor, 0);
    registry.free_ids.push(Reverse(anchor.id));

    assert_eq!(
        registry.validate_integrity(),
        Err(SlotInvariantError::AnchorRegistryInternalMismatch {
            detail: "free anchor id must not be active or detached",
            anchor_id: Some(anchor.id),
            expected: 0,
            actual: 1,
        })
    );
}

#[test]
fn registry_integrity_rejects_reused_generation_without_free_id() {
    let mut registry = AnchorRegistry::new();
    let anchor = AnchorId {
        id: 7,
        generation: 2,
    };
    registry.reused_generations.insert(anchor.id, 3);

    assert_eq!(
        registry.validate_integrity(),
        Err(SlotInvariantError::AnchorRegistryInternalMismatch {
            detail: "reused anchor generation must belong to a free id",
            anchor_id: Some(anchor.id),
            expected: 1,
            actual: 0,
        })
    );
}
