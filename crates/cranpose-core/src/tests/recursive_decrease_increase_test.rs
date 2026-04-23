use crate::tests::test_composition;
use crate::{location_key, Composition, MemoryApplier, MutableState};
use cranpose_macros::composable;

/// Simple recursive function that creates keyed groups similar to the desktop demo
#[composable]
fn recursive_node(depth: usize, index: usize) {
    if depth > 1 {
        // Create two children at each level
        for child_idx in 0..2 {
            let child_key = (depth - 1, index * 2 + child_idx);
            crate::with_key(&child_key, || {
                recursive_node(depth - 1, index * 2 + child_idx);
            });
        }
    }
}

#[composable]
fn recursive_root(depth_state: MutableState<usize>) {
    // Read the state inside the composable to subscribe to changes
    let depth = depth_state.get();
    recursive_node(depth, 0);
}

/// Count all groups in the slot table
fn count_groups(composition: &Composition<MemoryApplier>) -> usize {
    composition.debug_dump_slot_table_groups().len()
}

#[test]
fn recursive_decrease_increase_preserves_structure() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let depth_state = MutableState::with_runtime(3usize, runtime.clone());

    let key = location_key(file!(), line!(), column!());

    // Initial render at depth 3
    eprintln!("\n=== Initial render at depth 3 ===");
    composition
        .render(key, &mut || {
            recursive_root(depth_state);
        })
        .expect("initial render");

    let initial_groups = count_groups(&composition);
    eprintln!("Groups: {}", initial_groups);
    eprintln!(
        "Group keys: {:?}",
        composition
            .debug_dump_slot_table_groups()
            .iter()
            .map(|(idx, key, _, _)| (idx, key))
            .collect::<Vec<_>>()
    );
    assert!(initial_groups > 0, "Should have groups at depth 3");

    // Decrease to depth 2
    eprintln!("\n=== Decrease to depth 2 ===");
    depth_state.set(2);
    let mut recomp_count = 0;
    while composition
        .process_invalid_scopes()
        .expect("recompose after decrease")
    {
        recomp_count += 1;
    }
    eprintln!("Recomposed {} times", recomp_count);

    let decreased_groups = count_groups(&composition);
    eprintln!("Groups: {}", decreased_groups);
    eprintln!(
        "Group keys: {:?}",
        composition
            .debug_dump_slot_table_groups()
            .iter()
            .map(|(idx, key, _, _)| (idx, key))
            .collect::<Vec<_>>()
    );
    eprintln!("Slot entries:");
    for entry in composition.debug_dump_slot_entries() {
        eprintln!("  [{}] {}", entry.path, entry.line);
    }

    // The important property is that the active tree shrinks after the render.
    assert!(
        decreased_groups < initial_groups,
        "Decreasing depth should reduce group count"
    );

    // Increase back to depth 3
    eprintln!("\n=== Increase back to depth 3 ===");
    depth_state.set(3);
    while composition
        .process_invalid_scopes()
        .expect("recompose after increase")
    {}

    let restored_groups = count_groups(&composition);
    eprintln!("Groups: {}", restored_groups);
    eprintln!(
        "Group keys: {:?}",
        composition
            .debug_dump_slot_table_groups()
            .iter()
            .map(|(idx, key, _, _)| (idx, key))
            .collect::<Vec<_>>()
    );
    eprintln!("Slot entries (first 30):");
    for entry in composition.debug_dump_slot_entries().iter().take(30) {
        eprintln!("  [{}] {}", entry.path, entry.line);
    }

    // After increasing back, we should have restored the original structure exactly
    eprintln!("\nComparison:");
    eprintln!("  Initial groups: {}", initial_groups);
    eprintln!("  Restored groups: {}", restored_groups);

    assert_eq!(
        restored_groups,
        initial_groups,
        "After decrease-increase cycle, should restore exact same number of groups. Initial: {}, Restored: {}",
        initial_groups,
        restored_groups
    );
}

#[test]
fn recursive_decrease_increase_multiple_cycles() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let depth_state = MutableState::with_runtime(3usize, runtime.clone());

    let key = location_key(file!(), line!(), column!());

    // Initial render at depth 3
    composition
        .render(key, &mut || {
            recursive_root(depth_state);
        })
        .expect("initial render");

    let initial_groups = count_groups(&composition);
    let initial_keys: Vec<_> = composition
        .debug_dump_slot_table_groups()
        .iter()
        .map(|(_idx, key, _, _)| *key)
        .collect();
    eprintln!("Initial keys: {:?}", initial_keys);

    // Do multiple decrease-increase cycles
    for cycle in 0..3 {
        eprintln!("\n=== Cycle {} ===", cycle);

        // Decrease
        depth_state.set(2);
        while composition.process_invalid_scopes().expect("recompose") {}

        eprintln!("After decrease");

        // Increase
        depth_state.set(3);
        while composition.process_invalid_scopes().expect("recompose") {}

        let groups = count_groups(&composition);
        let current_keys: Vec<_> = composition
            .debug_dump_slot_table_groups()
            .iter()
            .map(|(_idx, key, _, _)| *key)
            .collect();
        eprintln!(
            "After cycle {}: {} groups (initial: {})",
            cycle, groups, initial_groups
        );
        eprintln!("Current keys: {:?}", current_keys);

        // Check for duplicate keys
        let mut key_counts: crate::collections::map::HashMap<u64, i32> =
            crate::collections::map::HashMap::default();
        for k in &current_keys {
            *key_counts.entry(*k).or_insert(0) += 1;
        }
        for (k, count) in key_counts.iter() {
            if *count > 1 {
                eprintln!("DUPLICATE KEY FOUND: {:?} appears {} times", k, count);
            }
        }

        // Check for missing keys
        for k in &initial_keys {
            if !current_keys.contains(k) {
                eprintln!("MISSING KEY: {:?}", k);
            }
        }

        assert_eq!(
            groups, initial_groups,
            "After cycle {}: groups should be exactly preserved. Initial: {}, Current: {}",
            cycle, initial_groups, groups
        );
    }
}
