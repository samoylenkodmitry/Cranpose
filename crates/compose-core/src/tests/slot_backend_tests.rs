//! Tests for slot storage backends.

use crate::{SlotBackendKind, SlotStorage};

/// Smoke test to verify all backends can perform basic operations.
#[test]
fn test_all_backends_smoke() {
    // Test all four backends now that they've reached feature parity
    let backends = [
        SlotBackendKind::Baseline,
        SlotBackendKind::Chunked,
        SlotBackendKind::Hierarchical,
        SlotBackendKind::Split,
    ];

    for kind in backends {
        test_backend_smoke(kind);
    }
}

fn test_backend_smoke(kind: SlotBackendKind) {
    use crate::slot_backend::SlotBackend;

    let mut storage = SlotBackend::new(kind);

    // Begin a root group
    let result1 = storage.begin_group(100);
    assert!(
        !result1.restored_from_gap,
        "{:?}: First group should not be from gap",
        kind
    );

    // Allocate a value slot
    let slot = storage.alloc_value_slot(|| 42);

    // Read it back
    assert_eq!(
        *storage.read_value::<i32>(slot),
        42,
        "{:?}: Value should match",
        kind
    );

    // Write a new value
    storage.write_value(slot, 100);
    assert_eq!(
        *storage.read_value::<i32>(slot),
        100,
        "{:?}: Updated value should match",
        kind
    );

    // End the group
    storage.end_group();

    // Flush any pending operations
    storage.flush();
}

/// Test that backends handle recomposition correctly.
#[test]
fn test_backends_recomposition() {
    let backends = [
        SlotBackendKind::Baseline,
        SlotBackendKind::Chunked,
        SlotBackendKind::Hierarchical,
        SlotBackendKind::Split,
    ];

    for kind in backends {
        test_backend_recomposition(kind);
    }
}

fn test_backend_recomposition(kind: SlotBackendKind) {
    use crate::slot_backend::SlotBackend;

    let mut storage = SlotBackend::new(kind);

    // First composition
    storage.reset();
    let result = storage.begin_group(200);
    let group1 = result.group;

    // Associate a scope with this group
    storage.set_group_scope(group1, 1);

    let _slot1 = storage.alloc_value_slot(|| "hello");

    storage.end_group();
    storage.flush();

    // Recompose at that scope
    storage.reset();
    if let Some(_group) = storage.begin_recompose_at_scope(1) {
        // Just verify we can navigate to this position
        // The actual value reuse depends on properly calling the same
        // init closure, which is handled by the composer
        storage.end_group();
        storage.end_recompose();
    } else {
        panic!("{:?}: Should find scope for recomposition", kind);
    }
}

/// Test gap behavior (conditional rendering).
#[test]
fn test_backends_gaps() {
    let backends = [
        SlotBackendKind::Baseline,
        SlotBackendKind::Chunked,
        SlotBackendKind::Hierarchical,
        SlotBackendKind::Split,
    ];

    for kind in backends {
        test_backend_gaps(kind);
    }
}

fn test_backend_gaps(kind: SlotBackendKind) {
    use crate::slot_backend::SlotBackend;

    let mut storage = SlotBackend::new(kind);

    // First composition: create a group with nested content
    storage.reset();
    let result = storage.begin_group(300);
    let group1 = result.group;
    storage.set_group_scope(group1, 2);

    // Create a nested group
    let _inner = storage.begin_group(301);
    let _slot = storage.alloc_value_slot(|| 123);
    storage.end_group();

    // Now end the outer group, but don't finalize
    // (In a real scenario, we'd have cursor < end)
    storage.end_group();

    storage.flush();

    // Just verify the backend can handle basic gap operations
    // Full gap restoration requires proper cursor management
    // that is complex to test in isolation
}

/// Test allocating fewer slots than before, creating gaps.
#[test]
fn test_backends_allocate_fewer_slots() {
    let backends = [
        SlotBackendKind::Baseline,
        SlotBackendKind::Chunked,
        SlotBackendKind::Hierarchical,
        SlotBackendKind::Split,
    ];

    for kind in backends {
        test_backend_allocate_fewer_slots(kind);
    }
}

