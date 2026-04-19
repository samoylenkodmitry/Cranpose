use std::cell::{Cell, RefCell};

use super::{
    begin_group_for_test, compact_for_test, drain_orphaned_node_ids_for_test, hide_range_for_test,
    reuse_planner::{ReusePlanner, StartPlan},
    storage::EntryKind,
    GroupFrame, GroupRetention, NodeSlotState, PassBoundary, SlotLifecycleCoordinator,
    SlotPassMode, SlotTable, SlotWriteSessionState,
};
use crate::{runtime::TestRuntime, GroupId, Owned, RecomposeScope, ScopeId, StartScopedGroup};

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
    with_test_lifecycle(|lifecycle| {
        table
            .write_session(lifecycle, state, SlotPassMode::Compose)
            .begin_scoped_group(key, init_scope)
    })
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

fn compact(table: &mut SlotTable) {
    with_test_lifecycle(|lifecycle| compact_for_test(table, lifecycle));
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
            extent,
            boundary_key,
        } => PlannedAction::ReuseLiveAtCursor {
            extent,
            boundary_key,
        },
        StartPlan::RestoreHiddenAtCursor { group } => PlannedAction::RestoreHiddenAtCursor {
            extent: group.extent as usize,
            boundary_key: group.retention.boundary_key(),
        },
        StartPlan::RestoreMatchingGroup {
            matched_group,
            retire_conflicting_group_at_cursor,
        } => PlannedAction::RestoreMatchingGroup {
            index: matched_group.index,
            extent: matched_group.group.extent as usize,
            boundary_key: matched_group.group.retention.boundary_key(),
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
    describe_plan(
        ReusePlanner::new(
            &table.storage,
            key,
            cursor,
            parent_end,
            parent_boundary,
            current_parent_boundary_key,
        )
        .plan(),
    )
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
    assert_eq!(table.storage.entry_kind(0), Some(EntryKind::Value));
    assert_eq!(table.storage.entry_kind(1), Some(EntryKind::HiddenValue));
    assert_eq!(table.storage.entry_kind(2), Some(EntryKind::HiddenValue));

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
    assert_eq!(table.storage.entry_kind(0), Some(EntryKind::HiddenValue));

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
    assert_eq!(table.storage.entry_kind(0), Some(EntryKind::Value));
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
        pass_boundary: PassBoundary::Fresh { boundary_key: 1 },
    });

    let index = use_value_slot(&mut table, &mut state, || 2i32);

    assert_eq!(index, 0);
    assert_eq!(table.storage.len(), 2);
    assert_eq!(table.storage.entry_kind(0), Some(EntryKind::Value));
    assert_eq!(table.storage.entry_kind(1), Some(EntryKind::HiddenValue));
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
        Some(EntryKind::HiddenGroup)
    );

    reset_session(&mut state);
    let _root = begin_group_for_test(&mut table, &mut state, root_key);
    let restored = begin_scoped_group(&mut table, &mut state, child_key, || {
        panic!("scope should be restored")
    });

    assert!(restored.restored_from_gap);
    assert_eq!(restored.group, GroupId(child.group.0));
    assert_eq!(restored.scope.id(), child_scope_id);
    assert_eq!(
        table.storage.entry_kind(restored.group.0),
        Some(EntryKind::Group)
    );
}

