use std::cell::{Cell, RefCell};

use super::{
    begin_group_for_test,
    boundary_policy::BoundaryTransition,
    compact_for_test, drain_orphaned_node_ids_for_test, hide_range_for_test,
    preserved_orphaned_node_ids_for_test,
    reuse_planner::{ReusePlanner, ReusePlannerContext, StartPlan},
    storage::{EntryClass, EntryKind, GroupSpans},
    verifier::SlotTableVerifier,
    GroupFrame, NodeSlotState, PassBoundary, SlotLifecycleCoordinator, SlotPassMode, SlotTable,
    SlotWriteSessionState,
};
use crate::{
    runtime::TestRuntime, AnchorId, GroupId, Owned, RecomposeScope, ScopeId, StartScopedGroup,
};

thread_local! {
    static TEST_LIFECYCLE: RefCell<SlotLifecycleCoordinator> =
        RefCell::new(SlotLifecycleCoordinator::default());
}

fn with_test_lifecycle<R>(f: impl FnOnce(&mut SlotLifecycleCoordinator) -> R) -> R {
    TEST_LIFECYCLE.with(|slot| f(&mut slot.borrow_mut()))
}

fn reset_session(state: &mut SlotWriteSessionState) {
    *state = SlotWriteSessionState::default();
}

fn new_table() -> SlotTable {
    with_test_lifecycle(|lifecycle| *lifecycle = SlotLifecycleCoordinator::default());
    SlotTable::new()
}

fn begin_scoped_group(
    table: &mut SlotTable,
    state: &mut SlotWriteSessionState,
    key: crate::Key,
    init_scope: impl FnOnce() -> RecomposeScope,
) -> StartScopedGroup<GroupId> {
    let started = with_test_lifecycle(|lifecycle| {
        table
            .write_session(lifecycle, state, SlotPassMode::Compose)
            .begin_scoped_group(key, init_scope)
    });
    started.scope.set_group_anchor(started.anchor);
    started
}

fn use_value_slot<T: 'static>(
    table: &mut SlotTable,
    state: &mut SlotWriteSessionState,
    init: impl FnOnce() -> T,
) -> usize {
    with_test_lifecycle(|lifecycle| {
        table
            .write_session(lifecycle, state, SlotPassMode::Compose)
            .use_value_slot(init)
    })
}

fn record_node(
    table: &mut SlotTable,
    state: &mut SlotWriteSessionState,
    id: crate::NodeId,
    generation: u32,
) {
    with_test_lifecycle(|lifecycle| {
        table
            .write_session(lifecycle, state, SlotPassMode::Compose)
            .record_node(id, generation);
    });
}

fn end_group(table: &mut SlotTable, state: &mut SlotWriteSessionState) {
    with_test_lifecycle(|lifecycle| {
        table
            .write_session(lifecycle, state, SlotPassMode::Compose)
            .end_group();
    });
}

fn start_recompose_at_anchor(
    table: &mut SlotTable,
    state: &mut SlotWriteSessionState,
    anchor: crate::AnchorId,
    owner: ScopeId,
) -> Option<GroupId> {
    with_test_lifecycle(|lifecycle| {
        table
            .write_session(lifecycle, state, SlotPassMode::Recompose)
            .start_recranpose_at_anchor(anchor, owner)
    })
}

fn start_recompose_at_index(table: &SlotTable, state: &mut SlotWriteSessionState, index: usize) {
    table.start_recompose_entry(state, index);
}

fn end_recompose(table: &mut SlotTable, state: &mut SlotWriteSessionState) {
    with_test_lifecycle(|lifecycle| {
        table
            .write_session(lifecycle, state, SlotPassMode::Recompose)
            .end_recompose();
    });
}

fn finalize_current_group(table: &mut SlotTable, state: &mut SlotWriteSessionState) -> bool {
    with_test_lifecycle(|lifecycle| {
        table
            .write_session(lifecycle, state, SlotPassMode::Compose)
            .finalize_current_group()
    })
}

fn hide_range(table: &mut SlotTable, start: usize, end: usize, owner_index: Option<usize>) -> bool {
    with_test_lifecycle(|lifecycle| hide_range_for_test(table, lifecycle, start, end, owner_index))
}

fn drain_orphaned(table: &mut SlotTable) -> Vec<super::OrphanedNode> {
    let _ = table;
    with_test_lifecycle(drain_orphaned_node_ids_for_test)
}

fn preserved_orphaned(table: &SlotTable) -> Vec<super::OrphanedNode> {
    let _ = table;
    with_test_lifecycle(|lifecycle| preserved_orphaned_node_ids_for_test(lifecycle))
}

fn compact(table: &mut SlotTable) {
    with_test_lifecycle(|lifecycle| compact_for_test(table, lifecycle));
}

fn verify_table(table: &SlotTable) -> Result<(), String> {
    with_test_lifecycle(|lifecycle| SlotTableVerifier::new(table, Some(lifecycle)).verify())
}

#[derive(Debug, PartialEq, Eq)]
enum PlannedAction {
    ReuseLiveAtCursor {
        extent: usize,
        boundary_key: crate::Key,
    },
    RestoreHiddenAtCursor {
        extent: usize,
        boundary_key: crate::Key,
    },
    RestoreMatchingGroup {
        index: usize,
        extent: usize,
        boundary_key: crate::Key,
        reused_hidden: bool,
        retire_conflicting_group_at_cursor: bool,
    },
    InsertFresh {
        retire_conflicting_group_at_cursor: bool,
    },
}