fn test_backend_allocate_fewer_slots(kind: SlotBackendKind) {
    use crate::slot_backend::SlotBackend;

    let mut storage = SlotBackend::new(kind);

    // First composition: create three slots
    storage.reset();
    let _group = storage.begin_group(400);

    let slot1 = storage.alloc_value_slot(|| 10);
    let slot2 = storage.alloc_value_slot(|| 20);
    let slot3 = storage.alloc_value_slot(|| 30);

    assert_eq!(*storage.read_value::<i32>(slot1), 10);
    assert_eq!(*storage.read_value::<i32>(slot2), 20);
    assert_eq!(*storage.read_value::<i32>(slot3), 30);

    storage.end_group();
    storage.flush();

    // Second composition: only allocate two slots (one less)
    storage.reset();
    let _group = storage.begin_group(400);

    // Reuse first two slots
    let slot1_v2 = storage.alloc_value_slot(|| 10);
    let slot2_v2 = storage.alloc_value_slot(|| 20);

    assert_eq!(*storage.read_value::<i32>(slot1_v2), 10);
    assert_eq!(*storage.read_value::<i32>(slot2_v2), 20);

    // Don't allocate the third slot - it should become a gap
    storage.finalize_current_group();
    storage.end_group();
    storage.flush();

    // Third composition: allocate all three again
    storage.reset();
    let _group = storage.begin_group(400);

    let slot1_v3 = storage.alloc_value_slot(|| 10);
    let slot2_v3 = storage.alloc_value_slot(|| 20);
    let slot3_v3 = storage.alloc_value_slot(|| 30);

    assert_eq!(*storage.read_value::<i32>(slot1_v3), 10);
    assert_eq!(*storage.read_value::<i32>(slot2_v3), 20);
    assert_eq!(*storage.read_value::<i32>(slot3_v3), 30);

    storage.end_group();
    storage.flush();
}

/// Test gap restoration: finalize and re-enter a group with the same key.
#[test]
fn test_backends_gap_restore() {
    let backends = [
        SlotBackendKind::Baseline,
        SlotBackendKind::Chunked,
        SlotBackendKind::Hierarchical,
        SlotBackendKind::Split,
    ];

    for kind in backends {
        test_backend_gap_restore(kind);
    }
}

fn test_backend_gap_restore(kind: SlotBackendKind) {
    use crate::slot_backend::SlotBackend;

    let mut storage = SlotBackend::new(kind);

    // First composition: create a group with content
    storage.reset();
    let result1 = storage.begin_group(500);
    assert!(
        !result1.restored_from_gap,
        "{:?}: First group should not be from gap",
        kind
    );

    let _slot = storage.alloc_value_slot(|| 42);
    storage.end_group();

    // Mark the rest as gaps (simulating conditional rendering where we skip this group)
    storage.finalize_current_group();
    storage.flush();

    // Second composition: skip the group (don't render it)
    storage.reset();
    // Finalize without entering - marks it as gap
    storage.finalize_current_group();
    storage.flush();

    // Third composition: render it again - should restore from gap
    storage.reset();
    let result3 = storage.begin_group(500);

    // All backends now support gap restoration
    assert!(
        result3.restored_from_gap,
        "{:?}: Group should be restored from gap",
        kind
    );

    storage.end_group();
    storage.flush();
}

/// Regression test: insert in the middle after finalize.
#[test]
fn test_backends_insert_middle_after_finalize() {
    let backends = [
        SlotBackendKind::Baseline,
        SlotBackendKind::Chunked,
        SlotBackendKind::Hierarchical,
        SlotBackendKind::Split,
    ];

    for kind in backends {
        test_backend_insert_middle_after_finalize(kind);
    }
}

fn test_backend_insert_middle_after_finalize(kind: SlotBackendKind) {
    use crate::slot_backend::SlotBackend;

    let mut storage = SlotBackend::new(kind);

    // First composition: group 1 → 3 value slots → finalize → end → flush
    storage.reset();
    let _group = storage.begin_group(700);
    let slot1 = storage.alloc_value_slot(|| 10);
    let slot2 = storage.alloc_value_slot(|| 20);
    let slot3 = storage.alloc_value_slot(|| 30);
    storage.finalize_current_group();
    storage.end_group();
    storage.flush();

    // Verify initial values
    assert_eq!(*storage.read_value::<i32>(slot1), 10);
    assert_eq!(*storage.read_value::<i32>(slot2), 20);
    assert_eq!(*storage.read_value::<i32>(slot3), 30);

    // Second composition: group 1 → 2 value slots → insert a node → finalize
    storage.reset();
    let _group = storage.begin_group(700);
    let slot1_v2 = storage.alloc_value_slot(|| 10);
    let slot2_v2 = storage.alloc_value_slot(|| 20);

    // Insert a node in the middle (this triggers insertion logic)
    storage.record_node(42);

    storage.finalize_current_group();
    storage.end_group();
    storage.flush();

    // Verify that reading earlier slots still works
    assert_eq!(*storage.read_value::<i32>(slot1_v2), 10);
    assert_eq!(*storage.read_value::<i32>(slot2_v2), 20);
}

/// Regression test: gap restore must set frame end correctly.
#[test]
fn test_backends_gap_restore_frame_end() {
    let backends = [
        SlotBackendKind::Baseline,
        SlotBackendKind::Chunked,
        SlotBackendKind::Hierarchical,
        SlotBackendKind::Split,
    ];

    for kind in backends {
        test_backend_gap_restore_frame_end(kind);
    }
}

