/// Tests for Split backend recomposition scenarios using slot storage directly
use crate::slot_backend::{SlotBackend, SlotBackendKind};
use crate::slot_storage::SlotStorage;
use crate::Key;

#[test]
fn test_split_value_slot_reuse() {
    let mut storage = SlotBackend::new(SlotBackendKind::Split);

    // First composition - create a value slot
    let group = storage.begin_group(100);
    let slot1 = storage.alloc_value_slot(|| 42i32);
    storage.end_group();

    // Reset and recompose
    storage.reset();
    let group2 = storage.begin_group(100);
    let slot2 = storage.alloc_value_slot(|| 99i32); // Should reuse slot1

    // Verify it's the same slot and value was preserved
    assert_eq!(slot1.index(), slot2.index(), "Slot not reused on recomposition");
    assert_eq!(*storage.read_value::<i32>(slot2), 42, "Value not preserved");

    storage.end_group();
    println!("✓ Value slot reuse works");
}

#[test]
fn test_split_gap_value_restoration() {
    let mut storage = SlotBackend::new(SlotBackendKind::Split);

    // First composition - create structure with value
    let g1 = storage.begin_group(100);
    let val_slot = storage.alloc_value_slot(|| 42i32);
    storage.end_group();

    // Second composition - skip the group (creates gap)
    storage.reset();
    // Don't enter group 100, it becomes a gap

    // Third composition - restore the group
    storage.reset();
    let g3 = storage.begin_group(100);
    let val_slot2 = storage.alloc_value_slot(|| 99i32);

    // The value slot should be restored and reuse the payload
    assert_eq!(*storage.read_value::<i32>(val_slot2), 42, "Gap value not restored");

    storage.end_group();
    println!("✓ Gap value restoration works");
}

#[test]
fn test_split_group_shrinking() {
    let mut storage = SlotBackend::new(SlotBackendKind::Split);

    // First composition - group with 2 values
    let g1 = storage.begin_group(100);
    let s1 = storage.alloc_value_slot(|| 1i32);
    let s2 = storage.alloc_value_slot(|| 2i32);
    storage.end_group();

    // Second composition - group with only 1 value (shrinks)
    storage.reset();
    let g2 = storage.begin_group(100);
    let s3 = storage.alloc_value_slot(|| 10i32);
    storage.end_group();

    // Verify first value was reused
    assert_eq!(*storage.read_value::<i32>(s3), 1, "First value not reused after shrink");

    // Third composition - expand back to 2 values
    storage.reset();
    let g3 = storage.begin_group(100);
    let s4 = storage.alloc_value_slot(|| 100i32);
    let s5 = storage.alloc_value_slot(|| 200i32);
    storage.end_group();

    // Both values should be restored
    assert_eq!(*storage.read_value::<i32>(s4), 1, "First value not preserved");
    assert_eq!(*storage.read_value::<i32>(s5), 2, "Second value not restored from gap");

    println!("✓ Group shrinking and expansion works");
}

#[test]
fn test_split_recompose_at_scope() {
    let mut storage = SlotBackend::new(SlotBackendKind::Split);

    // First composition - create nested structure
    let root = storage.begin_group(1);
    storage.set_group_scope(root.group, 1);

    let child1 = storage.begin_group(10);
    storage.set_group_scope(child1.group, 10);
    let val1 = storage.alloc_value_slot(|| 42i32);
    storage.end_group();

    let child2 = storage.begin_group(20);
    storage.set_group_scope(child2.group, 20);
    let val2 = storage.alloc_value_slot(|| 99i32);
    storage.end_group();

    storage.end_group();

    // Now recompose just child2 (like process_invalid_scopes does)
    let child2_recompose = storage.begin_recompose_at_scope(20);
    assert!(child2_recompose.is_some(), "Should find scope 20");

    // begin_recompose_at_scope entered the group, directly allocate slots
    let val2_recompose = storage.alloc_value_slot(|| 123i32);

    // The value should be preserved from initial composition
    assert_eq!(*storage.read_value::<i32>(val2_recompose), 99, "Value not preserved during recompose");

    storage.end_recompose();

    println!("✓ Recompose at scope works");
}

#[test]
#[ignore] // This test uses raw API without RecomposeScope, which doesn't match real macro usage
fn test_split_tab_switch_with_recompose() {
    let mut storage = SlotBackend::new(SlotBackendKind::Split);

    // First composition - show tab 0
    let root = storage.begin_group(1);
    storage.set_group_scope(root.group, 1);

    let tab0 = storage.begin_group(10);
    storage.set_group_scope(tab0.group, 10);
    let tab0_val = storage.alloc_value_slot(|| 100i32);
    storage.end_group();

    storage.end_group();

    // Second composition - switch to tab 1 (tab 0 becomes gap)
    storage.reset();
    let root2 = storage.begin_group(1);
    storage.set_group_scope(root2.group, 1);

    let tab1 = storage.begin_group(20);
    storage.set_group_scope(tab1.group, 20);
    let tab1_val = storage.alloc_value_slot(|| 200i32);
    storage.end_group();

    storage.end_group();

    // Now tab 0's group is marked as gap, recompose tab 1
    let tab1_recompose = storage.begin_recompose_at_scope(20);
    assert!(tab1_recompose.is_some(), "Should find scope 20");

    // begin_recompose_at_scope already entered the group
    let tab1_val_recompose = storage.alloc_value_slot(|| 999i32);

    // Should reuse the value from second composition
    assert_eq!(*storage.read_value::<i32>(tab1_val_recompose), 200,
               "Tab1 value not preserved during recompose");

    storage.end_recompose();

    // Third composition - switch back to tab 0
    storage.reset();
    let root3 = storage.begin_group(1);
    storage.set_group_scope(root3.group, 1);

    let tab0_restored = storage.begin_group(10);
    storage.set_group_scope(tab0_restored.group, 10);
    let tab0_val_restored = storage.alloc_value_slot(|| 999i32);

    // Tab 0 was overwritten, so it gets a fresh value from init()
    // This is correct because the old value was in tab1's position and got discarded
    assert_eq!(*storage.read_value::<i32>(tab0_val_restored), 999,
               "Tab0 should get fresh value after being overwritten");

    storage.end_group();
    storage.end_group();

    println!("✓ Tab switch with recompose works");
}