fn describe_plan(plan: StartPlan) -> PlannedAction {
    match plan {
        StartPlan::ReuseLiveAtCursor {
            scan_extent,
            boundary_key,
            ..
        } => PlannedAction::ReuseLiveAtCursor {
            extent: scan_extent,
            boundary_key,
        },
        StartPlan::RestoreHiddenAtCursor {
            scan_extent,
            boundary_key,
            ..
        } => PlannedAction::RestoreHiddenAtCursor {
            extent: scan_extent,
            boundary_key,
        },
        StartPlan::RestoreMatchingGroup {
            matched_group,
            retire_conflicting_group_at_cursor,
        } => PlannedAction::RestoreMatchingGroup {
            index: matched_group.index,
            extent: matched_group.scan_extent,
            boundary_key: matched_group.boundary_key,
            reused_hidden: matched_group.reused_hidden,
            retire_conflicting_group_at_cursor,
        },
        StartPlan::InsertFresh {
            retire_conflicting_group_at_cursor,
        } => PlannedAction::InsertFresh {
            retire_conflicting_group_at_cursor,
        },
    }
}

fn plan_start(
    table: &SlotTable,
    key: crate::Key,
    cursor: usize,
    parent_end: usize,
    parent_boundary: PassBoundary,
    current_parent_boundary_key: Option<crate::Key>,
) -> PlannedAction {
    plan_start_with_parent_anchor(
        table,
        key,
        cursor,
        parent_end,
        parent_boundary,
        current_parent_boundary_key,
        AnchorId::INVALID,
    )
}

fn plan_start_with_parent_anchor(
    table: &SlotTable,
    key: crate::Key,
    cursor: usize,
    parent_end: usize,
    parent_boundary: PassBoundary,
    current_parent_boundary_key: Option<crate::Key>,
    current_parent_anchor: AnchorId,
) -> PlannedAction {
    let previous_live_sibling_group =
        derive_previous_live_sibling_group_anchor(table, cursor, current_parent_anchor);
    describe_plan(
        ReusePlanner::new(
            &table.storage,
            ReusePlannerContext {
                key,
                cursor,
                parent_end,
                parent_boundary,
                current_parent_boundary_key,
                current_parent_anchor,
                previous_live_sibling_group,
            },
        )
        .plan(),
    )
}

fn derive_previous_live_sibling_group_anchor(
    table: &SlotTable,
    cursor: usize,
    current_parent_anchor: AnchorId,
) -> Option<AnchorId> {
    let mut search_index = cursor;
    while search_index > 0 {
        search_index -= 1;
        let kind = table.storage.entry_kind(search_index)?;
        if !kind.matches(EntryClass::Group, super::storage::EntryVisibility::Live) {
            continue;
        }
        if table
            .storage
            .group_parent_anchor_at(search_index)
            .unwrap_or(AnchorId::INVALID)
            != current_parent_anchor
        {
            continue;
        }
        let group_end = search_index + table.storage.entry_scan_extent(search_index).max(1);
        if group_end == cursor {
            return Some(table.storage.entry_anchor(search_index));
        }
    }
    None
}

#[test]
fn large_slot_tables_grow_incrementally_instead_of_doubling() {
    assert_eq!(SlotTable::next_slot_target_len(0), SlotTable::INITIAL_CAP);
    assert_eq!(
        SlotTable::next_slot_target_len(SlotTable::INITIAL_CAP),
        SlotTable::INITIAL_CAP * 2
    );
    assert_eq!(
        SlotTable::next_slot_target_len(SlotTable::LARGE_GROWTH_THRESHOLD),
        SlotTable::LARGE_GROWTH_THRESHOLD
            + (SlotTable::LARGE_GROWTH_THRESHOLD / SlotTable::LARGE_GROWTH_DIVISOR)
    );
}

#[test]
fn trim_marks_values_hidden_and_compaction_removes_them() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();
    use_value_slot(&mut table, &mut state, || 1i32);
    use_value_slot(&mut table, &mut state, || 2i32);
    use_value_slot(&mut table, &mut state, || 3i32);

    state.cursor = 1;
    assert!(finalize_current_group(&mut table, &mut state));
    assert_eq!(
        table.storage.entry_kind(0),
        Some(EntryKind::live(EntryClass::Value))
    );
    assert_eq!(
        table.storage.entry_kind(1),
        Some(EntryKind::hidden(EntryClass::Value))
    );
    assert_eq!(
        table.storage.entry_kind(2),
        Some(EntryKind::hidden(EntryClass::Value))
    );

    compact(&mut table);

    assert_eq!(table.storage.len(), 1);
    assert_eq!(table.read_value::<i32>(0), &1);
}

#[test]
fn compaction_reclaims_dense_storage_after_large_hidden_range() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();
    for value in 0..4096usize {
        use_value_slot(&mut table, &mut state, move || value);
    }

    let before_compact = table.heap_bytes();

    state.cursor = 1;
    assert!(finalize_current_group(&mut table, &mut state));
    compact(&mut table);

    let after_compact = table.heap_bytes();
    assert_eq!(table.storage.len(), 1);
    assert!(
        after_compact * 8 < before_compact,
        "compaction should rebuild dense arenas: before={before_compact} after={after_compact}",
    );
}

#[test]
fn hidden_value_is_restored_without_running_initializer() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();
    use_value_slot(&mut table, &mut state, || 7i32);

    reset_session(&mut state);
    assert!(hide_range(&mut table, 0, 1, None));
    assert_eq!(
        table.storage.entry_kind(0),
        Some(EntryKind::hidden(EntryClass::Value))
    );

    let initialized = Cell::new(false);
    reset_session(&mut state);
    let index = use_value_slot(&mut table, &mut state, || {
        initialized.set(true);
        99i32
    });

    assert_eq!(index, 0);
    assert!(
        !initialized.get(),
        "hidden value reuse should not reinitialize"
    );
    assert_eq!(
        table.storage.entry_kind(0),
        Some(EntryKind::live(EntryClass::Value))
    );
    assert_eq!(table.read_value::<i32>(0), &7);
}

