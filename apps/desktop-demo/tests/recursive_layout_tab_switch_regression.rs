use cranpose_core::{
    debug_recompose_scope_registry_stats, debug_snapshot_pinning_stats,
    debug_snapshot_state_thread_local_stats, debug_snapshot_v2_stats, MutableState,
};
use cranpose_testing::ComposeTestRule;
use desktop_app::app::{
    combined_app, DemoTab, TEST_ACTIVE_TAB_STATE, TEST_RECURSIVE_LAYOUT_DEPTH_STATE,
};

fn with_active_tab<F>(f: F)
where
    F: FnOnce(&MutableState<DemoTab>),
{
    TEST_ACTIVE_TAB_STATE.with(|cell| {
        let state = *cell.borrow().as_ref().expect("active tab state registered");
        f(&state);
    });
}

fn with_recursive_depth<F>(f: F)
where
    F: FnOnce(&MutableState<usize>),
{
    TEST_RECURSIVE_LAYOUT_DEPTH_STATE.with(|cell| {
        let state = *cell
            .borrow()
            .as_ref()
            .expect("recursive layout depth state registered");
        f(&state);
    });
}

fn set_active_tab(tab: DemoTab) {
    with_active_tab(|state| state.set(tab));
}

fn set_recursive_depth(depth: usize) {
    with_recursive_depth(|state| state.set(depth));
}

fn wait_for_recursive_depth_registration(rule: &mut ComposeTestRule) {
    for _ in 0..20 {
        let registered = TEST_RECURSIVE_LAYOUT_DEPTH_STATE.with(|cell| cell.borrow().is_some());
        if registered {
            return;
        }
        rule.pump_until_idle()
            .expect("pump while waiting for recursive layout depth state registration");
    }
    panic!(
        "recursive layout depth state not registered. tree:\n{}",
        rule.dump_tree(),
    );
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
struct RuntimeLeakDebugStats {
    pass_stats: cranpose_core::CompositionPassDebugStats,
    slot_stats: cranpose_core::SlotTableDebugStats,
    observer_stats: cranpose_core::SnapshotStateObserverDebugStats,
    runtime_stats: cranpose_core::RuntimeDebugStats,
    state_arena_stats: cranpose_core::StateArenaDebugStats,
    recompose_scope_stats: cranpose_core::RecomposeScopeRegistryDebugStats,
    snapshot_v2_stats: cranpose_core::SnapshotV2DebugStats,
    snapshot_pinning_stats: cranpose_core::SnapshotPinningDebugStats,
    snapshot_state_stats: cranpose_core::SnapshotStateThreadLocalDebugStats,
}

fn capture_runtime_debug_stats(rule: &mut ComposeTestRule) -> RuntimeLeakDebugStats {
    let runtime = rule.composition().runtime_handle();
    RuntimeLeakDebugStats {
        pass_stats: rule.composition().debug_last_pass_stats(),
        slot_stats: rule.composition().debug_slot_table_stats(),
        observer_stats: rule.composition().debug_observer_stats(),
        runtime_stats: runtime.debug_stats(),
        state_arena_stats: runtime.state_arena_debug_stats(),
        recompose_scope_stats: debug_recompose_scope_registry_stats(),
        snapshot_v2_stats: debug_snapshot_v2_stats(),
        snapshot_pinning_stats: debug_snapshot_pinning_stats(),
        snapshot_state_stats: debug_snapshot_state_thread_local_stats(),
    }
}

#[test]
fn switching_away_from_deep_recursive_layout_releases_actual_app_tree() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());
    TEST_RECURSIVE_LAYOUT_DEPTH_STATE.with(|cell| cell.borrow_mut().take());

    let mut rule = ComposeTestRule::new();
    rule.set_content(combined_app)
        .expect("install combined app content");
    rule.pump_until_idle().expect("initial idle");

    let baseline_active = rule.applier_mut().len();
    let baseline_capacity = rule.applier_mut().capacity();
    let baseline_slots = rule.composition().debug_dump_all_slots().len();
    let baseline_recycled_nodes = rule.applier_mut().debug_recycled_node_count();
    let baseline_recycled_heap = rule.applier_mut().debug_recycled_node_heap_bytes();
    let baseline_runtime = capture_runtime_debug_stats(&mut rule);

    set_active_tab(DemoTab::Layout);
    rule.pump_until_idle()
        .expect("switch to recursive layout tab");
    wait_for_recursive_depth_registration(&mut rule);

    set_recursive_depth(15);
    rule.pump_until_idle()
        .expect("increase recursive layout depth");

    let peak_active = rule.applier_mut().len();
    let peak_slots = rule.composition().debug_dump_all_slots().len();
    let peak_recycled_nodes = rule.applier_mut().debug_recycled_node_count();
    let peak_recycled_heap = rule.applier_mut().debug_recycled_node_heap_bytes();
    let peak_runtime = capture_runtime_debug_stats(&mut rule);
    let peak_slot_value_types = rule.composition().debug_slot_value_type_counts(12);
    assert!(
        peak_active > baseline_active * 20,
        "expected deep recursive layout to grow active nodes: baseline={baseline_active} peak={peak_active}",
    );
    assert!(
        peak_slots > baseline_slots * 20,
        "expected deep recursive layout to grow slot count: baseline={baseline_slots} peak={peak_slots}",
    );

    set_active_tab(DemoTab::Counter);
    rule.pump_until_idle()
        .expect("switch away from recursive layout tab");

    let after_active = rule.applier_mut().len();
    let after_capacity = rule.applier_mut().capacity();
    let after_tombstones = rule.applier_mut().tombstone_count();
    let after_slots = rule.composition().debug_dump_all_slots().len();
    let after_recycled_nodes = rule.applier_mut().debug_recycled_node_count();
    let after_recycled_heap = rule.applier_mut().debug_recycled_node_heap_bytes();
    let after_runtime = capture_runtime_debug_stats(&mut rule);
    let after_slot_value_types = rule.composition().debug_slot_value_type_counts(12);

    println!(
        "baseline: active={baseline_active} capacity={baseline_capacity} slots={baseline_slots} recycled_nodes={baseline_recycled_nodes} recycled_heap={baseline_recycled_heap}"
    );
    println!("baseline runtime: {baseline_runtime:#?}");
    println!(
        "peak: active={peak_active} slots={peak_slots} recycled_nodes={peak_recycled_nodes} recycled_heap={peak_recycled_heap}"
    );
    println!("peak runtime: {peak_runtime:#?}");
    println!("peak slot value types: {peak_slot_value_types:#?}");
    println!(
        "after: active={after_active} capacity={after_capacity} tombstones={after_tombstones} slots={after_slots} recycled_nodes={after_recycled_nodes} recycled_heap={after_recycled_heap}"
    );
    println!("after runtime: {after_runtime:#?}");
    println!("after slot value types: {after_slot_value_types:#?}");

    assert!(
        after_active <= baseline_active + 24,
        "tab switch leaked active nodes: baseline={baseline_active} peak={peak_active} after={after_active}",
    );
    assert!(
        after_capacity <= baseline_capacity.max(256) * 4,
        "tab switch retained excess capacity: baseline={baseline_capacity} after={after_capacity} tombstones={after_tombstones}",
    );
    assert!(
        after_slots <= baseline_slots + 512,
        "tab switch leaked slot groups: baseline={baseline_slots} after={after_slots}",
    );
}