#[test]
fn orphaned_hidden_node_becomes_active_again_when_restored() {
    let mut table = new_table();
    let mut state = SlotWriteSessionState::default();
    record_node(&mut table, &mut state, 42, 7);

    reset_session(&mut state);
    assert!(hide_range(&mut table, 0, 1, None));
    let orphaned = drain_orphaned(&mut table);
    assert_eq!(orphaned.len(), 1);
    let orphaned = orphaned[0];
    assert_eq!(
        table.orphaned_node_state(orphaned),
        NodeSlotState::PreservedGap
    );

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

    let _second_group = begin_group_for_test(&mut table, &mut compose_state, 2);
    let survivor_index = use_value_slot(&mut table, &mut compose_state, || {
        Owned::new(String::from("survivor"))
    });
    let survivor_anchor = table.storage.entry_anchor(survivor_index);
    end_group(&mut table, &mut compose_state);
    table.flush();

    let resolved_index = table
        .storage
        .resolve_anchor(survivor_anchor)
        .expect("survivor anchor should resolve");
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

    let shifted_index = table
        .storage
        .resolve_anchor(survivor_anchor)
        .expect("survivor anchor should still resolve");
    assert!(shifted_index < survivor_index);
    assert_eq!(
        table
            .read_value::<Owned<String>>(shifted_index)
            .with(|text| text.clone()),
        String::from("survivor")
    );
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
fn anchor_resolution_tracks_group_and_value_anchors_across_sibling_reorders() {
    let cases = [
        ("swap_front_pair", [2u64, 1, 3]),
        ("move_tail_to_front", [3u64, 1, 2]),
        ("rotate_left", [2u64, 3, 1]),
    ];

    for (label, order) in cases {
        let mut table = new_table();
        let mut state = SlotWriteSessionState::default();
        let mut group_anchors = std::collections::BTreeMap::new();
        let mut value_anchors = std::collections::BTreeMap::new();

        for key in [1u64, 2, 3] {
            let group = begin_group_for_test(&mut table, &mut state, key);
            let value_index = use_value_slot(&mut table, &mut state, move || key as i32);
            end_group(&mut table, &mut state);
            group_anchors.insert(key, table.storage.entry_anchor(group.0));
            value_anchors.insert(key, table.storage.entry_anchor(value_index));
        }
        table.flush();

        reset_session(&mut state);
        for key in order {
            let _group = begin_group_for_test(&mut table, &mut state, key);
            let _value = use_value_slot(&mut table, &mut state, move || key as i32);
            end_group(&mut table, &mut state);
        }
        table.flush();

        for (position, key) in order.into_iter().enumerate() {
            let expected_group_index = position * 2;
            let expected_value_index = expected_group_index + 1;
            let group_anchor = group_anchors[&key];
            let value_anchor = value_anchors[&key];

            assert_eq!(
                table.storage.resolve_anchor(group_anchor),
                Some(expected_group_index),
                "{label}: group anchor for key {key} resolved incorrectly",
            );
            assert_eq!(
                table.storage.resolve_anchor(value_anchor),
                Some(expected_value_index),
                "{label}: value anchor for key {key} resolved incorrectly",
            );
            assert_eq!(
                table.read_value::<i32>(expected_value_index),
                &(key as i32),
                "{label}: reused value for key {key} moved incorrectly",
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
    let kept_value_anchor = table.storage.entry_anchor(kept_value_index);
    end_group(&mut table, &mut state);

    assert!(hide_range(&mut table, dropped.0, kept.0, None));

    compact(&mut table);
    let first_slots = table.debug_dump_all_slots();
    let first_groups = table.debug_dump_groups();
    let first_heap_bytes = table.heap_bytes();
    let first_group_anchor = table.storage.resolve_anchor(kept_group_anchor);
    let first_value_anchor = table.storage.resolve_anchor(kept_value_anchor);

    compact(&mut table);
    let second_slots = table.debug_dump_all_slots();
    let second_groups = table.debug_dump_groups();
    let second_heap_bytes = table.heap_bytes();
    let second_group_anchor = table.storage.resolve_anchor(kept_group_anchor);
    let second_value_anchor = table.storage.resolve_anchor(kept_value_anchor);

    assert_eq!(first_slots, second_slots);
    assert_eq!(first_groups, second_groups);
    assert_eq!(first_heap_bytes, second_heap_bytes);
    assert_eq!(first_group_anchor, second_group_anchor);
    assert_eq!(first_value_anchor, second_value_anchor);
    assert_eq!(
        table.read_value::<String>(second_value_anchor.unwrap()),
        "keep"
    );
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
    restore_only
        .storage
        .set_group_retention(hidden_only.0, GroupRetention::preserved(7));

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
    restore_or_move
        .storage
        .set_group_retention(hidden.0, GroupRetention::preserved(7));

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