#[test]
fn fresh_parent_inserts_before_hidden_value_instead_of_restoring_it() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();
    use_value_slot(&mut table, &mut state, || 1i32);

    reset_session(&mut state);
    assert!(hide_range(&mut table, 0, 1, None));
    state.group_stack.push(GroupFrame {
        start: 0,
        end: table.storage.len(),
        stored_live_end: table.storage.len(),
        live_end: table.storage.len(),
        pass_boundary: PassBoundary::Fresh { boundary_key: 1 },
        previous_direct_child_group: None,
    });

    let index = use_value_slot(&mut table, &mut state, || 2i32);

    assert_eq!(index, 0);
    assert_eq!(table.storage.len(), 2);
    assert_eq!(
        table.storage.entry_kind(0),
        Some(EntryKind::live(EntryClass::Value))
    );
    assert_eq!(
        table.storage.entry_kind(1),
        Some(EntryKind::hidden(EntryClass::Value))
    );
    assert_eq!(table.read_value::<i32>(0), &2);
}

#[test]
fn hidden_group_restore_reuses_scope() {
    let runtime = TestRuntime::new();
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();
    let root_key = crate::hash_key(&"root");
    let child_key = crate::hash_key(&"child");

    let root = begin_group_for_test(&mut table, &mut state, root_key);
    let child = begin_scoped_group(&mut table, &mut state, child_key, || {
        RecomposeScope::new(runtime.handle())
    });
    let child_scope_id = child.scope.id();
    use_value_slot(&mut table, &mut state, || String::from("payload"));
    end_group(&mut table, &mut state);
    end_group(&mut table, &mut state);

    let child_extent = table.storage.group_extent_at(child.group.0);
    assert!(hide_range(
        &mut table,
        child.group.0,
        child.group.0 + child_extent,
        Some(root.0),
    ));
    assert_eq!(
        table.storage.entry_kind(child.group.0),
        Some(EntryKind::hidden(EntryClass::Group))
    );

    reset_session(&mut state);
    let _root = begin_group_for_test(&mut table, &mut state, root_key);
    let restored = begin_scoped_group(&mut table, &mut state, child_key, || {
        panic!("scope should be restored")
    });

    assert!(restored.requires_recompose);
    assert_eq!(restored.group, GroupId(child.group.0));
    assert_eq!(restored.scope.id(), child_scope_id);
    assert_eq!(
        table.storage.entry_kind(restored.group.0),
        Some(EntryKind::live(EntryClass::Group))
    );
}

#[test]
fn live_group_reuse_at_cursor_does_not_require_recompose() {
    let runtime = TestRuntime::new();
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();
    let root_key = crate::hash_key(&"root");
    let child_key = crate::hash_key(&"child");

    let _root = begin_group_for_test(&mut table, &mut state, root_key);
    let child = begin_scoped_group(&mut table, &mut state, child_key, || {
        RecomposeScope::new(runtime.handle())
    });
    end_group(&mut table, &mut state);
    end_group(&mut table, &mut state);

    reset_session(&mut state);
    let _root = begin_group_for_test(&mut table, &mut state, root_key);
    let reused = begin_scoped_group(&mut table, &mut state, child_key, || {
        panic!("scope should be reused in place")
    });

    assert!(!reused.requires_recompose);
    assert_eq!(reused.group, child.group);
    assert_eq!(reused.scope.id(), child.scope.id());
}

#[test]
fn restoring_hidden_value_clears_parent_preservation_immediately() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();

    let root = begin_group_for_test(&mut table, &mut state, 1);
    use_value_slot(&mut table, &mut state, || 7i32);
    end_group(&mut table, &mut state);

    assert!(hide_range(&mut table, 1, 2, Some(root.0)));
    assert!(table.storage.group_hidden_descendants_at(root.0) > 0);

    reset_session(&mut state);
    let _root = begin_group_for_test(&mut table, &mut state, 1);
    let restored = Cell::new(false);
    let index = use_value_slot(&mut table, &mut state, || {
        restored.set(true);
        99i32
    });
    assert_eq!(index, 1);
    end_group(&mut table, &mut state);
    assert!(
        !restored.get(),
        "hidden value should restore without reinitializing"
    );

    assert_eq!(table.storage.group_hidden_descendants_at(root.0), 0);
    verify_table(&table).expect("table should remain well-formed");
}

#[test]
fn orphaned_hidden_node_becomes_active_again_when_restored() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();
    record_node(&mut table, &mut state, 42, 7);

    reset_session(&mut state);
    assert!(hide_range(&mut table, 0, 1, None));
    let orphaned = preserved_orphaned(&table);
    assert_eq!(orphaned.len(), 1);
    let orphaned = orphaned[0];
    assert_eq!(
        table.orphaned_node_state(orphaned),
        NodeSlotState::PreservedGap
    );
    assert!(drain_orphaned(&mut table).is_empty());

    reset_session(&mut state);
    record_node(&mut table, &mut state, 42, 7);
    assert_eq!(table.orphaned_node_state(orphaned), NodeSlotState::Active);
}

