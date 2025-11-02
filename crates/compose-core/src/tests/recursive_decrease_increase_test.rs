use crate::{location_key, Composition, MemoryApplier, MutableState};
use compose_macros::composable;

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

/// Count all gaps with preserved group keys by inspecting the debug output
fn count_gap_groups(composition: &Composition<MemoryApplier>) -> usize {
    composition
        .debug_dump_all_slots()
        .iter()
        .filter(|(_idx, desc)| desc.contains("Gap") && desc.contains("was_group_key"))
        .count()
}

/// Get all gap keys
fn get_gap_keys(composition: &Composition<MemoryApplier>) -> Vec<u64> {
    composition
        .debug_dump_all_slots()
        .iter()
        .filter_map(|(_idx, desc)| {
            if desc.contains("Gap(was_group_key=") {
                // Extract key from string like "Gap(was_group_key=12345..."
                let key_start = desc.find("was_group_key=")? + "was_group_key=".len();
                let key_end = desc[key_start..].find(',')?;
                let key_str = &desc[key_start..key_start + key_end];
                key_str.parse::<u64>().ok()
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn recursive_decrease_increase_preserves_structure() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let depth_state = MutableState::with_runtime(3usize, runtime.clone());

    let key = location_key(file!(), line!(), column!());

    // Initial render at depth 3
    println!("\n=== Initial render at depth 3 ===");
    composition
        .render(key, &mut || {
            recursive_root(depth_state.clone());
        })
        .expect("initial render");

    let initial_groups = count_groups(&composition);
    let initial_gaps = count_gap_groups(&composition);
    println!(
        "Groups: {}, Gaps with keys: {}",
        initial_groups, initial_gaps
    );
    println!(
        "Group keys: {:?}",
        composition
            .debug_dump_slot_table_groups()
            .iter()
            .map(|(idx, key, _, _)| (idx, key))
            .collect::<Vec<_>>()
    );
    assert!(initial_groups > 0, "Should have groups at depth 3");

    // Decrease to depth 2
    println!("\n=== Decrease to depth 2 ===");
    depth_state.set(2);
    let mut recomp_count = 0;
    while composition
        .process_invalid_scopes()
        .expect("recompose after decrease")
    {
        recomp_count += 1;
    }
    println!("Recomposed {} times", recomp_count);

    let decreased_groups = count_groups(&composition);
    let decreased_gaps = count_gap_groups(&composition);
    println!(
        "Groups: {}, Gaps with keys: {}",
        decreased_groups, decreased_gaps
    );
    println!(
        "Group keys: {:?}",
        composition
            .debug_dump_slot_table_groups()
            .iter()
            .map(|(idx, key, _, _)| (idx, key))
            .collect::<Vec<_>>()
    );
    println!("All slots:");
    for (idx, desc) in composition.debug_dump_all_slots() {
        println!("  [{}] {}", idx, desc);
    }

    // After decrease, some groups should become gaps
    assert!(
        decreased_gaps > 0,
        "Decreasing depth should create gaps with preserved keys"
    );

    // Increase back to depth 3
    println!("\n=== Increase back to depth 3 ===");
    depth_state.set(3);
    while composition
        .process_invalid_scopes()
        .expect("recompose after increase")
    {}

    let restored_groups = count_groups(&composition);
    let restored_gaps = count_gap_groups(&composition);
    println!(
        "Groups: {}, Gaps with keys: {}",
        restored_groups, restored_gaps
    );
    println!(
        "Group keys: {:?}",
        composition
            .debug_dump_slot_table_groups()
            .iter()
            .map(|(idx, key, _, _)| (idx, key))
            .collect::<Vec<_>>()
    );
    println!("All slots (first 30):");
    for (idx, desc) in composition.debug_dump_all_slots().iter().take(30) {
        println!("  [{}] {}", idx, desc);
    }

    // After increasing back, we should have restored the original structure exactly
    println!("\nComparison:");
    println!("  Initial groups: {}", initial_groups);
    println!("  Restored groups: {}", restored_groups);

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
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let depth_state = MutableState::with_runtime(3usize, runtime.clone());

    let key = location_key(file!(), line!(), column!());

    // Initial render at depth 3
    composition
        .render(key, &mut || {
            recursive_root(depth_state.clone());
        })
        .expect("initial render");

    let initial_groups = count_groups(&composition);
    let initial_keys: Vec<_> = composition
        .debug_dump_slot_table_groups()
        .iter()
        .map(|(_idx, key, _, _)| *key)
        .collect();
    println!("Initial keys: {:?}", initial_keys);

    // Do multiple decrease-increase cycles
    for cycle in 0..3 {
        println!("\n=== Cycle {} ===", cycle);

        // Decrease
        depth_state.set(2);
        while composition.process_invalid_scopes().expect("recompose") {}

        let gaps_after_decrease = count_gap_groups(&composition);
        let gap_keys = get_gap_keys(&composition);
        println!(
            "After decrease: {} gaps with keys: {:?}",
            gaps_after_decrease, gap_keys
        );

        // Increase
        depth_state.set(3);
        while composition.process_invalid_scopes().expect("recompose") {}

        let groups = count_groups(&composition);
        let current_keys: Vec<_> = composition
            .debug_dump_slot_table_groups()
            .iter()
            .map(|(_idx, key, _, _)| *key)
            .collect();
        let gaps_after_increase = count_gap_groups(&composition);
        println!(
            "After cycle {}: {} groups (initial: {}), {} gaps remaining",
            cycle, groups, initial_groups, gaps_after_increase
        );
        println!("Current keys: {:?}", current_keys);

        // Check for duplicate keys
        let mut key_counts: crate::collections::map::HashMap<u64, i32> =
            crate::collections::map::HashMap::default();
        for k in &current_keys {
            *key_counts.entry(*k).or_insert(0) += 1;
        }
        for (k, count) in key_counts.iter() {
            if *count > 1 {
                println!("DUPLICATE KEY FOUND: {:?} appears {} times", k, count);
            }
        }

        // Check for missing keys
        for k in &initial_keys {
            if !current_keys.contains(k) {
                println!("MISSING KEY: {:?}", k);
            }
        }

        assert_eq!(
            groups, initial_groups,
            "After cycle {}: groups should be exactly preserved. Initial: {}, Current: {}",
            cycle, initial_groups, groups
        );
    }
}