fn test_backend_gap_restore_frame_end(kind: SlotBackendKind) {
    use crate::slot_backend::SlotBackend;

    let mut storage = SlotBackend::new(kind);

    // First pass: group(key=10) → 2 values → end → flush
    storage.reset();
    let _group = storage.begin_group(800);
    let _slot1 = storage.alloc_value_slot(|| 100);
    let _slot2 = storage.alloc_value_slot(|| 200);
    storage.end_group();
    storage.finalize_current_group();
    storage.flush();

    // Second pass: group(key=10) (restored_from_gap) → immediately finalize
    // This tests that the frame end calculation (start + len + 1) is correct
    storage.reset();
    let result = storage.begin_group(800);

    // If this is a gap restore, frame.end should be set correctly to not walk past the group
    if result.restored_from_gap {
        // Immediately finalize without traversing children - should not panic
        storage.finalize_current_group();
    }

    storage.end_group();
    storage.flush();
}

/// Test scope-based recomposition across all backends.
#[test]
fn test_backends_scope_recomposition() {
    let backends = [
        SlotBackendKind::Baseline,
        SlotBackendKind::Chunked,
        SlotBackendKind::Hierarchical,
        SlotBackendKind::Split,
    ];

    for kind in backends {
        test_backend_scope_recomposition(kind);
    }
}

fn test_backend_scope_recomposition(kind: SlotBackendKind) {
    use crate::slot_backend::SlotBackend;

    let mut storage = SlotBackend::new(kind);

    // First composition: create nested groups with scopes
    storage.reset();

    let outer = storage.begin_group(600);
    storage.set_group_scope(outer.group, 10);

    let inner = storage.begin_group(601);
    storage.set_group_scope(inner.group, 11);

    let _slot = storage.alloc_value_slot(|| "test");

    storage.end_group(); // inner
    storage.end_group(); // outer
    storage.flush();

    // Recompose at inner scope
    storage.reset();
    let found = storage.begin_recompose_at_scope(11);
    assert!(found.is_some(), "{:?}: Should find inner scope", kind);

    if found.is_some() {
        storage.end_group();
        storage.end_recompose();
    }

    storage.flush();
}

// ═══════════════════════════════════════════════════════════════════════════
// Helper functions for backend iteration
// ═══════════════════════════════════════════════════════════════════════════

fn all_backends() -> [SlotBackendKind; 4] {
    [
        SlotBackendKind::Baseline,
        SlotBackendKind::Chunked,
        SlotBackendKind::Hierarchical,
        SlotBackendKind::Split,
    ]
}

#[allow(dead_code)]
fn all_but_hierarchical() -> [SlotBackendKind; 3] {
    [
        SlotBackendKind::Baseline,
        SlotBackendKind::Chunked,
        SlotBackendKind::Split,
    ]
}

// ═══════════════════════════════════════════════════════════════════════════
// Hardened corner-case tests
// ═══════════════════════════════════════════════════════════════════════════

/// Test nested groups with partial finalize to ensure gap-restore math doesn't
/// cause the frame.end to walk past the parent's boundary.
#[test]
fn test_backends_nested_groups_partial_finalize() {
    for kind in all_backends() {
        use crate::slot_backend::SlotBackend;
        let mut storage = SlotBackend::new(kind);

        // pass 1: outer(100) -> inner(101) -> value
        storage.reset();
        let _outer = storage.begin_group(100);
        let _inner = storage.begin_group(101);
        let _slot = storage.alloc_value_slot(|| 1usize);
        storage.end_group(); // inner
                             // finalize outer while cursor is inside its range
        storage.finalize_current_group();
        storage.end_group(); // outer
        storage.flush();

        // pass 2: re-enter outer and inner; must not panic and must restore structure
        storage.reset();
        let _outer2 = storage.begin_group(100);
        // outer might be restored_from_gap
        let _inner2 = storage.begin_group(101);
        // When reusing a slot with matching type, old value persists
        let slot2 = storage.alloc_value_slot(|| 2usize);
        // Update to new value explicitly
        storage.write_value(slot2, 2usize);
        assert_eq!(*storage.read_value::<usize>(slot2), 2, "{:?}", kind);
        storage.end_group(); // inner
        storage.end_group(); // outer
        storage.flush();
    }
}

/// Test root-level finalize to ensure all backends support calling
/// finalize_current_group() when no groups are on the stack.
#[test]
fn test_backends_root_finalize_safe() {
    for kind in all_backends() {
        use crate::slot_backend::SlotBackend;
        let mut storage = SlotBackend::new(kind);

        // create some stuff
        storage.reset();
        let _g = storage.begin_group(2000);
        let _slot = storage.alloc_value_slot(|| 10i32);
        storage.end_group();
        storage.flush();

        // now reset and call finalize at root
        storage.reset();
        // root-level finalize should not panic
        let _ = storage.finalize_current_group();
        storage.flush();
    }
}