#[test]
fn compaction_preserves_anchor_identity_for_shifted_slots() {
    let runtime = TestRuntime::new();
    let mut table = new_table();
    let mut compose_state = SlotWriteSessionState::default();

    let first_group = begin_scoped_group(&mut table, &mut compose_state, 1, || {
        RecomposeScope::new(runtime.handle())
    });
    let first_group_index = first_group.group.0;
    let first_group_scope_id = first_group.scope.id();
    use_value_slot(&mut table, &mut compose_state, || String::from("drop-a"));
    use_value_slot(&mut table, &mut compose_state, || String::from("drop-b"));
    end_group(&mut table, &mut compose_state);

    let second_group = begin_group_for_test(&mut table, &mut compose_state, 2);
    let survivor_index = use_value_slot(&mut table, &mut compose_state, || {
        Owned::new(String::from("survivor"))
    });
    let second_group_anchor = table.storage.entry_anchor(second_group.0);
    end_group(&mut table, &mut compose_state);

    let resolved_group_index = table
        .storage
        .resolve_anchor(second_group_anchor)
        .expect("second group anchor should resolve");
    let resolved_index = resolved_group_index + 1;
    assert_eq!(
        table
            .read_value::<Owned<String>>(resolved_index)
            .with(|text| text.clone()),
        String::from("survivor")
    );
    assert_eq!(resolved_index, survivor_index);

    let mut recompose_state = SlotWriteSessionState::default();
    let started = start_recompose_at_anchor(
        &mut table,
        &mut recompose_state,
        first_group.anchor,
        first_group_scope_id,
    )
    .expect("recompose scope should be found");
    assert_eq!(started.0, first_group_index);
    end_recompose(&mut table, &mut recompose_state);
    assert!(table.storage.needs_compact);

    compact(&mut table);

    let shifted_group_index = table
        .storage
        .resolve_anchor(second_group_anchor)
        .expect("second group anchor should still resolve");
    let shifted_index = shifted_group_index + 1;
    assert!(shifted_index < survivor_index);
    assert_eq!(
        table
            .read_value::<Owned<String>>(shifted_index)
            .with(|text| text.clone()),
        String::from("survivor")
    );
}

#[test]
fn overwrite_value_keeps_value_slots_anchorless() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();
    let index = use_value_slot(&mut table, &mut state, || 7i32);
    assert_eq!(table.storage.entry_anchor(index), AnchorId::INVALID);
    let replacement = table.storage.alloc_value(11i32);

    let dropped = table.storage.overwrite_value(index, replacement, false);
    assert!(
        dropped.is_some(),
        "overwrite should return previous payload for disposal"
    );

    let stats = table.debug_stats();
    assert_eq!(stats.anchors_len, 0);
    assert_eq!(stats.free_anchor_ids_len, 0);
    assert_eq!(table.storage.entry_anchor(index), AnchorId::INVALID);
    assert_eq!(table.read_value::<i32>(index), &11);
}

#[test]
fn hidden_descendant_scopes_are_excluded_until_restored() {
    let runtime = TestRuntime::new();
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();

    let root = begin_group_for_test(&mut table, &mut state, 1);
    let first = begin_scoped_group(&mut table, &mut state, 2, || {
        RecomposeScope::new(runtime.handle())
    });
    end_group(&mut table, &mut state);
    let second = begin_scoped_group(&mut table, &mut state, 3, || {
        RecomposeScope::new(runtime.handle())
    });
    let second_scope_id = second.scope.id();
    end_group(&mut table, &mut state);
    end_group(&mut table, &mut state);

    let second_extent = table.storage.group_extent_at(second.group.0);
    assert!(hide_range(
        &mut table,
        second.group.0,
        second.group.0 + second_extent,
        Some(root.0),
    ));

    let mut recompose_state = SlotWriteSessionState::default();
    start_recompose_at_index(&table, &mut recompose_state, root.0);
    let scopes = table.descendant_scopes_in_current_group(&recompose_state, 0);
    end_recompose(&mut table, &mut recompose_state);
    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].id(), first.scope.id());

    reset_session(&mut state);
    let _root = begin_group_for_test(&mut table, &mut state, 1);
    let _first = begin_scoped_group(&mut table, &mut state, 2, || {
        panic!("first scope should be reused")
    });
    end_group(&mut table, &mut state);
    let restored = begin_scoped_group(&mut table, &mut state, 3, || {
        panic!("second scope should be restored")
    });
    assert_eq!(restored.scope.id(), second_scope_id);
}

#[test]
fn group_anchor_resolution_tracks_sibling_reorders_without_value_anchors() {
    let cases = [
        ("swap_front_pair", [2u64, 1, 3]),
        ("move_tail_to_front", [3u64, 1, 2]),
        ("rotate_left", [2u64, 3, 1]),
    ];

    for (label, order) in cases {
        let mut table = new_table();
        let mut state = SlotWriteSessionState::default();
        let mut group_anchors = std::collections::BTreeMap::new();

        for key in [1u64, 2, 3] {
            let group = begin_group_for_test(&mut table, &mut state, key);
            let value_index = use_value_slot(&mut table, &mut state, move || key as i32);
            end_group(&mut table, &mut state);
            group_anchors.insert(key, table.storage.entry_anchor(group.0));
            assert_eq!(
                table.storage.entry_anchor(value_index),
                AnchorId::INVALID,
                "{label}: value slot for key {key} should not allocate an anchor",
            );
        }

        reset_session(&mut state);
        for key in order {
            let _group = begin_group_for_test(&mut table, &mut state, key);
            let _value = use_value_slot(&mut table, &mut state, move || key as i32);
            end_group(&mut table, &mut state);
        }

        for (position, key) in order.into_iter().enumerate() {
            let expected_group_index = position * 2;
            let expected_value_index = expected_group_index + 1;
            let group_anchor = group_anchors[&key];

            assert_eq!(
                table.storage.resolve_anchor(group_anchor),
                Some(expected_group_index),
                "{label}: group anchor for key {key} resolved incorrectly",
            );
            assert_eq!(
                table.read_value::<i32>(expected_value_index),
                &(key as i32),
                "{label}: reused value for key {key} moved incorrectly",
            );
            assert_eq!(
                table.storage.entry_anchor(expected_value_index),
                AnchorId::INVALID,
                "{label}: reordered value for key {key} unexpectedly gained an anchor",
            );
        }
    }
}

