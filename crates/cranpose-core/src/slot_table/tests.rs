use std::cell::Cell;

use super::{storage::EntryKind, GroupFrame, NodeSlotState, ReuseState, SlotTable};
use crate::{runtime::TestRuntime, GroupId, Owned, RecomposeScope};

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
    let mut table = SlotTable::new();
    table.use_value_slot(|| 1i32);
    table.use_value_slot(|| 2i32);
    table.use_value_slot(|| 3i32);

    table.cursor = 1;
    assert!(table.trim_to_cursor());
    assert_eq!(table.storage.entry_kind(0), Some(EntryKind::Value));
    assert_eq!(table.storage.entry_kind(1), Some(EntryKind::HiddenValue));
    assert_eq!(table.storage.entry_kind(2), Some(EntryKind::HiddenValue));

    table.compact();

    assert_eq!(table.storage.len(), 1);
    assert_eq!(table.read_value::<i32>(0), &1);
}

#[test]
fn compaction_reclaims_dense_storage_after_large_hidden_range() {
    let mut table = SlotTable::new();
    for value in 0..4096usize {
        table.use_value_slot(move || value);
    }

    let before_compact = table.heap_bytes();

    table.cursor = 1;
    assert!(table.trim_to_cursor());
    table.compact();

    let after_compact = table.heap_bytes();
    assert_eq!(table.storage.len(), 1);
    assert!(
        after_compact * 8 < before_compact,
        "compaction should rebuild dense arenas: before={before_compact} after={after_compact}",
    );
}

#[test]
fn hidden_value_is_restored_without_running_initializer() {
    let mut table = SlotTable::new();
    table.use_value_slot(|| 7i32);

    table.reset();
    assert!(table.mark_range_as_gaps(0, 1, None));
    assert_eq!(table.storage.entry_kind(0), Some(EntryKind::HiddenValue));

    let initialized = Cell::new(false);
    table.reset();
    let index = table.use_value_slot(|| {
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
    let mut table = SlotTable::new();
    table.use_value_slot(|| 1i32);

    table.reset();
    assert!(table.mark_range_as_gaps(0, 1, None));
    table.group_stack.push(GroupFrame {
        start: 0,
        end: table.storage.len(),
        reuse_state: ReuseState::Fresh { boundary_key: 1 },
    });

    let index = table.use_value_slot(|| 2i32);

    assert_eq!(index, 0);
    assert_eq!(table.storage.len(), 2);
    assert_eq!(table.storage.entry_kind(0), Some(EntryKind::Value));
    assert_eq!(table.storage.entry_kind(1), Some(EntryKind::HiddenValue));
    assert_eq!(table.read_value::<i32>(0), &2);
}

#[test]
fn hidden_group_restore_reuses_scope() {
    let runtime = TestRuntime::new();
    let mut table = SlotTable::new();
    let root_key = crate::hash_key(&"root");
    let child_key = crate::hash_key(&"child");

    let root = table.begin_group(root_key);
    let child = table.begin_scoped_group(child_key, || RecomposeScope::new(runtime.handle()));
    let child_scope_id = child.scope.id();
    table.use_value_slot(|| String::from("payload"));
    table.end_group();
    table.end_group();

    let child_len = table.storage.group_len_at(child.group.0);
    assert!(table.mark_range_as_gaps(child.group.0, child.group.0 + child_len, Some(root.group.0),));
    assert_eq!(
        table.storage.entry_kind(child.group.0),
        Some(EntryKind::HiddenGroup)
    );

    table.reset();
    let _root = table.begin_group(root_key);
    let restored = table.begin_scoped_group(child_key, || panic!("scope should be restored"));

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
    let mut table = SlotTable::new();
    table.record_node(42, 7);

    table.reset();
    assert!(table.mark_range_as_gaps(0, 1, None));
    let orphaned = table.drain_orphaned_node_ids();
    assert_eq!(orphaned.len(), 1);
    let orphaned = orphaned[0];
    assert_eq!(
        table.orphaned_node_state(orphaned),
        NodeSlotState::PreservedGap
    );

    table.reset();
    table.record_node(42, 7);
    assert_eq!(table.orphaned_node_state(orphaned), NodeSlotState::Active);
}

#[test]
fn compaction_preserves_anchor_identity_for_shifted_slots() {
    let runtime = TestRuntime::new();
    let mut table = SlotTable::new();

    let first_group = table.begin_scoped_group(1, || RecomposeScope::new(runtime.handle()));
    let first_group_index = first_group.group.0;
    let first_group_scope_id = first_group.scope.id();
    table.use_value_slot(|| String::from("drop-a"));
    table.use_value_slot(|| String::from("drop-b"));
    table.end_group();

    let _second_group = table.start(2);
    let (survivor_index, survivor_anchor) = table.remember_with_anchor(|| String::from("survivor"));
    table.end_group();
    table.flush();

    assert_eq!(
        table
            .read_value_by_anchor::<Owned<String>>(survivor_anchor)
            .map(|value| value.with(|text| text.clone())),
        Some(String::from("survivor"))
    );
    assert_eq!(table.resolve_anchor(survivor_anchor), Some(survivor_index));

    let started = table
        .start_recranpose_at_anchor(first_group.anchor, first_group_scope_id)
        .expect("recompose scope should be found");
    assert_eq!(started.0, first_group_index);
    table.end_recompose();
    assert!(table.storage.needs_compact);

    table.compact();

    let shifted_index = table
        .resolve_anchor(survivor_anchor)
        .expect("survivor anchor should still resolve");
    assert!(shifted_index < survivor_index);
    assert_eq!(
        table
            .read_value_by_anchor::<Owned<String>>(survivor_anchor)
            .map(|value| value.with(|text| text.clone())),
        Some(String::from("survivor"))
    );
}

#[test]
fn hidden_descendant_scopes_are_excluded_until_restored() {
    let runtime = TestRuntime::new();
    let mut table = SlotTable::new();

    let root = table.begin_group(1);
    let first = table.begin_scoped_group(2, || RecomposeScope::new(runtime.handle()));
    table.end_group();
    let second = table.begin_scoped_group(3, || RecomposeScope::new(runtime.handle()));
    let second_scope_id = second.scope.id();
    table.end_group();
    table.end_group();

    let second_len = table.storage.group_len_at(second.group.0);
    assert!(table.mark_range_as_gaps(
        second.group.0,
        second.group.0 + second_len,
        Some(root.group.0),
    ));

    table.start_recompose(root.group.0);
    let scopes = table.descendant_scopes_in_current_group(0);
    table.end_recompose();
    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].id(), first.scope.id());

    table.reset();
    let _root = table.begin_group(1);
    let _first = table.begin_scoped_group(2, || panic!("first scope should be reused"));
    table.end_group();
    let restored = table.begin_scoped_group(3, || panic!("second scope should be restored"));
    assert_eq!(restored.scope.id(), second_scope_id);
}