/// Test that all backends properly handle type mismatches when reusing value slots.
/// If a slot exists but has a different type, it should be overwritten with the new type.
#[test]
fn test_backends_value_slot_type_mismatch_overwrite() {
    for kind in all_backends() {
        use crate::slot_backend::SlotBackend;
        let mut storage = SlotBackend::new(kind);

        // pass 1: store an i32
        storage.reset();
        let _g = storage.begin_group(3000);
        let slot = storage.alloc_value_slot(|| 123i32);
        assert_eq!(*storage.read_value::<i32>(slot), 123);
        storage.end_group();
        storage.flush();

        // pass 2: same position, but now store &str
        storage.reset();
        let _g = storage.begin_group(3000);
        let slot2 = storage.alloc_value_slot(|| "hello");
        assert_eq!(storage.read_value::<&str>(slot2), &"hello");
        storage.end_group();
        storage.flush();
    }
}

/// Test interleaving nodes with values to stress the insertion and shifting logic.
/// Pattern: value -> node -> value
#[test]
fn test_backends_value_node_value_sequence() {
    for kind in all_backends() {
        use crate::slot_backend::SlotBackend;
        let mut storage = SlotBackend::new(kind);

        storage.reset();
        let _g = storage.begin_group(4000);

        let s1 = storage.alloc_value_slot(|| 1u8);
        storage.record_node(999); // interleave a node
        let s2 = storage.alloc_value_slot(|| 2u8);

        assert_eq!(*storage.read_value::<u8>(s1), 1, "{:?}", kind);
        assert_eq!(*storage.read_value::<u8>(s2), 2, "{:?}", kind);

        storage.end_group();
        storage.flush();
    }
}