#[test]
fn compaction_is_idempotent_after_hidden_ranges_are_removed() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();

    let dropped = begin_group_for_test(&mut table, &mut state, 1);
    let _dropped_value = use_value_slot(&mut table, &mut state, || String::from("drop"));
    end_group(&mut table, &mut state);

    let kept = begin_group_for_test(&mut table, &mut state, 2);
    let kept_value_index = use_value_slot(&mut table, &mut state, || String::from("keep"));
    let kept_group_anchor = table.storage.entry_anchor(kept.0);
    assert_eq!(
        table.storage.entry_anchor(kept_value_index),
        AnchorId::INVALID
    );
    end_group(&mut table, &mut state);

    assert!(hide_range(&mut table, dropped.0, kept.0, None));

    compact(&mut table);
    let first_slots = table.debug_dump_all_slots();
    let first_groups = table.debug_dump_groups();
    let first_heap_bytes = table.heap_bytes();
    let first_group_anchor = table.storage.resolve_anchor(kept_group_anchor);
    let first_value_index = first_group_anchor.expect("kept group anchor should resolve") + 1;

    compact(&mut table);
    let second_slots = table.debug_dump_all_slots();
    let second_groups = table.debug_dump_groups();
    let second_heap_bytes = table.heap_bytes();
    let second_group_anchor = table.storage.resolve_anchor(kept_group_anchor);
    let second_value_index =
        second_group_anchor.expect("kept group anchor should still resolve") + 1;

    assert_eq!(first_slots, second_slots);
    assert_eq!(first_groups, second_groups);
    assert_eq!(first_heap_bytes, second_heap_bytes);
    assert_eq!(first_group_anchor, second_group_anchor);
    assert_eq!(first_value_index, second_value_index);
    assert_eq!(table.read_value::<String>(second_value_index), "keep");
}

#[test]
fn planner_moves_matching_live_group_only_when_parent_boundary_is_open() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();
    let _first = begin_group_for_test(&mut table, &mut state, 1);
    end_group(&mut table, &mut state);
    let _second = begin_group_for_test(&mut table, &mut state, 2);
    end_group(&mut table, &mut state);

    let cases = [
        (
            "open",
            PassBoundary::Open,
            None,
            PlannedAction::RestoreMatchingGroup {
                index: 1,
                extent: 1,
                boundary_key: 2,
                reused_hidden: false,
                retire_conflicting_group_at_cursor: true,
            },
        ),
        (
            "restored",
            PassBoundary::Restored { boundary_key: 99 },
            Some(99),
            PlannedAction::InsertFresh {
                retire_conflicting_group_at_cursor: true,
            },
        ),
        (
            "fresh",
            PassBoundary::Fresh { boundary_key: 99 },
            Some(99),
            PlannedAction::InsertFresh {
                retire_conflicting_group_at_cursor: true,
            },
        ),
    ];

    for (label, parent_boundary, current_boundary_key, expected) in cases {
        assert_eq!(
            plan_start(
                &table,
                2,
                0,
                table.storage.len(),
                parent_boundary,
                current_boundary_key,
            ),
            expected,
            "{label} parent boundary produced an unexpected live-group plan",
        );
    }
}

#[test]
fn planner_rejects_live_group_candidates_from_the_wrong_parent() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();

    let root = begin_group_for_test(&mut table, &mut state, 1);
    let _child = begin_group_for_test(&mut table, &mut state, 2);
    let nested = begin_group_for_test(&mut table, &mut state, 9);
    end_group(&mut table, &mut state);
    end_group(&mut table, &mut state);
    end_group(&mut table, &mut state);

    assert_ne!(
        table.storage.group_parent_anchor_at(nested.0),
        Some(table.storage.entry_anchor(root.0)),
        "test setup must place the candidate under a different parent",
    );

    assert_eq!(
        plan_start_with_parent_anchor(
            &table,
            9,
            nested.0,
            root.0 + table.storage.group_scan_extent_at(root.0),
            PassBoundary::Open,
            None,
            table.storage.entry_anchor(root.0),
        ),
        PlannedAction::InsertFresh {
            retire_conflicting_group_at_cursor: false,
        },
        "planner must not reuse a live descendant as a direct child candidate",
    );
}

#[test]
fn planner_rejects_preserved_live_group_when_previous_sibling_has_same_key() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();

    let root = begin_group_for_test(&mut table, &mut state, 1);
    let _first = begin_group_for_test(&mut table, &mut state, 7);
    use_value_slot(&mut table, &mut state, || 11i32);
    end_group(&mut table, &mut state);
    let second = begin_group_for_test(&mut table, &mut state, 7);
    let _second_live = use_value_slot(&mut table, &mut state, || 22i32);
    let second_hidden = use_value_slot(&mut table, &mut state, || 33i32);
    end_group(&mut table, &mut state);
    end_group(&mut table, &mut state);

    assert!(hide_range(
        &mut table,
        second_hidden,
        second_hidden + 1,
        Some(second.0),
    ));
    assert_eq!(table.storage.group_hidden_descendants_at(second.0), 1);

    assert_eq!(
        plan_start_with_parent_anchor(
            &table,
            7,
            second.0,
            root.0 + table.storage.group_scan_extent_at(root.0),
            PassBoundary::Open,
            None,
            table.storage.entry_anchor(root.0),
        ),
        PlannedAction::InsertFresh {
            retire_conflicting_group_at_cursor: false,
        },
        "planner must not reuse a preserved group occurrence when the previous sibling has the same key",
    );
}

#[test]
fn planner_hidden_group_restore_and_move_respects_parent_boundary() {
    let mut restore_only = new_table();
    let mut restore_state = SlotWriteSessionState::default();
    let hidden_only = begin_group_for_test(&mut restore_only, &mut restore_state, 7);
    end_group(&mut restore_only, &mut restore_state);
    assert!(hide_range(
        &mut restore_only,
        hidden_only.0,
        hidden_only.0 + 1,
        None,
    ));

    let restore_cases = [
        (
            "open_mismatch",
            PassBoundary::Open,
            Some(42),
            PlannedAction::RestoreHiddenAtCursor {
                extent: 1,
                boundary_key: 7,
            },
        ),
        (
            "restored_match",
            PassBoundary::Restored { boundary_key: 7 },
            Some(7),
            PlannedAction::RestoreHiddenAtCursor {
                extent: 1,
                boundary_key: 7,
            },
        ),
        (
            "fresh_match",
            PassBoundary::Fresh { boundary_key: 7 },
            Some(7),
            PlannedAction::RestoreHiddenAtCursor {
                extent: 1,
                boundary_key: 7,
            },
        ),
        (
            "restored_mismatch",
            PassBoundary::Restored { boundary_key: 9 },
            Some(9),
            PlannedAction::InsertFresh {
                retire_conflicting_group_at_cursor: false,
            },
        ),
        (
            "fresh_mismatch",
            PassBoundary::Fresh { boundary_key: 9 },
            Some(9),
            PlannedAction::InsertFresh {
                retire_conflicting_group_at_cursor: false,
            },
        ),
    ];

    for (label, parent_boundary, current_boundary_key, expected) in restore_cases {
        assert_eq!(
            plan_start(
                &restore_only,
                7,
                0,
                restore_only.storage.len(),
                parent_boundary,
                current_boundary_key,
            ),
            expected,
            "{label} produced an unexpected hidden-restore plan",
        );
    }

    let mut restore_or_move = new_table();
    let mut move_state = SlotWriteSessionState::default();
    let hidden = begin_group_for_test(&mut restore_or_move, &mut move_state, 7);
    end_group(&mut restore_or_move, &mut move_state);
    let _conflict = begin_group_for_test(&mut restore_or_move, &mut move_state, 8);
    end_group(&mut restore_or_move, &mut move_state);
    let _live_match = begin_group_for_test(&mut restore_or_move, &mut move_state, 7);
    end_group(&mut restore_or_move, &mut move_state);
    assert!(hide_range(
        &mut restore_or_move,
        hidden.0,
        hidden.0 + 1,
        None,
    ));

    for (label, parent_boundary, current_boundary_key) in [
        ("open", PassBoundary::Open, None),
        (
            "restored",
            PassBoundary::Restored { boundary_key: 7 },
            Some(7),
        ),
        ("fresh", PassBoundary::Fresh { boundary_key: 7 }, Some(7)),
    ] {
        assert_eq!(
            plan_start(
                &restore_or_move,
                7,
                0,
                restore_or_move.storage.len(),
                parent_boundary,
                current_boundary_key,
            ),
            PlannedAction::RestoreMatchingGroup {
                index: 2,
                extent: 1,
                boundary_key: 7,
                reused_hidden: false,
                retire_conflicting_group_at_cursor: false,
            },
            "{label} should move the later live group after a hidden placeholder",
        );
    }
}

#[test]
fn fresh_boundary_reuse_live_enters_restricted_restored_mode() {
    let boundary = PassBoundary::Fresh { boundary_key: 7 }
        .transition(BoundaryTransition::ReuseLive { boundary_key: 11 });
    let policy = boundary.policy();

    assert_eq!(boundary, PassBoundary::Restored { boundary_key: 11 });
    assert!(policy.allows_exact_live_reuse());
    assert!(!policy.disallows_live_value_reuse());
    assert_eq!(policy.restricted_boundary(), Some(11));
}

#[test]
fn fresh_boundary_restore_enters_restricted_restored_mode() {
    let boundary = PassBoundary::Fresh { boundary_key: 7 }
        .transition(BoundaryTransition::Restore { boundary_key: 13 });
    let policy = boundary.policy();

    assert_eq!(boundary, PassBoundary::Restored { boundary_key: 13 });
    assert!(policy.allows_exact_live_reuse());
    assert!(!policy.disallows_live_value_reuse());
    assert_eq!(policy.restricted_boundary(), Some(13));
}

#[test]
fn restart_recompose_reopens_boundary_for_child_live_search() {
    let runtime = TestRuntime::new();
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();

    let root = begin_scoped_group(&mut table, &mut state, 10, || {
        RecomposeScope::new(runtime.handle())
    });
    let _first = begin_group_for_test(&mut table, &mut state, 1);
    end_group(&mut table, &mut state);
    let _second = begin_group_for_test(&mut table, &mut state, 2);
    end_group(&mut table, &mut state);
    end_group(&mut table, &mut state);

    reset_session(&mut state);
    let started = start_recompose_at_anchor(&mut table, &mut state, root.anchor, root.scope.id());
    assert_eq!(started, Some(root.group));
    assert_eq!(
        state.group_stack.last().map(|frame| frame.pass_boundary),
        Some(PassBoundary::Open)
    );

    let _second = begin_group_for_test(&mut table, &mut state, 2);
    end_group(&mut table, &mut state);
    let _first = begin_group_for_test(&mut table, &mut state, 1);
    end_group(&mut table, &mut state);
    end_recompose(&mut table, &mut state);

    assert_eq!(table.storage.group_key_at(0), Some(10));
    assert_eq!(table.storage.group_key_at(1), Some(2));
    assert_eq!(table.storage.group_key_at(2), Some(1));
    verify_table(&table).expect("restarted recompose should keep the table well-formed");
}