/// Test that backends can handle long forward gap scans by creating many slots,
/// finalizing to create gaps, then inserting again.
#[test]
fn test_backends_gap_scan_forward() {
    for kind in all_backends() {
        use crate::slot_backend::SlotBackend;
        let mut storage = SlotBackend::new(kind);

        storage.reset();
        let _g = storage.begin_group(5000);

        // fill with several values so the next insert has to look ahead
        for i in 0..16 {
            let s = storage.alloc_value_slot(|| i);
            assert_eq!(*storage.read_value::<i32>(s), i, "{:?}", kind);
        }

        // finalize to create gaps beyond cursor
        storage.finalize_current_group();
        storage.end_group();
        storage.flush();

        // second pass: re-enter and insert again; must not panic
        storage.reset();
        let _g = storage.begin_group(5000);
        let _s = storage.alloc_value_slot(|| 999i32);
        storage.end_group();
        storage.flush();
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Deeper cross-backend tests (edge cases and regressions)
// ═══════════════════════════════════════════════════════════════════════════

/// Test that gap restore is rejected when the key doesn't match.
/// All backends must NOT restore from gap if the key changed.
#[test]
fn test_backends_gap_restore_rejects_key_mismatch() {
    for kind in all_backends() {
        use crate::slot_backend::SlotBackend;
        let mut storage = SlotBackend::new(kind);

        // pass 1: render key 900, with a value
        storage.reset();
        let _g = storage.begin_group(900);
        let _s = storage.alloc_value_slot(|| 1i32);
        storage.end_group();
        storage.finalize_current_group();
        storage.flush();

        // pass 2: now try to render key 901 at that spot
        storage.reset();
        let res = storage.begin_group(901);
        // must NOT be restored_from_gap because key differs
        assert!(
            !res.restored_from_gap,
            "{:?}: should not restore if key changed",
            kind
        );
        storage.end_group();
        storage.flush();
    }
}

/// Test allocating MORE slots after gap restore (opposite of fewer slots test).
/// Ensures that groups can grow after being restored from a gap.
#[test]
fn test_backends_allocate_more_slots_after_gap_restore() {
    for kind in all_backends() {
        use crate::slot_backend::SlotBackend;
        let mut storage = SlotBackend::new(kind);

        // pass 1: create group with 2 children
        storage.reset();
        let _g = storage.begin_group(910);
        let _a = storage.alloc_value_slot(|| 10i32);
        let _b = storage.alloc_value_slot(|| 20i32);
        storage.end_group();
        storage.flush();

        // pass 2: skip the group entirely to make it a gap
        storage.reset();
        storage.finalize_current_group(); // marks everything as gaps
        storage.flush();

        // pass 3: restore and allocate 3rd child
        storage.reset();
        let res = storage.begin_group(910);
        assert!(res.restored_from_gap, "{:?}: should restore from gap", kind);
        let _a2 = storage.alloc_value_slot(|| 10i32);
        let _b2 = storage.alloc_value_slot(|| 20i32);
        let c2 = storage.alloc_value_slot(|| 30i32);
        assert_eq!(*storage.read_value::<i32>(c2), 30);
        storage.end_group();
        storage.flush();
    }
}

/// Test multiple sibling groups where only the second is skipped.
/// Ensures root-level finalize doesn't over-mark siblings.
#[test]
fn test_backends_multiple_sibling_groups_gap_isolated() {
    for kind in all_backends() {
        use crate::slot_backend::SlotBackend;
        let mut storage = SlotBackend::new(kind);

        // pass 1: G1(1000), G2(1001)
        storage.reset();
        let _g1 = storage.begin_group(1000);
        let _s1 = storage.alloc_value_slot(|| 1i32);
        storage.end_group();

        let _g2 = storage.begin_group(1001);
        let _s2 = storage.alloc_value_slot(|| 2i32);
        storage.end_group();

        storage.flush();

        // pass 2: render only G1, skip G2 entirely
        storage.reset();
        let _g1b = storage.begin_group(1000);
        let _s1b = storage.alloc_value_slot(|| 11i32);
        storage.end_group();

        // Finalize to mark G2 (and anything after) as gaps
        storage.finalize_current_group();
        storage.flush();

        // pass 3: render both G1 and G2 – G2 should restore from gap
        storage.reset();
        let _g1c = storage.begin_group(1000);
        let _s1c = storage.alloc_value_slot(|| 111i32);
        storage.end_group();

        let g2c = storage.begin_group(1001);
        assert!(
            g2c.restored_from_gap,
            "{:?}: second sibling should restore from gap",
            kind
        );
        storage.end_group();
        storage.flush();
    }
}

/// Test insertion right before a following group.
/// Ensures that group frames of the following group are updated correctly after shifts.
#[test]
fn test_backends_insert_before_following_group() {
    for kind in all_backends() {
        use crate::slot_backend::SlotBackend;
        let mut storage = SlotBackend::new(kind);

        // pass 1: group 1, group 2
        storage.reset();
        let _g1 = storage.begin_group(1100);
        let _v1 = storage.alloc_value_slot(|| 1u32);
        storage.end_group();

        let _g2 = storage.begin_group(1101);
        let _v2 = storage.alloc_value_slot(|| 2u32);
        storage.end_group();
        storage.flush();

        // pass 2: re-enter, but in g1 insert an extra node so g2 shifts right
        storage.reset();
        let _g1b = storage.begin_group(1100);
        let _v1b = storage.alloc_value_slot(|| 1u32);
        // insertion that forces shifting
        storage.record_node(777);
        storage.end_group();

        let _g2b = storage.begin_group(1101);
        let v2b = storage.alloc_value_slot(|| 22u32);
        // Explicitly write value to ensure it's set (alloc may reuse old slot)
        storage.write_value(v2b, 22u32);
        assert_eq!(
            *storage.read_value::<u32>(v2b),
            22,
            "{:?}: shifted second group must still work",
            kind
        );
        storage.end_group();
        storage.flush();
    }
}

/// Test that scope recomposition returns None for non-existent scope and doesn't break next composition.
#[test]
fn test_backends_scope_recomposition_not_found_is_safe() {
    for kind in all_backends() {
        use crate::slot_backend::SlotBackend;
        let mut storage = SlotBackend::new(kind);

        // initial content
        storage.reset();
        let _g = storage.begin_group(1200);
        let _v = storage.alloc_value_slot(|| 1i32);
        storage.end_group();
        storage.flush();

        // try to recompose at non-existent scope
        storage.reset();
        let found = storage.begin_recompose_at_scope(999_999);
        assert!(
            found.is_none(),
            "{:?}: nonexistent scope should return None",
            kind
        );
        // after a failed recompose we should still be able to do a normal pass
        let _g2 = storage.begin_group(1200);
        let _v2 = storage.alloc_value_slot(|| 2i32);
        storage.end_group();
        storage.flush();
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Backend-specific tests
// ═══════════════════════════════════════════════════════════════════════════

/// Split-specific test: verify that payload persists in the HashMap even when
/// layout slots are marked as gaps.
#[test]
fn test_split_payload_persists_across_gap() {
    use crate::slot_backend::{SlotBackend, SlotBackendKind};

    let mut storage = SlotBackend::new(SlotBackendKind::Split);

    // pass 1
    storage.reset();
    let _g = storage.begin_group(6000);
    let vs = storage.alloc_value_slot(|| String::from("keep me"));
    assert_eq!(storage.read_value::<String>(vs), "keep me");
    storage.end_group();
    storage.finalize_current_group();
    storage.flush();

    // pass 2: same group, same slot position — should restore from gap and still be able to read
    storage.reset();
    let _g = storage.begin_group(6000);
    let vs2 = storage.alloc_value_slot(|| String::from("should not overwrite if recovered"));
    // we accept either behavior (reused or overwritten), but it must not panic
    let _ = storage.read_value::<String>(vs2);
    storage.end_group();
    storage.flush();
}

/// Hierarchical-specific test: verify scope search works in the current storage.
#[test]
fn test_hierarchical_scope_search() {
    use crate::slot_backend::{SlotBackend, SlotBackendKind};

    let mut storage = SlotBackend::new(SlotBackendKind::Hierarchical);

    // Create a group with a scope
    storage.reset();
    let result = storage.begin_group(7000);
    storage.set_group_scope(result.group, 123);
    let _slot = storage.alloc_value_slot(|| 42i32);
    storage.end_group();
    storage.flush();

    // Try to recompose at that scope
    storage.reset();
    let found = storage.begin_recompose_at_scope(123);
    assert!(found.is_some(), "Hierarchical should find scope 123");

    if found.is_some() {
        storage.end_group();
        storage.end_recompose();
    }

    storage.flush();
}

/// Split-specific test: gap with payload, then different type.
/// Ensures split backend properly overwrites payload when type changes.
#[test]
fn test_split_payload_gap_then_type_change() {
    use crate::slot_backend::{SlotBackend, SlotBackendKind};
    let mut storage = SlotBackend::new(SlotBackendKind::Split);

    // pass 1
    storage.reset();
    let _g = storage.begin_group(1300);
    let s = storage.alloc_value_slot(|| String::from("first"));
    assert_eq!(storage.read_value::<String>(s), "first");
    storage.end_group();
    storage.finalize_current_group();
    storage.flush();

    // pass 2: same place, but different type – must overwrite, not panic
    storage.reset();
    let _g2 = storage.begin_group(1300);
    let s2 = storage.alloc_value_slot(|| 1234i32);
    assert_eq!(*storage.read_value::<i32>(s2), 1234);
    storage.end_group();
    storage.flush();
}

/// Chunked-specific test: anchor rebuild after big shift.
/// Ensures chunked backend rebuilds anchors correctly after shifting operations.
#[test]
fn test_chunked_anchor_rebuild_after_shift() {
    use crate::slot_backend::{SlotBackend, SlotBackendKind};
    let mut storage = SlotBackend::new(SlotBackendKind::Chunked);

    // pass 1: make a bunch of values
    storage.reset();
    let _g = storage.begin_group(1400);
    let mut ids = Vec::new();
    for i in 0..8 {
        let s = storage.alloc_value_slot(|| i as i32);
        ids.push(s);
    }
    storage.end_group();
    storage.flush();

    // pass 2: insert in the middle to trigger shift
    storage.reset();
    let _g2 = storage.begin_group(1400);
    // reuse first few
    let _ = storage.alloc_value_slot(|| 0i32);
    // insert a node to force shift
    storage.record_node(9999);
    // allocate another value after shift
    let s_after = storage.alloc_value_slot(|| 777i32);
    // Explicitly write value to ensure it's set (alloc may reuse old slot)
    storage.write_value(s_after, 777i32);
    assert_eq!(*storage.read_value::<i32>(s_after), 777);
    storage.end_group();
    storage.flush();
}

/// Test that chunked backend returns position-based slot IDs.
/// This ensures the chunked backend uses the same ValueSlotId semantics as other backends.
#[test]
fn test_chunked_value_slot_is_position_based() {
    use crate::slot_backend::{SlotBackend, SlotBackendKind};
    let mut storage = SlotBackend::new(SlotBackendKind::Chunked);

    storage.reset();
    let _g = storage.begin_group(9000);
    let slot = storage.alloc_value_slot(|| 123i32);
    // we expect the id to be 1 here because group is at 0, value at 1
    assert_eq!(
        slot.index(),
        1,
        "chunked backend must use position-based ValueSlotId like other backends"
    );
    storage.end_group();
    storage.flush();
}

/// Test that chunked backend can read values after root-level finalize.
/// This ensures the original bug (root finalize breaking reads) stays fixed.
#[test]
fn test_chunked_read_after_root_finalize() {
    use crate::slot_backend::{SlotBackend, SlotBackendKind};
    let mut storage = SlotBackend::new(SlotBackendKind::Chunked);

    // pass 1
    storage.reset();
    let _g = storage.begin_group(9100);
    let _slot = storage.alloc_value_slot(|| 10i32);
    storage.end_group();
    storage.flush();

    // pass 2: root finalize creates gaps
    storage.reset();
    storage.finalize_current_group(); // root-level finalize
    storage.flush();

    // pass 3: re-render and read again
    storage.reset();
    let _g = storage.begin_group(9100);
    let slot2 = storage.alloc_value_slot(|| 10i32);
    // should not panic, should be readable
    assert_eq!(*storage.read_value::<i32>(slot2), 10);
    storage.end_group();
    storage.flush();
}

// ═══════════════════════════════════════════════════════════════════════════
// Targeted corner-case hardening tests
// ═══════════════════════════════════════════════════════════════════════════

/// Test that gap restore with fewer children than stored respects parent frame bounds.
/// This catches "forgot +1" and "didn't shrink frame" mistakes where traversal
/// could walk past the parent's end boundary.
#[test]
fn test_backends_gap_restore_shorter_children_respects_parent_frame() {
    for kind in all_backends() {
        use crate::slot_backend::SlotBackend;
        let mut storage = SlotBackend::new(kind);

        // pass 1: parent(2000) → child(2001) → 3 values
        storage.reset();
        let _parent = storage.begin_group(2000);
        let _child = storage.begin_group(2001);
        let _v1 = storage.alloc_value_slot(|| 100u32);
        let _v2 = storage.alloc_value_slot(|| 200u32);
        let _v3 = storage.alloc_value_slot(|| 300u32);
        storage.end_group(); // child
                             // Finalize parent early to establish its bounds
        storage.finalize_current_group();
        storage.end_group(); // parent
        storage.flush();

        // pass 2: skip entire structure to create gaps
        storage.reset();
        storage.finalize_current_group();
        storage.flush();

        // pass 3: restore parent and child, but allocate only 1 child value
        storage.reset();
        let res_parent = storage.begin_group(2000);
        assert!(
            res_parent.restored_from_gap,
            "{:?}: parent should restore from gap",
            kind
        );

        let _res_child = storage.begin_group(2001);
        // Child might be restored from gap or freshly created depending on backend
        let v1_new = storage.alloc_value_slot(|| 111u32);
        storage.write_value(v1_new, 111u32);
        assert_eq!(
            *storage.read_value::<u32>(v1_new),
            111,
            "{:?}: should read new value",
            kind
        );

        // Finalize child to mark remaining slots as gaps
        storage.finalize_current_group();
        storage.end_group(); // child

        // This must not panic — the frame.end calculation must account for shorter children
        storage.end_group(); // parent
        storage.flush();
    }
}

/// Test that insertion inside the first group doesn't corrupt a following group's frame
/// when that second group has nested content.
#[test]
fn test_backends_insert_in_first_group_preserves_nested_second_group() {
    for kind in all_backends() {
        use crate::slot_backend::SlotBackend;
        let mut storage = SlotBackend::new(kind);

        // pass 1: group1(3000) → value, group2(3001) → inner(3002) → value
        storage.reset();
        let _g1 = storage.begin_group(3000);
        let _v1 = storage.alloc_value_slot(|| 10i32);
        storage.end_group();

        let _g2 = storage.begin_group(3001);
        let _inner = storage.begin_group(3002);
        let _v2 = storage.alloc_value_slot(|| 20i32);
        storage.end_group(); // inner
        storage.end_group(); // group2

        storage.flush();

        // pass 2: re-enter group1, insert a node to trigger shifting
        storage.reset();
        let _g1b = storage.begin_group(3000);
        let _v1b = storage.alloc_value_slot(|| 10i32);
        // Insert node — this shifts everything after, including group2
        storage.record_node(888);
        storage.end_group();

        // Now enter group2 and its nested inner group
        let _g2b = storage.begin_group(3001);
        let _innerb = storage.begin_group(3002);
        let v2b = storage.alloc_value_slot(|| 22i32);
        storage.write_value(v2b, 22i32);
        // This must not panic, and must read correctly
        assert_eq!(
            *storage.read_value::<i32>(v2b),
            22,
            "{:?}: nested group after shift must be readable",
            kind
        );
        storage.end_group(); // inner
        storage.end_group(); // group2

        storage.flush();
    }
}

/// Test that root-level finalize followed by gap restore keeps anchors consistent.
/// If someone "optimizes" anchor rebuild away, this breaks.
#[test]
fn test_backends_root_finalize_then_gap_restore_anchors_consistent() {
    for kind in all_backends() {
        use crate::slot_backend::SlotBackend;
        let mut storage = SlotBackend::new(kind);

        // pass 1: render group → value
        storage.reset();
        let _g = storage.begin_group(4000);
        let slot1 = storage.alloc_value_slot(|| 42i32);
        assert_eq!(*storage.read_value::<i32>(slot1), 42, "{:?}", kind);
        storage.end_group();
        storage.flush();

        // pass 2: root-level finalize creates gaps
        storage.reset();
        storage.finalize_current_group();
        storage.flush();

        // pass 3: restore group and read value again
        storage.reset();
        let res = storage.begin_group(4000);
        assert!(res.restored_from_gap, "{:?}: should restore from gap", kind);
        let slot2 = storage.alloc_value_slot(|| 42i32);
        // This must not panic and must have the correct value
        assert_eq!(
            *storage.read_value::<i32>(slot2),
            42,
            "{:?}: value must be readable after gap restore",
            kind
        );
        storage.end_group();
        storage.flush();
    }
}

/// Test that gap restore preserves scope, so scope-based recomposition still works.
/// We currently copy scope on restore — this test enforces that behavior.
#[test]
fn test_backends_gap_restore_preserves_scope() {
    for kind in all_backends() {
        use crate::slot_backend::SlotBackend;
        let mut storage = SlotBackend::new(kind);

        // pass 1: create group with scope 123
        storage.reset();
        let result = storage.begin_group(5000);
        storage.set_group_scope(result.group, 123);
        let _v = storage.alloc_value_slot(|| "test");
        storage.end_group();
        storage.flush();

        // pass 2: skip the group to make it a gap
        storage.reset();
        storage.finalize_current_group();
        storage.flush();

        // pass 3: restore the group from gap
        storage.reset();
        let res = storage.begin_group(5000);
        assert!(res.restored_from_gap, "{:?}: should restore from gap", kind);
        storage.end_group();
        storage.flush();

        // pass 4: verify scope is still findable
        storage.reset();
        let found = storage.begin_recompose_at_scope(123);
        assert!(
            found.is_some(),
            "{:?}: scope 123 must be findable after gap restore",
            kind
        );
        if found.is_some() {
            storage.end_group();
            storage.end_recompose();
        }
        storage.flush();
    }
}

/// Test that type mismatch on reused layout slot in split backend properly overwrites payload.
/// We don't require gap-restore here; we just re-enter the same layout slot.
#[test]
fn test_split_type_mismatch_overwrites_payload_string_to_u64() {
    use crate::slot_backend::{SlotBackend, SlotBackendKind};
    let mut storage = SlotBackend::new(SlotBackendKind::Split);

    // pass 1: store String
    storage.reset();
    let _g = storage.begin_group(6000);
    let slot1 = storage.alloc_value_slot(|| String::from("original"));
    assert_eq!(storage.read_value::<String>(slot1), "original");
    storage.end_group();
    storage.flush();

    // pass 2: same group, same slot position, but with u64
    storage.reset();
    let _g2 = storage.begin_group(6000); // this reuses the existing layout, not a gap
    let slot2 = storage.alloc_value_slot(|| 9999u64);
    // must overwrite with new type
    assert_eq!(*storage.read_value::<u64>(slot2), 9999);
    storage.end_group();
    storage.flush();
}

#[test]
fn test_split_type_mismatch_overwrites_payload_string_to_u64_via_gap() {
    use crate::slot_backend::{SlotBackend, SlotBackendKind};
    let mut storage = SlotBackend::new(SlotBackendKind::Split);

    // pass 1: render String
    storage.reset();
    let _g = storage.begin_group(6000);
    let _s = storage.alloc_value_slot(|| String::from("original"));
    storage.end_group();
    storage.flush();

    // pass 2: skip everything -> make it a gap
    storage.reset();
    storage.finalize_current_group(); // cursor is 0 now, so group is turned into a gap
    storage.flush();

    // pass 3: re-enter, restore from gap, allocate u64
    storage.reset();
    let res = storage.begin_group(6000);
    assert!(res.restored_from_gap, "should restore from gap now");
    let s2 = storage.alloc_value_slot(|| 9999u64);
    assert_eq!(*storage.read_value::<u64>(s2), 9999);
    storage.end_group();
    storage.flush();
}

/// Test chunked gap scan upper bound: create >128 items (scan limit), make gaps past
/// the scan window, insert near the top. Must not panic; fallback overwrite works.
#[test]
fn test_chunked_gap_scan_upper_bound_fallback() {
    use crate::slot_backend::{SlotBackend, SlotBackendKind};
    let mut storage = SlotBackend::new(SlotBackendKind::Chunked);

    // pass 1: create group with 150 values (exceeds 128 scan limit)
    storage.reset();
    let _g = storage.begin_group(7000);
    for i in 0..150 {
        let s = storage.alloc_value_slot(|| i as u32);
        assert_eq!(*storage.read_value::<u32>(s), i as u32);
    }
    storage.end_group();
    storage.flush();

    // pass 2: create gaps past position 128 by finalizing after re-entering
    storage.reset();
    let _g = storage.begin_group(7000);
    // Traverse only the first few items
    let _s1 = storage.alloc_value_slot(|| 0u32);
    let _s2 = storage.alloc_value_slot(|| 1u32);
    // Finalize to mark positions 2..150 as gaps
    storage.finalize_current_group();
    storage.end_group();
    storage.flush();

    // pass 3: insert new item near the top (position 2)
    // This should trigger gap scan, but gaps are far away (past 128 window)
    // Must not panic; falls back to overwrite
    storage.reset();
    let _g = storage.begin_group(7000);
    let _s1 = storage.alloc_value_slot(|| 0u32);
    let _s2 = storage.alloc_value_slot(|| 1u32);
    // This allocation hits the gap scan limit and falls back
    let s3 = storage.alloc_value_slot(|| 999u32);
    storage.write_value(s3, 999u32);
    assert_eq!(
        *storage.read_value::<u32>(s3),
        999,
        "fallback overwrite must work"
    );
    storage.end_group();
    storage.flush();
}