#[test]
fn verifier_rejects_invalid_group_spans() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();
    let group = begin_group_for_test(&mut table, &mut state, 7);
    end_group(&mut table, &mut state);

    table
        .storage
        .set_group_spans(group.0, GroupSpans::new(3, 2));

    let err = verify_table(&table).expect_err("invalid spans should fail verification");
    assert!(err.contains("invalid spans"), "{err}");
}

#[test]
fn verifier_rejects_unresolved_parent_anchor() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();
    let group = begin_group_for_test(&mut table, &mut state, 7);
    end_group(&mut table, &mut state);

    table
        .storage
        .set_group_parent_anchor(group.0, AnchorId(999_999));

    let err = verify_table(&table).expect_err("unresolved parent anchor should fail verification");
    assert!(err.contains("unexpectedly has parent anchor"), "{err}");
}

#[test]
fn verifier_rejects_wrong_resolved_parent_anchor() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();

    let root = begin_group_for_test(&mut table, &mut state, 1);
    let first_child = begin_group_for_test(&mut table, &mut state, 2);
    end_group(&mut table, &mut state);
    let second_child = begin_group_for_test(&mut table, &mut state, 3);
    end_group(&mut table, &mut state);
    end_group(&mut table, &mut state);

    table
        .storage
        .set_group_parent_anchor(second_child.0, table.storage.entry_anchor(first_child.0));

    let err =
        verify_table(&table).expect_err("wrong resolved parent anchor should fail verification");
    assert!(err.contains("wrong parent anchor"), "{err}");
    assert_eq!(
        table.storage.group_parent_anchor_at(second_child.0),
        Some(table.storage.entry_anchor(first_child.0))
    );
    assert_eq!(
        table.storage.group_parent_anchor_at(first_child.0),
        Some(table.storage.entry_anchor(root.0))
    );
}

#[test]
fn planner_scans_preserved_tail_before_matching_the_next_sibling() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();

    let root = begin_group_for_test(&mut table, &mut state, 1);
    let first = begin_group_for_test(&mut table, &mut state, 2);
    let first_live = use_value_slot(&mut table, &mut state, || 11i32);
    let first_hidden = use_value_slot(&mut table, &mut state, || 12i32);
    end_group(&mut table, &mut state);
    let second = begin_group_for_test(&mut table, &mut state, 3);
    end_group(&mut table, &mut state);
    end_group(&mut table, &mut state);

    assert!(hide_range(
        &mut table,
        first_hidden,
        first_hidden + 1,
        Some(first.0),
    ));
    assert_eq!(table.storage.group_hidden_descendants_at(first.0), 1);
    table
        .storage
        .set_group_spans(first.0, GroupSpans::new(2, 3));

    reset_session(&mut state);
    let reused_root = begin_group_for_test(&mut table, &mut state, 1);
    assert_eq!(reused_root, root);
    let reused_first = begin_group_for_test(&mut table, &mut state, 2);
    assert_eq!(reused_first, first);
    let reused_first_value = use_value_slot(&mut table, &mut state, || 99i32);
    assert_eq!(reused_first_value, first_live);
    end_group(&mut table, &mut state);

    assert_eq!(
        state.cursor,
        first.0 + table.storage.group_scan_extent_at(first.0),
        "ending a preserved group must advance to the end of its preserved scan span",
    );

    let reused_second = begin_group_for_test(&mut table, &mut state, 3);
    assert_eq!(reused_second, second);
    end_group(&mut table, &mut state);
    end_group(&mut table, &mut state);

    verify_table(&table).expect("next sibling reuse must happen after the preserved tail");
}

#[test]
fn ending_group_caches_previous_direct_child_anchor_even_after_many_values() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();

    let _root = begin_group_for_test(&mut table, &mut state, 1);
    let child = begin_group_for_test(&mut table, &mut state, 2);
    for value in 0..50 {
        let _ = use_value_slot(&mut table, &mut state, move || value);
    }
    end_group(&mut table, &mut state);

    assert_eq!(
        state
            .group_stack
            .last()
            .and_then(|frame| frame.previous_direct_child_group),
        Some(table.storage.entry_anchor(child.0)),
        "parent frame should cache the direct child group anchor instead of rediscovering it from the child's value slots",
    );

    let _ = use_value_slot(&mut table, &mut state, || 999i32);
    assert_eq!(
        state
            .group_stack
            .last()
            .and_then(|frame| frame.previous_direct_child_group),
        None,
        "a direct scalar slot after the child group must clear the cached sibling group",
    );
}

#[test]
fn direct_child_reparent_batch_coalesces_previous_parent_updates() {
    let mut batch = super::DirectChildReparentBatch::default();
    batch.record_previous_parent(7, 2, 3);
    batch.record_previous_parent(7, 4, 5);
    batch.record_previous_parent(9, 1, 0);
    batch.add_hidden_descendants_to_new_parent(6);

    assert_eq!(batch.removed_hidden_descendants.len(), 2);
    assert_eq!(batch.removed_hidden_descendants.get(&7), Some(&6));
    assert_eq!(batch.removed_hidden_descendants.get(&9), Some(&1));
    assert_eq!(batch.removed_preserved_scan.len(), 1);
    assert_eq!(batch.removed_preserved_scan.get(&7), Some(&8));
    assert_eq!(batch.added_hidden_descendants, 6);
}

#[test]
fn planner_does_not_extract_nested_hidden_descendant_from_previous_sibling_tail() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();

    let root = begin_group_for_test(&mut table, &mut state, 1);
    let first = begin_group_for_test(&mut table, &mut state, 2);
    let nested_parent = begin_group_for_test(&mut table, &mut state, 3);
    let target = begin_group_for_test(&mut table, &mut state, 4);
    end_group(&mut table, &mut state);
    end_group(&mut table, &mut state);
    let _second = begin_group_for_test(&mut table, &mut state, 5);
    end_group(&mut table, &mut state);
    end_group(&mut table, &mut state);

    let root_anchor = table.storage.entry_anchor(root.0);
    let nested_parent_anchor = table.storage.entry_anchor(nested_parent.0);
    let target_anchor = table.storage.entry_anchor(target.0);
    let target_extent = table.storage.group_scan_extent_at(target.0);

    assert!(hide_range(
        &mut table,
        target.0,
        target.0 + target_extent,
        Some(nested_parent.0),
    ));
    table
        .storage
        .set_group_spans(nested_parent.0, GroupSpans::new(1, 2));
    table
        .storage
        .set_group_spans(first.0, GroupSpans::new(2, 3));

    reset_session(&mut state);
    assert_eq!(begin_group_for_test(&mut table, &mut state, 1), root);
    assert_eq!(begin_group_for_test(&mut table, &mut state, 2), first);
    assert_eq!(
        begin_group_for_test(&mut table, &mut state, 3),
        nested_parent
    );
    end_group(&mut table, &mut state);
    end_group(&mut table, &mut state);

    assert_eq!(
        state.cursor,
        first.0 + table.storage.group_scan_extent_at(first.0),
        "ending the parent group must advance across the preserved tail",
    );

    let restarted_target = begin_group_for_test(&mut table, &mut state, 4);
    assert_ne!(
        restarted_target, target,
        "nested hidden descendants must not be restored as direct children of the ancestor",
    );
    assert_eq!(
        table.storage.group_parent_anchor_at(restarted_target.0),
        Some(root_anchor)
    );

    let preserved_target = table
        .storage
        .resolve_anchor(target_anchor)
        .expect("original hidden target anchor should still resolve");
    assert_ne!(preserved_target, restarted_target.0);
    assert_eq!(
        table.storage.entry_kind(preserved_target),
        Some(EntryKind::hidden(EntryClass::Group))
    );
    assert_eq!(
        table.storage.group_parent_anchor_at(preserved_target),
        Some(nested_parent_anchor)
    );
}

#[test]
fn planner_restores_previous_tail_even_at_parent_end() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();

    let root = begin_group_for_test(&mut table, &mut state, 1);
    let parent = begin_group_for_test(&mut table, &mut state, 2);
    let child = begin_group_for_test(&mut table, &mut state, 3);
    end_group(&mut table, &mut state);
    end_group(&mut table, &mut state);
    end_group(&mut table, &mut state);

    let child_extent = table.storage.group_scan_extent_at(child.0).max(1);
    assert!(hide_range(
        &mut table,
        child.0,
        child.0 + child_extent,
        Some(parent.0),
    ));
    assert_eq!(
        table.storage.entry_kind(child.0),
        Some(EntryKind::hidden(EntryClass::Group))
    );

    reset_session(&mut state);
    assert_eq!(begin_group_for_test(&mut table, &mut state, 1), root);
    assert_eq!(begin_group_for_test(&mut table, &mut state, 2), parent);
    end_group(&mut table, &mut state);

    let restored_child = begin_group_for_test(&mut table, &mut state, 3);
    assert_eq!(
        restored_child, child,
        "the preserved tail child must restore even when the cursor is already at the parent end",
    );
    assert_eq!(
        table.storage.group_parent_anchor_at(restored_child.0),
        Some(table.storage.entry_anchor(root.0))
    );
    end_group(&mut table, &mut state);
    end_group(&mut table, &mut state);

    verify_table(&table).expect("table should remain well-formed after tail restoration");
}

#[test]
fn value_slots_never_register_anchors() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();
    let first = use_value_slot(&mut table, &mut state, || 1i32);
    let second = use_value_slot(&mut table, &mut state, || 2i32);

    assert_eq!(table.storage.entry_anchor(first), AnchorId::INVALID);
    assert_eq!(table.storage.entry_anchor(second), AnchorId::INVALID);
    assert_eq!(table.debug_stats().anchors_len, 0);
    verify_table(&table).expect("anchorless value slots should remain well-formed");
}

#[test]
fn verifier_rejects_scope_anchor_mismatch() {
    let runtime = TestRuntime::new();
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();
    let first = begin_scoped_group(&mut table, &mut state, 1, || {
        RecomposeScope::new(runtime.handle())
    });
    end_group(&mut table, &mut state);
    let second = begin_scoped_group(&mut table, &mut state, 2, || {
        RecomposeScope::new(runtime.handle())
    });
    end_group(&mut table, &mut state);

    table
        .storage
        .set_group_scope(first.group.0, Some(second.scope.clone()));

    let err = verify_table(&table).expect_err("scope anchor mismatch should fail verification");
    assert!(err.contains("scope anchor mismatch"), "{err}");
}

#[test]
fn verifier_rejects_ready_orphan_queue_entries() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();
    record_node(&mut table, &mut state, 17, 3);
    let orphan = super::OrphanedNode::new(17, 3, table.storage.entry_anchor(0));
    with_test_lifecycle(|lifecycle| lifecycle.queue_orphaned_node(orphan));

    let err = verify_table(&table).expect_err("ready orphan queue should fail verification");
    assert!(err.contains("orphaned node queue must be empty"), "{err}");
}

#[test]
fn verifier_rejects_stale_preserved_orphan_state() {
    let table = new_table();
    with_test_lifecycle(|lifecycle| {
        lifecycle.preserve_orphaned_node(super::OrphanedNode::new(41, 9, AnchorId(777_777)));
    });

    let err = verify_table(&table).expect_err("stale preserved orphan should fail verification");
    assert!(err.contains("preserved orphan"), "{err}");
}
