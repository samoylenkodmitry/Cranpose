use super::{
    AnchorState, DetachedSubtree, SlotInvariantError, SlotLifecycleCoordinator, SlotPassMode,
    SlotTable, SlotWriteSession, SlotWriteSessionState,
};
use crate::{
    retention::{RetainKey, RetentionManager},
    slot_storage::{GroupId, GroupKey, GroupKeySeed, GroupStart, GroupStartKind, SlotStorage},
    AnchorId, Applier, BeginGroupInput, Key, MemoryApplier, Node, NodeId, ScopeId,
};
use std::any::TypeId;
use std::cell::Cell;
use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;

struct SlotHarness {
    table: SlotTable,
    lifecycle: SlotLifecycleCoordinator,
    state: SlotWriteSessionState,
    applier: MemoryApplier,
}

struct UnmountTrackingNode {
    unmounts: Rc<Cell<usize>>,
}

impl UnmountTrackingNode {
    fn new(unmounts: Rc<Cell<usize>>) -> Self {
        Self { unmounts }
    }
}

impl Node for UnmountTrackingNode {
    fn unmount(&mut self) {
        self.unmounts.set(self.unmounts.get() + 1);
    }
}

impl SlotHarness {
    fn new() -> Self {
        Self {
            table: SlotTable::new(),
            lifecycle: SlotLifecycleCoordinator::default(),
            state: SlotWriteSessionState::default(),
            applier: MemoryApplier::new(),
        }
    }

    fn begin_pass(&mut self, mode: SlotPassMode) {
        self.state.reset_for_pass(&self.table, mode);
    }

    fn session<R>(
        &mut self,
        mode: SlotPassMode,
        f: impl FnOnce(&mut SlotWriteSession<'_>) -> R,
    ) -> R {
        let mut session = self
            .table
            .write_session(&mut self.lifecycle, &mut self.state, mode);
        let result = f(&mut session);
        self.state
            .validate(&self.table)
            .expect("slot writer state must stay within active table bounds");
        result
    }

    fn finish_pass(&mut self, mode: SlotPassMode) {
        let detached_root_children = {
            let mut session = self
                .table
                .write_session(&mut self.lifecycle, &mut self.state, mode);
            session.finalize_pass(&mut self.applier)
        };
        for subtree in detached_root_children {
            self.table.invalidate_detached_subtree_anchors(&subtree);
            self.lifecycle.queue_subtree_disposal(subtree);
        }
        self.lifecycle.flush_pending_drops();
        self.table.debug_verify(Some(&self.lifecycle));
    }
}

fn begin_unkeyed(
    session: &mut SlotWriteSession<'_>,
    key: Key,
    restored: Option<DetachedSubtree>,
) -> GroupStart<GroupId> {
    let group_key = session.preview_group_key(GroupKeySeed::unkeyed(key));
    session.begin_group(BeginGroupInput::new(group_key, restored))
}

fn begin_keyed(
    session: &mut SlotWriteSession<'_>,
    static_key: Key,
    explicit_key: Key,
    restored: Option<DetachedSubtree>,
) -> GroupStart<GroupId> {
    let group_key = session.preview_group_key(GroupKeySeed::keyed(static_key, explicit_key));
    session.begin_group(BeginGroupInput::new(group_key, restored))
}

fn composed_parent_child_table(
    parent_key: Key,
    child_key: Key,
    child_scope: Option<ScopeId>,
) -> SlotTable {
    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, parent_key, None);

        let child = begin_unkeyed(session, child_key, None);
        if let Some(scope_id) = child_scope {
            session.set_group_scope(child.group, scope_id);
        }
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass(SlotPassMode::Compose);
    harness.table
}

fn detached_single_child(parent_key: Key, child_key: Key) -> (SlotHarness, DetachedSubtree) {
    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, parent_key, None);
        begin_unkeyed(session, child_key, None);
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();
        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass(SlotPassMode::Compose);

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, parent_key, None);
        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass(SlotPassMode::Compose);
    (harness, detached)
}

fn composed_group_with_value_and_node_table(group_key: Key) -> SlotTable {
    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, group_key, None);
        let _ = session.value_slot(|| 17_i32);
        session.record_node(31, 1);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass(SlotPassMode::Compose);
    harness.table
}

fn detached_group_payloads(
    subtree: &DetachedSubtree,
    group_index: usize,
) -> &[super::PayloadRecord] {
    let group = &subtree.groups[group_index];
    let start = group.payload_start as usize;
    let end = start + group.payload_len as usize;
    &subtree.payloads[start..end]
}

fn detached_group_nodes(subtree: &DetachedSubtree, group_index: usize) -> &[super::NodeRecord] {
    let group = &subtree.groups[group_index];
    let start = group.node_start as usize;
    let end = start + group.node_len as usize;
    &subtree.nodes[start..end]
}

fn exercise_slot_storage_trait_surface<S: SlotStorage<Group = GroupId>>(
    slots: &mut S,
    group_key: GroupKey,
    scope_id: ScopeId,
) -> GroupId {
    let started = SlotStorage::begin_group(slots, BeginGroupInput::new(group_key, None));
    assert_eq!(started.kind, GroupStartKind::Inserted);
    SlotStorage::set_group_scope(slots, started.group, scope_id);

    let slot = SlotStorage::value_slot(slots, || 7_i32);
    assert_eq!(*SlotStorage::read_value::<i32>(slots, slot), 7);
    *SlotStorage::read_value_mut::<i32>(slots, slot) = 8;
    SlotStorage::write_value(slots, slot, 9_i32);
    assert_eq!(*SlotStorage::read_value::<i32>(slots, slot), 9);

    let recorded = SlotStorage::record_node(slots, 55, 1);
    assert!(!recorded.reused);
    assert_eq!(recorded.id, 55);
    assert_eq!(SlotStorage::nodes_in_current_group(slots), vec![55]);

    let result = SlotStorage::finish_group_body(slots);
    assert!(result.detached_children.is_empty());
    SlotStorage::end_group(slots);
    assert!(SlotStorage::validate(slots).is_ok());
    assert_eq!(SlotStorage::debug_snapshot(slots).active_groups.len(), 1);
    started.group
}

fn next_bool(seed: &mut u64) -> bool {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    (*seed & 1) == 0
}

fn shuffle<T>(values: &mut [T], seed: &mut u64) {
    for index in (1..values.len()).rev() {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        let swap_with = (*seed as usize) % (index + 1);
        values.swap(index, swap_with);
    }
}

#[test]
fn slot_v2_empty_table_validates() {
    let table = SlotTable::new();
    assert_eq!(table.validate(), Ok(()));
    assert!(table.groups.is_empty());
    assert!(table.debug_dump_groups().is_empty());
    assert!(table.debug_dump_slot_entries().is_empty());
}

#[test]
fn first_composition_records_group_value_and_node() {
    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    let slot = harness.session(SlotPassMode::Compose, |session| {
        let started = begin_unkeyed(session, 1, None);
        assert_eq!(started.kind, GroupStartKind::Inserted);
        let slot = session.value_slot(|| 41_i32);
        session.record_node(7, 1);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        assert!(result.direct_nodes.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass(SlotPassMode::Compose);

    assert_eq!(*harness.table.read_value::<i32>(slot), 41);
    assert_eq!(harness.table.groups.len(), 1);
    assert_eq!(harness.table.total_payload_count(), 1);
    assert_eq!(harness.table.groups[0].payload_len, 1);
    assert_eq!(harness.table.total_node_count(), 1);
    assert_eq!(harness.table.groups[0].node_len, 1);
    assert_eq!(harness.table.groups[0].subtree_len, 1);
    assert_eq!(harness.table.groups[0].subtree_node_count, 1);
    let payload = harness.table.group_payload_record_at(0, 0);
    assert_eq!(payload.type_id, TypeId::of::<i32>());
    assert_eq!(
        harness.table.group_node_record_at(0, 0).lifecycle,
        super::NodeLifecycle::Active
    );
}

#[test]
fn slot_storage_trait_exposes_semantic_session_surface() {
    const GROUP_KEY: Key = 77;
    const SCOPE_ID: ScopeId = 901;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    let composed_group = harness.session(SlotPassMode::Compose, |session| {
        exercise_slot_storage_trait_surface(session, GroupKey::new(GROUP_KEY, None, 0), SCOPE_ID)
    });
    harness.finish_pass(SlotPassMode::Compose);

    harness.begin_pass(SlotPassMode::Recompose);
    harness.session(SlotPassMode::Recompose, |session| {
        let recomposed_group = SlotStorage::begin_recompose_at_scope(session, SCOPE_ID)
            .expect("trait should resolve the indexed scope");
        assert_eq!(recomposed_group, composed_group);
        assert_eq!(SlotStorage::nodes_in_current_group(session), vec![55]);
        SlotStorage::skip_group(session);
        let result = SlotStorage::finish_group_body(session);
        assert!(result.detached_children.is_empty());
        assert_eq!(result.subtree_nodes, vec![55]);
        assert_eq!(result.root_nodes, vec![55]);
        assert!(result.was_skipped);
        SlotStorage::end_recompose(session);
        assert!(SlotStorage::validate(session).is_ok());
    });
    harness.finish_pass(SlotPassMode::Recompose);
}

#[test]
fn debug_snapshot_reports_active_groups_anchors_and_scopes() {
    const ROOT_KEY: Key = 41;
    const CHILD_KEY: Key = 42;
    const ROOT_SCOPE: ScopeId = 101;
    const CHILD_SCOPE: ScopeId = 202;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(SlotPassMode::Compose, |session| {
        let root = begin_unkeyed(session, ROOT_KEY, None);
        session.set_group_scope(root.group, ROOT_SCOPE);
        let _ = session.value_slot(|| 7_i32);
        session.record_node(11, 1);

        let child = begin_unkeyed(session, CHILD_KEY, None);
        session.set_group_scope(child.group, CHILD_SCOPE);
        let _ = session.value_slot(|| 9_i32);
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let root_result = session.finish_group_body();
        assert!(root_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass(SlotPassMode::Compose);

    let snapshot = harness.table.debug_snapshot();
    assert_eq!(snapshot.active_groups.len(), 2);
    assert_eq!(snapshot.anchors.len(), 2);
    assert_eq!(snapshot.scopes.len(), 2);
    assert_eq!(snapshot.active_payload_count, 2);
    assert_eq!(snapshot.active_node_count, 1);
    assert_eq!(snapshot.active_scope_count, 2);
    assert_eq!(snapshot.scope_registry_count, 2);
    assert_eq!(snapshot.retained_subtree_count, 0);

    let root = snapshot
        .active_groups
        .iter()
        .find(|group| group.static_key == ROOT_KEY)
        .expect("root group snapshot");
    assert_eq!(root.scope_id, Some(ROOT_SCOPE));
    assert_eq!(root.subtree_len, 2);
    assert_eq!(root.payload_len, 1);
    assert_eq!(root.node_len, 1);

    let child = snapshot
        .active_groups
        .iter()
        .find(|group| group.static_key == CHILD_KEY)
        .expect("child group snapshot");
    assert_eq!(child.parent_anchor, root.anchor);
    assert_eq!(child.scope_id, Some(CHILD_SCOPE));
    assert_eq!(child.subtree_len, 1);
    assert_eq!(child.payload_len, 1);
}

#[test]
fn debug_dump_slot_entries_use_v2_native_labels() {
    const GROUP_KEY: Key = 51;

    let table = composed_group_with_value_and_node_table(GROUP_KEY);
    let rows = table.debug_dump_slot_entries();

    assert!(
        rows.iter()
            .any(|entry| entry.kind == super::SlotDebugEntryKind::Payload),
        "payload entries should still be visible in debug output"
    );
    assert!(
        rows.iter()
            .any(|entry| entry.line.contains("kind=internal")),
        "debug output must expose the stored payload classification"
    );
    assert!(
        rows.iter().all(|entry| {
            !entry.line.starts_with("Value(") && !entry.line.starts_with("PayloadKind(")
        }),
        "debug output must speak in V2 entry labels rather than fake slot-stream wrappers"
    );
}

#[test]
fn debug_stats_report_explicit_v2_table_counts() {
    const GROUP_KEY: Key = 34;

    let table = composed_group_with_value_and_node_table(GROUP_KEY);
    let stats = table.debug_stats();

    assert_eq!(stats.group_count, 1);
    assert_eq!(stats.payload_count, 1);
    assert_eq!(stats.payload_location_count, 1);
    assert_eq!(stats.node_count, 1);
    assert_eq!(stats.active_anchor_count, 1);
    assert_eq!(stats.anchor_slot_count, 1);
    assert_eq!(stats.anchor_sparse_count, 0);
    assert_eq!(stats.detached_anchor_count, 0);
    assert_eq!(stats.invalidated_anchor_count, 0);
    assert_eq!(stats.free_anchor_count, 0);
    assert_eq!(stats.scope_index_count, 0);
    assert_eq!(stats.pending_drop_count, 0);
    assert!(stats.group_capacity >= stats.group_count);
    assert!(stats.payload_capacity >= stats.payload_count);
    assert!(stats.payload_location_capacity >= stats.payload_location_count);
    assert!(stats.node_capacity >= stats.node_count);
    assert!(stats.anchor_capacity >= stats.active_anchor_count);
    assert!(stats.anchor_heap_bytes > 0);
    assert!(stats.scope_index_capacity >= stats.scope_index_count);
    assert!(stats.pending_drop_capacity >= stats.pending_drop_count);
    assert_eq!(stats.retained_subtree_count, 0);
    assert_eq!(stats.retained_group_count, 0);
    assert_eq!(stats.retained_payload_count, 0);
    assert_eq!(stats.retained_node_count, 0);
    assert_eq!(stats.retained_scope_count, 0);
    assert_eq!(stats.retained_anchor_count, 0);
    assert_eq!(stats.retained_heap_bytes, 0);
}

#[test]
fn payload_records_store_semantic_payload_kinds() {
    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, 52, None);
        let _remembered = session.remember(|| 17_i32);
        let _param_slot = session
            .value_slot_with_kind(super::PayloadKind::Param, crate::ParamState::<i32>::default);
        let _callback_slot =
            session.value_slot_with_kind(super::PayloadKind::Param, crate::CallbackHolder::new);
        let _return_slot = session.value_slot_with_kind(
            super::PayloadKind::Return,
            crate::ReturnSlot::<i32>::default,
        );
        let _effect_slot = session.remember_with_kind(
            super::PayloadKind::Effect,
            crate::DisposableEffectState::default,
        );
        let _type_named_internal_slot = session.value_slot(crate::ReturnSlot::<i32>::default);
        let _internal_slot = session.value_slot(|| 99_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass(SlotPassMode::Compose);

    let payload_kinds = harness
        .table
        .group_payload_records_at(0)
        .iter()
        .map(|payload| payload.kind)
        .collect::<Vec<_>>();

    assert_eq!(
        payload_kinds,
        vec![
            super::PayloadKind::Remember,
            super::PayloadKind::Param,
            super::PayloadKind::Param,
            super::PayloadKind::Return,
            super::PayloadKind::Effect,
            super::PayloadKind::Internal,
            super::PayloadKind::Internal,
        ]
    );
}

#[test]
fn second_identical_composition_reuses_group_and_value() {
    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let first_slot = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, 11, None);
        let slot = session.value_slot(|| 10_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass(SlotPassMode::Compose);
    harness.table.write_value(first_slot, 99_i32);

    harness.begin_pass(SlotPassMode::Compose);
    let (kind, second_slot) = harness.session(SlotPassMode::Compose, |session| {
        let started = begin_unkeyed(session, 11, None);
        let slot = session.value_slot(|| 0_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        (started.kind, slot)
    });
    harness.finish_pass(SlotPassMode::Compose);

    assert_eq!(kind, GroupStartKind::Reused);
    assert_eq!(second_slot, first_slot);
    assert_eq!(*harness.table.read_value::<i32>(second_slot), 99);
}

#[test]
fn read_value_mut_updates_existing_slot_in_place() {
    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let slot = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, 12, None);
        let slot = session.value_slot(|| 5_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass(SlotPassMode::Compose);

    *harness.table.read_value_mut::<i32>(slot) = 77;

    assert_eq!(*harness.table.read_value::<i32>(slot), 77);
}

#[test]
fn read_value_type_mismatch_panics_consistently() {
    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let slot = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, 15, None);
        let slot = session.value_slot(|| 5_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass(SlotPassMode::Compose);

    let mismatch_read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = harness.table.read_value::<u32>(slot);
    }));
    assert!(
        mismatch_read.is_err(),
        "typed value-slot reads must fail on a type mismatch"
    );
}

#[test]
fn disposed_value_slot_handle_does_not_alias_new_slot() {
    const OLD_KEY: Key = 13;
    const NEW_KEY: Key = 14;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let old_slot = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, OLD_KEY, None);
        let slot = session.value_slot(|| 5_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass(SlotPassMode::Compose);

    harness.begin_pass(SlotPassMode::Compose);
    harness.finish_pass(SlotPassMode::Compose);
    assert!(harness.table.groups.is_empty());

    harness.begin_pass(SlotPassMode::Compose);
    let new_slot = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, NEW_KEY, None);
        let slot = session.value_slot(|| 77_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass(SlotPassMode::Compose);

    assert_eq!(*harness.table.read_value::<i32>(new_slot), 77);

    let stale_read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = harness.table.read_value::<i32>(old_slot);
    }));
    assert!(
        stale_read.is_err(),
        "disposed value slot handles must fail cleanly instead of aliasing a new slot"
    );
}

#[test]
fn detach_restore_preserves_nested_payloads_and_scopes() {
    const PARENT_KEY: Key = 100;
    const CHILD_KEY: Key = 200;
    const GRANDCHILD_KEY: Key = 300;
    const CHILD_SCOPE: ScopeId = 7;
    const GRANDCHILD_SCOPE: ScopeId = 8;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (child_slot, grandchild_slot, child_anchor) =
        harness.session(SlotPassMode::Compose, |session| {
            begin_unkeyed(session, PARENT_KEY, None);

            let child = begin_unkeyed(session, CHILD_KEY, None);
            session.set_group_scope(child.group, CHILD_SCOPE);
            let child_slot = session.value_slot(|| 70_i32);
            session.record_node(21, 1);

            let grandchild = begin_unkeyed(session, GRANDCHILD_KEY, None);
            session.set_group_scope(grandchild.group, GRANDCHILD_SCOPE);
            let grandchild_slot = session.value_slot(|| 110_i32);
            session.record_node(22, 1);
            let grandchild_result = session.finish_group_body();
            assert!(grandchild_result.detached_children.is_empty());
            session.end_group();

            let child_result = session.finish_group_body();
            assert!(child_result.detached_children.is_empty());
            session.end_group();

            let parent_result = session.finish_group_body();
            assert!(parent_result.detached_children.is_empty());
            session.end_group();

            (child_slot, grandchild_slot, child.anchor)
        });
    harness.finish_pass(SlotPassMode::Compose);
    harness.table.write_value(child_slot, 71_i32);
    harness.table.write_value(grandchild_slot, 111_i32);

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);
        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass(SlotPassMode::Compose);
    assert!(
        !harness.table.anchors.contains_active(child_anchor),
        "detached child must leave the active anchor index"
    );
    assert_eq!(
        harness.table.anchors.state(child_anchor),
        Some(AnchorState::Detached),
        "detached child must remain addressable as detached until restored or disposed"
    );
    assert_eq!(detached.groups[0].parent_anchor, AnchorId::INVALID);
    assert_eq!(detached.groups[0].depth, 0);
    assert_eq!(detached.groups[1].parent_anchor, detached.groups[0].anchor);
    assert_eq!(detached.groups[1].depth, 1);

    harness.begin_pass(SlotPassMode::Compose);
    let (
        child_kind,
        restored_child_scope,
        restored_child_slot,
        grandchild_kind,
        restored_grandchild_scope,
        restored_grandchild_slot,
    ) = harness.session(SlotPassMode::Compose, move |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let child = begin_unkeyed(session, CHILD_KEY, Some(detached));
        let child_slot = session.value_slot(|| 0_i32);

        let grandchild = begin_unkeyed(session, GRANDCHILD_KEY, None);
        let grandchild_slot = session.value_slot(|| 0_i32);
        let grandchild_result = session.finish_group_body();
        assert!(grandchild_result.detached_children.is_empty());
        session.end_group();

        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        (
            child.kind,
            child.scope_id,
            child_slot,
            grandchild.kind,
            grandchild.scope_id,
            grandchild_slot,
        )
    });
    harness.finish_pass(SlotPassMode::Compose);

    assert_eq!(child_kind, GroupStartKind::Restored);
    assert_eq!(restored_child_scope, Some(CHILD_SCOPE));
    assert_eq!(grandchild_kind, GroupStartKind::Reused);
    assert_eq!(restored_grandchild_scope, Some(GRANDCHILD_SCOPE));
    assert_eq!(*harness.table.read_value::<i32>(restored_child_slot), 71);
    assert_eq!(
        *harness.table.read_value::<i32>(restored_grandchild_slot),
        111
    );
}

#[test]
fn restore_subtree_between_existing_siblings_reactivates_scope_and_anchor_indexes() {
    const PARENT_KEY: Key = 320;
    const CHILD_A_KEY: Key = 321;
    const CHILD_B_KEY: Key = 322;
    const CHILD_C_KEY: Key = 323;
    const CHILD_B_SCOPE: ScopeId = 324;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (parent_anchor, child_a_anchor, child_b_anchor, child_c_anchor, child_b_slot) = harness
        .session(SlotPassMode::Compose, |session| {
            let parent = begin_unkeyed(session, PARENT_KEY, None);

            let child_a = begin_unkeyed(session, CHILD_A_KEY, None);
            let child_a_result = session.finish_group_body();
            assert!(child_a_result.detached_children.is_empty());
            session.end_group();

            let child_b = begin_unkeyed(session, CHILD_B_KEY, None);
            session.set_group_scope(child_b.group, CHILD_B_SCOPE);
            let child_b_slot = session.value_slot(|| 80_i32);
            let child_b_result = session.finish_group_body();
            assert!(child_b_result.detached_children.is_empty());
            session.end_group();

            let child_c = begin_unkeyed(session, CHILD_C_KEY, None);
            let child_c_result = session.finish_group_body();
            assert!(child_c_result.detached_children.is_empty());
            session.end_group();

            let parent_result = session.finish_group_body();
            assert!(parent_result.detached_children.is_empty());
            session.end_group();

            (
                parent.anchor,
                child_a.anchor,
                child_b.anchor,
                child_c.anchor,
                child_b_slot,
            )
        });
    harness.finish_pass(SlotPassMode::Compose);
    harness.table.write_value(child_b_slot, 88_i32);

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        begin_unkeyed(session, CHILD_A_KEY, None);
        let child_a_result = session.finish_group_body();
        assert!(child_a_result.detached_children.is_empty());
        session.end_group();

        begin_unkeyed(session, CHILD_C_KEY, None);
        let child_c_result = session.finish_group_body();
        assert!(child_c_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass(SlotPassMode::Compose);

    assert_eq!(
        harness.table.anchors.state(child_b_anchor),
        Some(AnchorState::Detached)
    );
    assert!(
        harness.table.group_for_scope(CHILD_B_SCOPE).is_none(),
        "detached scopes must leave the active scope index"
    );

    let insert_index = harness.table.current_group_index(child_c_anchor);
    let restored_anchor =
        harness
            .table
            .restore_subtree(insert_index, parent_anchor, detached.root_key(), detached);

    assert_eq!(restored_anchor, child_b_anchor);
    assert_eq!(
        harness.table.anchors.state(restored_anchor),
        Some(AnchorState::Active(2))
    );
    assert_eq!(harness.table.groups[1].anchor, child_a_anchor);
    assert_eq!(harness.table.groups[2].anchor, child_b_anchor);
    assert_eq!(harness.table.groups[3].anchor, child_c_anchor);

    let restored_group = harness
        .table
        .group_for_scope(CHILD_B_SCOPE)
        .expect("restored child scope must become active again");
    assert_eq!(harness.table.group_anchor(restored_group), child_b_anchor);
    assert_eq!(*harness.table.read_value::<i32>(child_b_slot), 88);
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn restore_subtree_rejects_mismatched_root_key_without_mutating_table() {
    const PARENT_KEY: Key = 354;
    const CHILD_KEY: Key = 355;

    let (mut harness, detached) = detached_single_child(PARENT_KEY, CHILD_KEY);
    let parent_anchor = harness.table.groups[0].anchor;
    let group_count_before = harness.table.groups.len();
    let detached_anchor = detached
        .group_anchors()
        .next()
        .expect("detached subtree must contain an anchor");
    let wrong_key = GroupKey::new(CHILD_KEY + 1, None, 0);

    let restore = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        harness
            .table
            .restore_subtree(1, parent_anchor, wrong_key, detached);
    }));
    assert!(
        restore.is_err(),
        "restoring a detached subtree under a different group key must be rejected",
    );
    assert_eq!(harness.table.groups.len(), group_count_before);
    assert_eq!(
        harness.table.anchor_state(detached_anchor),
        Some(AnchorState::Detached),
        "failed restore must leave the detached anchor out of the active table",
    );
    harness.table.debug_verify(Some(&harness.lifecycle));
}

#[test]
fn removing_conditional_child_returns_detached_subtree() {
    const PARENT_KEY: Key = 350;
    const CHILD_KEY: Key = 351;
    const CHILD_SCOPE: ScopeId = 91;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let child = begin_unkeyed(session, CHILD_KEY, None);
        session.set_group_scope(child.group, CHILD_SCOPE);
        let _ = session.value_slot(|| 17_i32);
        session.record_node(41, 1);
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass(SlotPassMode::Compose);

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);
        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass(SlotPassMode::Compose);

    assert_eq!(detached.root_key().static_key, CHILD_KEY);
    assert_eq!(detached.root_scope_id(), Some(CHILD_SCOPE));
    assert_eq!(detached.group_count(), 1);
    assert_eq!(detached.node_count(), 1);
    assert_eq!(detached.node_ids_iter().collect::<Vec<_>>(), vec![41]);
    assert_eq!(detached.root_nodes(), &[41]);
    assert_eq!(
        detached.node_states().collect::<Vec<_>>(),
        vec![(41, super::NodeLifecycle::Active)]
    );
    assert_eq!(detached.generation(), 1);
    assert_eq!(detached.scope_ids(), vec![CHILD_SCOPE]);
    assert_eq!(detached.group_anchors().collect::<Vec<_>>().len(), 1);
    assert_eq!(detached.groups[0].parent_anchor, AnchorId::INVALID);
    assert_eq!(detached.groups[0].depth, 0);
}

#[test]
fn retention_marks_detached_nodes_and_reactivates_on_take() {
    const PARENT_KEY: Key = 364;
    const CHILD_KEY: Key = 365;

    let mut harness = SlotHarness::new();
    let child_id = harness
        .applier
        .create(Box::new(UnmountTrackingNode::new(Rc::new(Cell::new(0)))));
    let child_generation = harness.applier.node_generation(child_id);

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        begin_unkeyed(session, CHILD_KEY, None);
        session.record_node(child_id, child_generation);
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass(SlotPassMode::Compose);

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);
        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass(SlotPassMode::Compose);

    let key = RetainKey {
        parent_scope: None,
        key: detached.root_key(),
    };
    let mut retention = RetentionManager::default();
    retention.insert(key, detached);
    assert_eq!(retention.validate(&harness.table), Ok(()));

    let restored = retention.take(key).expect("retained subtree must exist");
    assert_eq!(
        restored.node_states().collect::<Vec<_>>(),
        vec![(child_id, super::NodeLifecycle::Active)]
    );
}

#[test]
fn retention_insert_rejects_duplicate_key_without_replacing_existing_subtree() {
    const PARENT_KEY: Key = 366;
    const CHILD_KEY: Key = 367;

    let (harness, first_detached) = detached_single_child(PARENT_KEY, CHILD_KEY);
    let (_, duplicate_detached) = detached_single_child(PARENT_KEY, CHILD_KEY);
    let retain_key = RetainKey {
        parent_scope: None,
        key: first_detached.root_key(),
    };
    assert_eq!(duplicate_detached.root_key(), retain_key.key);

    let mut retention = RetentionManager::default();
    retention.insert(retain_key, first_detached);

    let duplicate_insert = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        retention.insert(retain_key, duplicate_detached);
    }));
    assert!(
        duplicate_insert.is_err(),
        "retention must reject duplicate retained keys instead of dropping the existing subtree",
    );
    assert_eq!(retention.debug_stats().subtree_count, 1);
    assert_eq!(retention.validate(&harness.table), Ok(()));

    let restored = retention
        .take(retain_key)
        .expect("original retained subtree must remain available after duplicate rejection");
    assert_eq!(restored.root_key(), retain_key.key);
}

#[test]
fn retention_debug_stats_report_retained_payload_anchor_and_heap_counts() {
    const PARENT_KEY: Key = 368;
    const CHILD_KEY: Key = 369;
    const CHILD_SCOPE: ScopeId = 22;

    let mut harness = SlotHarness::new();
    let child_id = harness
        .applier
        .create(Box::new(UnmountTrackingNode::new(Rc::new(Cell::new(0)))));
    let child_generation = harness.applier.node_generation(child_id);

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let child = begin_unkeyed(session, CHILD_KEY, None);
        session.set_group_scope(child.group, CHILD_SCOPE);
        let _remembered = session.remember(|| 91_i32);
        session.record_node(child_id, child_generation);
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 0);
    });
    harness.finish_pass(SlotPassMode::Compose);

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);
        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass(SlotPassMode::Compose);

    let retain_key = RetainKey {
        parent_scope: None,
        key: detached.root_key(),
    };
    let mut retention = RetentionManager::default();
    retention.insert(retain_key, detached);
    let stats = retention.debug_stats();

    assert_eq!(stats.subtree_count, 1);
    assert_eq!(stats.group_count, 1);
    assert_eq!(stats.payload_count, 1);
    assert_eq!(stats.node_count, 1);
    assert_eq!(stats.scope_count, 1);
    assert_eq!(stats.anchor_count, 1);
    assert!(stats.heap_bytes > 0);
}

#[test]
fn finalize_pass_disposes_removed_child_nodes() {
    const PARENT_KEY: Key = 360;
    const CHILD_KEY: Key = 361;

    let mut harness = SlotHarness::new();
    let child_unmounts = Rc::new(Cell::new(0));
    let child_id = harness
        .applier
        .create(Box::new(UnmountTrackingNode::new(Rc::clone(
            &child_unmounts,
        ))));
    let child_generation = harness.applier.node_generation(child_id);

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        begin_unkeyed(session, CHILD_KEY, None);
        session.record_node(child_id, child_generation);
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass(SlotPassMode::Compose);

    assert_eq!(child_unmounts.get(), 0);
    assert_eq!(harness.applier.len(), 1);
    assert!(
        harness
            .applier
            .with_node::<UnmountTrackingNode, _>(child_id, |_| ())
            .is_ok(),
        "active child nodes must remain live before removal"
    );

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);
    });
    harness.finish_pass(SlotPassMode::Compose);

    assert_eq!(child_unmounts.get(), 1);
    assert_eq!(harness.applier.len(), 0);
    assert!(
        harness
            .applier
            .with_node::<UnmountTrackingNode, _>(child_id, |_| ())
            .is_err(),
        "disposed child nodes must be physically removed from the applier"
    );
}

#[test]
fn retained_detached_child_nodes_stay_live_across_restore() {
    const PARENT_KEY: Key = 362;
    const CHILD_KEY: Key = 363;

    let mut harness = SlotHarness::new();
    let child_unmounts = Rc::new(Cell::new(0));
    let child_id = harness
        .applier
        .create(Box::new(UnmountTrackingNode::new(Rc::clone(
            &child_unmounts,
        ))));
    let child_generation = harness.applier.node_generation(child_id);

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        begin_unkeyed(session, CHILD_KEY, None);
        session.record_node(child_id, child_generation);
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass(SlotPassMode::Compose);

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);
        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass(SlotPassMode::Compose);

    assert_eq!(detached.node_ids_iter().collect::<Vec<_>>(), vec![child_id]);
    assert_eq!(child_unmounts.get(), 0);
    assert_eq!(harness.applier.len(), 1);
    assert!(
        harness
            .applier
            .with_node::<UnmountTrackingNode, _>(child_id, |_| ())
            .is_ok(),
        "retained detached nodes must remain live while held by the caller"
    );

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(SlotPassMode::Compose, move |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let child = begin_unkeyed(session, CHILD_KEY, Some(detached));
        assert_eq!(child.kind, GroupStartKind::Restored);
        assert_eq!(
            session.current_node_record(),
            Some((child_id, child_generation)),
            "restored children must expose their retained node for explicit reuse"
        );
        let recorded = session.record_node(child_id, child_generation);
        assert!(recorded.reused);
        assert_eq!(recorded.id, child_id);
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass(SlotPassMode::Compose);

    assert_eq!(child_unmounts.get(), 0);
    assert_eq!(harness.applier.len(), 1);
    assert!(
        harness
            .applier
            .with_node::<UnmountTrackingNode, _>(child_id, |_| ())
            .is_ok(),
        "restored retained nodes must stay live instead of being recreated or disposed"
    );
}

#[test]
fn record_node_result_is_false_when_replacing_existing_node_slot() {
    const GROUP_KEY: Key = 366;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let recorded = session.record_node(11, 1);
        assert!(!recorded.reused);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass(SlotPassMode::Compose);

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, GROUP_KEY, None);
        assert_eq!(session.current_node_record(), Some((11, 1)));
        let recorded = session.record_node(12, 1);
        assert!(
            !recorded.reused,
            "replacing the node at the current cursor must not report a reused node"
        );
        assert_eq!(recorded.id, 12);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass(SlotPassMode::Compose);

    assert_eq!(harness.table.group_node_record_at(0, 0).id, 12);
}

#[test]
fn keyed_sibling_reorder_preserves_values_and_anchors() {
    const PARENT_KEY: Key = 400;
    const STATIC_KEY: Key = 401;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (first_slot, first_anchor, second_slot, second_anchor) =
        harness.session(SlotPassMode::Compose, |session| {
            begin_unkeyed(session, PARENT_KEY, None);

            let first = begin_keyed(session, STATIC_KEY, 1, None);
            let first_slot = session.value_slot(|| 10_i32);
            let first_result = session.finish_group_body();
            assert!(first_result.detached_children.is_empty());
            session.end_group();

            let second = begin_keyed(session, STATIC_KEY, 2, None);
            let second_slot = session.value_slot(|| 20_i32);
            let second_result = session.finish_group_body();
            assert!(second_result.detached_children.is_empty());
            session.end_group();

            let parent_result = session.finish_group_body();
            assert!(parent_result.detached_children.is_empty());
            session.end_group();

            (first_slot, first.anchor, second_slot, second.anchor)
        });
    harness.finish_pass(SlotPassMode::Compose);
    harness.table.write_value(first_slot, 101_i32);
    harness.table.write_value(second_slot, 202_i32);

    harness.begin_pass(SlotPassMode::Compose);
    let (
        second_kind,
        reordered_second_anchor,
        reordered_second_slot,
        first_kind,
        reordered_first_anchor,
        reordered_first_slot,
    ) = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let second = begin_keyed(session, STATIC_KEY, 2, None);
        let second_slot = session.value_slot(|| 0_i32);
        let second_result = session.finish_group_body();
        assert!(second_result.detached_children.is_empty());
        session.end_group();

        let first = begin_keyed(session, STATIC_KEY, 1, None);
        let first_slot = session.value_slot(|| 0_i32);
        let first_result = session.finish_group_body();
        assert!(first_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        (
            second.kind,
            second.anchor,
            second_slot,
            first.kind,
            first.anchor,
            first_slot,
        )
    });
    harness.finish_pass(SlotPassMode::Compose);

    assert_eq!(second_kind, GroupStartKind::Moved);
    assert_eq!(reordered_second_anchor, second_anchor);
    assert_eq!(first_kind, GroupStartKind::Reused);
    assert_eq!(reordered_first_anchor, first_anchor);
    assert_eq!(*harness.table.read_value::<i32>(reordered_second_slot), 202);
    assert_eq!(*harness.table.read_value::<i32>(reordered_first_slot), 101);
}

#[test]
fn large_keyed_sibling_reorder_preserves_values_and_anchors() {
    const PARENT_KEY: Key = 405;
    const STATIC_KEY: Key = 406;
    const CHILD_COUNT: Key = 24;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let original = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);
        let mut children = Vec::new();

        for key in 1..=CHILD_COUNT {
            let child = begin_keyed(session, STATIC_KEY, key, None);
            let slot = session.value_slot(|| key as i32);
            let result = session.finish_group_body();
            assert!(result.detached_children.is_empty());
            session.end_group();
            children.push((key, child.anchor, slot));
        }

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
        children
    });
    harness.finish_pass(SlotPassMode::Compose);

    for (key, _, slot) in original.iter().copied() {
        harness.table.write_value(slot, 10_000 + key as i32);
    }

    harness.begin_pass(SlotPassMode::Compose);
    let reordered = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);
        let mut children = Vec::new();

        for key in (1..=CHILD_COUNT).rev() {
            let child = begin_keyed(session, STATIC_KEY, key, None);
            assert!(
                matches!(child.kind, GroupStartKind::Reused | GroupStartKind::Moved),
                "large keyed reorder must reuse or move existing child {key}, got {:?}",
                child.kind,
            );
            let slot = session.value_slot(|| -1_i32);
            let result = session.finish_group_body();
            assert!(result.detached_children.is_empty());
            session.end_group();
            children.push((key, child.anchor, slot));
        }

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
        children
    });
    harness.finish_pass(SlotPassMode::Compose);

    for (key, anchor, slot) in reordered {
        let (_, original_anchor, _) = original
            .iter()
            .copied()
            .find(|(original_key, _, _)| *original_key == key)
            .expect("original child key must exist");
        assert_eq!(anchor, original_anchor);
        assert_eq!(*harness.table.read_value::<i32>(slot), 10_000 + key as i32);
    }
}

#[test]
fn keyed_sibling_search_never_matches_a_grandchild() {
    const PARENT_KEY: Key = 410;
    const STATIC_KEY: Key = 411;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (direct_anchor, grandchild_anchor) = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let _first = begin_keyed(session, STATIC_KEY, 1, None);
        let grandchild = begin_keyed(session, STATIC_KEY, 2, None);
        let grandchild_result = session.finish_group_body();
        assert!(grandchild_result.detached_children.is_empty());
        session.end_group();

        let first_result = session.finish_group_body();
        assert!(first_result.detached_children.is_empty());
        session.end_group();

        let second = begin_keyed(session, STATIC_KEY, 2, None);
        let second_result = session.finish_group_body();
        assert!(second_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        (second.anchor, grandchild.anchor)
    });
    harness.finish_pass(SlotPassMode::Compose);

    harness.begin_pass(SlotPassMode::Compose);
    let (moved_anchor, reused_grandchild_anchor) =
        harness.session(SlotPassMode::Compose, |session| {
            begin_unkeyed(session, PARENT_KEY, None);

            let moved = begin_keyed(session, STATIC_KEY, 2, None);
            assert_eq!(moved.kind, GroupStartKind::Moved);
            let moved_result = session.finish_group_body();
            assert!(moved_result.detached_children.is_empty());
            session.end_group();

            let _first = begin_keyed(session, STATIC_KEY, 1, None);
            let reused_grandchild = begin_keyed(session, STATIC_KEY, 2, None);
            assert_eq!(reused_grandchild.kind, GroupStartKind::Reused);
            let grandchild_result = session.finish_group_body();
            assert!(grandchild_result.detached_children.is_empty());
            session.end_group();

            let first_result = session.finish_group_body();
            assert!(first_result.detached_children.is_empty());
            session.end_group();

            let parent_result = session.finish_group_body();
            assert!(parent_result.detached_children.is_empty());
            session.end_group();

            (moved.anchor, reused_grandchild.anchor)
        });
    harness.finish_pass(SlotPassMode::Compose);

    assert_eq!(moved_anchor, direct_anchor);
    assert_eq!(reused_grandchild_anchor, grandchild_anchor);
}

#[test]
fn restored_keyed_sibling_can_move_on_a_later_pass() {
    const PARENT_KEY: Key = 440;
    const STATIC_KEY: Key = 441;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (second_slot, second_anchor) = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let first = begin_keyed(session, STATIC_KEY, 1, None);
        let first_result = session.finish_group_body();
        assert!(first_result.detached_children.is_empty());
        session.end_group();

        let second = begin_keyed(session, STATIC_KEY, 2, None);
        let second_slot = session.value_slot(|| 20_i32);
        let second_result = session.finish_group_body();
        assert!(second_result.detached_children.is_empty());
        session.end_group();

        let third = begin_keyed(session, STATIC_KEY, 3, None);
        let third_result = session.finish_group_body();
        assert!(third_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        assert_eq!(first.kind, GroupStartKind::Inserted);
        assert_eq!(third.kind, GroupStartKind::Inserted);
        (second_slot, second.anchor)
    });
    harness.finish_pass(SlotPassMode::Compose);
    harness.table.write_value(second_slot, 202_i32);

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let first = begin_keyed(session, STATIC_KEY, 1, None);
        let first_result = session.finish_group_body();
        assert!(first_result.detached_children.is_empty());
        session.end_group();

        let third = begin_keyed(session, STATIC_KEY, 3, None);
        let third_result = session.finish_group_body();
        assert!(third_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        session.end_group();

        assert_eq!(first.kind, GroupStartKind::Reused);
        assert_eq!(third.kind, GroupStartKind::Moved);
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass(SlotPassMode::Compose);

    harness.begin_pass(SlotPassMode::Compose);
    let (restored_kind, restored_anchor, restored_slot) =
        harness.session(SlotPassMode::Compose, move |session| {
            begin_unkeyed(session, PARENT_KEY, None);

            let first = begin_keyed(session, STATIC_KEY, 1, None);
            let first_result = session.finish_group_body();
            assert!(first_result.detached_children.is_empty());
            session.end_group();

            let second = begin_keyed(session, STATIC_KEY, 2, Some(detached));
            let second_slot = session.value_slot(|| 0_i32);
            let second_result = session.finish_group_body();
            assert!(second_result.detached_children.is_empty());
            session.end_group();

            let third = begin_keyed(session, STATIC_KEY, 3, None);
            let third_result = session.finish_group_body();
            assert!(third_result.detached_children.is_empty());
            session.end_group();

            let parent_result = session.finish_group_body();
            assert!(parent_result.detached_children.is_empty());
            session.end_group();

            assert_eq!(first.kind, GroupStartKind::Reused);
            assert_eq!(third.kind, GroupStartKind::Reused);
            (second.kind, second.anchor, second_slot)
        });
    harness.finish_pass(SlotPassMode::Compose);

    assert_eq!(restored_kind, GroupStartKind::Restored);
    assert_eq!(restored_anchor, second_anchor);
    assert_eq!(restored_slot, second_slot);
    assert_eq!(*harness.table.read_value::<i32>(restored_slot), 202);

    harness.begin_pass(SlotPassMode::Compose);
    let (moved_kind, moved_anchor, moved_slot) =
        harness.session(SlotPassMode::Compose, |session| {
            begin_unkeyed(session, PARENT_KEY, None);

            let second = begin_keyed(session, STATIC_KEY, 2, None);
            let second_slot = session.value_slot(|| 0_i32);
            let second_result = session.finish_group_body();
            assert!(second_result.detached_children.is_empty());
            session.end_group();

            let first = begin_keyed(session, STATIC_KEY, 1, None);
            let first_result = session.finish_group_body();
            assert!(first_result.detached_children.is_empty());
            session.end_group();

            let third = begin_keyed(session, STATIC_KEY, 3, None);
            let third_result = session.finish_group_body();
            assert!(third_result.detached_children.is_empty());
            session.end_group();

            let parent_result = session.finish_group_body();
            assert!(parent_result.detached_children.is_empty());
            session.end_group();

            assert_eq!(first.kind, GroupStartKind::Reused);
            assert_eq!(third.kind, GroupStartKind::Reused);
            (second.kind, second.anchor, second_slot)
        });
    harness.finish_pass(SlotPassMode::Compose);

    assert_eq!(moved_kind, GroupStartKind::Moved);
    assert_eq!(moved_anchor, second_anchor);
    assert_eq!(moved_slot, second_slot);
    assert_eq!(*harness.table.read_value::<i32>(moved_slot), 202);
}

#[test]
fn unkeyed_siblings_follow_positional_identity() {
    const PARENT_KEY: Key = 450;
    const SHARED_KEY: Key = 451;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (first_slot, second_slot) = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        begin_unkeyed(session, SHARED_KEY, None);
        let first_slot = session.value_slot(|| 10_i32);
        let first_result = session.finish_group_body();
        assert!(first_result.detached_children.is_empty());
        session.end_group();

        begin_unkeyed(session, SHARED_KEY, None);
        let second_slot = session.value_slot(|| 20_i32);
        let second_result = session.finish_group_body();
        assert!(second_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        (first_slot, second_slot)
    });
    harness.finish_pass(SlotPassMode::Compose);
    harness.table.write_value(first_slot, 101_i32);
    harness.table.write_value(second_slot, 202_i32);

    harness.begin_pass(SlotPassMode::Compose);
    let remaining_slot = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let remaining = begin_unkeyed(session, SHARED_KEY, None);
        assert_eq!(remaining.kind, GroupStartKind::Reused);
        let slot = session.value_slot(|| 0_i32);
        let result = session.finish_group_body();
        assert_eq!(result.detached_children.len(), 0);
        session.end_group();

        let parent_result = session.finish_group_body();
        assert_eq!(parent_result.detached_children.len(), 1);
        session.end_group();
        slot
    });
    harness.finish_pass(SlotPassMode::Compose);

    assert_eq!(
            *harness.table.read_value::<i32>(remaining_slot),
            101,
            "unkeyed duplicate siblings must follow positional identity, not the removed sibling's previous state",
        );
}

#[test]
fn duplicate_explicit_keys_are_rejected_during_writer_traversal() {
    const PARENT_KEY: Key = 460;
    const STATIC_KEY: Key = 461;
    const EXPLICIT_KEY: Key = 77;

    let mut table = SlotTable::new();
    let mut lifecycle = SlotLifecycleCoordinator::default();
    let mut state = SlotWriteSessionState::default();
    state.reset_for_pass(&table, SlotPassMode::Compose);
    {
        let mut session = table.write_session(&mut lifecycle, &mut state, SlotPassMode::Compose);
        begin_unkeyed(&mut session, PARENT_KEY, None);

        begin_keyed(&mut session, STATIC_KEY, EXPLICIT_KEY, None);
        let first = session.finish_group_body();
        assert!(first.detached_children.is_empty());
        session.end_group();

        let duplicate = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            begin_keyed(&mut session, STATIC_KEY, EXPLICIT_KEY, None);
        }));
        assert!(
            duplicate.is_err(),
            "duplicate explicit sibling keys must fail before mutating the slot table",
        );

        let parent = session.finish_group_body();
        assert!(parent.detached_children.is_empty());
        session.end_group();
    }

    assert_eq!(table.validate(), Ok(()));
}

#[test]
fn validate_reports_duplicate_sibling_key_structurally() {
    const PARENT_KEY: Key = 462;
    const STATIC_KEY: Key = 463;

    let mut table = SlotTable::new();
    let mut lifecycle = SlotLifecycleCoordinator::default();
    let mut state = SlotWriteSessionState::default();
    state.reset_for_pass(&table, SlotPassMode::Compose);
    {
        let mut session = table.write_session(&mut lifecycle, &mut state, SlotPassMode::Compose);
        begin_unkeyed(&mut session, PARENT_KEY, None);

        begin_keyed(&mut session, STATIC_KEY, 1, None);
        let first = session.finish_group_body();
        assert!(first.detached_children.is_empty());
        session.end_group();

        begin_keyed(&mut session, STATIC_KEY, 2, None);
        let second = session.finish_group_body();
        assert!(second.detached_children.is_empty());
        session.end_group();

        let parent = session.finish_group_body();
        assert!(parent.detached_children.is_empty());
        session.end_group();
    }

    table.groups[2].key = table.groups[1].key;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::DuplicateSiblingKey {
            parent_anchor: table.groups[0].anchor,
            key: table.groups[1].key,
        })
    );
}

#[test]
fn validate_reports_invalid_parent_structurally() {
    let mut table = composed_parent_child_table(470, 471, None);
    table.groups[1].parent_anchor = AnchorId::INVALID;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::InvalidParent {
            group_index: 1,
            expected: table.groups[0].anchor,
            actual: AnchorId::INVALID,
        })
    );
}

#[test]
fn validate_reports_bad_subtree_len_structurally() {
    let mut table = composed_parent_child_table(472, 473, None);
    table.groups[1].subtree_len = 0;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::BadSubtreeLen {
            group_index: 1,
            expected: 0,
            actual: 0,
        })
    );
}

#[test]
fn validate_reports_scope_index_mismatch_structurally() {
    const SCOPE_ID: ScopeId = 64;
    const STALE_SCOPE_ID: ScopeId = 65;

    let mut table = composed_parent_child_table(474, 475, Some(SCOPE_ID));
    table.groups[1].scope_id = Some(STALE_SCOPE_ID);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::ScopeIndexMismatch {
            scope_id: STALE_SCOPE_ID,
            expected: table.groups[1].anchor,
            actual: None,
        })
    );
}

#[test]
fn validate_reports_anchor_mismatch_for_wrong_active_index_structurally() {
    let mut table = composed_parent_child_table(476, 477, None);
    let root_anchor = table.groups[0].anchor;
    table.anchors.set_active(root_anchor, 1);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::AnchorMismatch {
            anchor: root_anchor,
            expected: 0,
            actual: Some(AnchorState::Active(1)),
        })
    );
}

#[test]
fn validate_reports_anchor_mismatch_for_detached_anchor_structurally() {
    let mut table = composed_parent_child_table(487, 488, None);
    let root_anchor = table.groups[0].anchor;
    table.anchors.mark_detached(root_anchor);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::AnchorMismatch {
            anchor: root_anchor,
            expected: 0,
            actual: Some(AnchorState::Detached),
        })
    );
}

#[test]
fn validate_reports_group_anchor_count_mismatch_structurally() {
    let mut table = composed_parent_child_table(480, 481, None);
    table.anchors.set_active(AnchorId::new(2_000), 9);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::GroupAnchorCountMismatch {
            expected: 2,
            actual: 3,
        })
    );
}

#[test]
fn validate_reports_anchor_mismatch_for_missing_anchor_structurally() {
    let mut table = composed_parent_child_table(494, 495, None);
    let root_anchor = table.groups[0].anchor;
    table.anchors.invalidate(root_anchor);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::AnchorMismatch {
            anchor: root_anchor,
            expected: 0,
            actual: None,
        })
    );
}

#[test]
fn validate_reports_bad_depth_structurally() {
    let mut table = composed_parent_child_table(489, 490, None);
    table.groups[1].depth = 0;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::BadDepth {
            group_index: 1,
            expected: 1,
            actual: 0,
        })
    );
}

#[test]
fn validate_reports_payload_owner_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(478);
    let payload_anchor = table.group_payload_record_at(0, 0).anchor;
    table.group_payload_record_at_mut(0, 0).owner = AnchorId::INVALID;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::PayloadOwnerMismatch {
            payload_anchor,
            expected: table.groups[0].anchor,
            actual: AnchorId::INVALID,
        })
    );
}

#[test]
fn validate_reports_payload_start_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(481);
    table.groups[0].payload_start = 1;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::PayloadStartMismatch {
            group_index: 0,
            expected: 0,
            actual: 1,
        })
    );
}

#[test]
fn validate_reports_payload_out_of_range_structurally() {
    let mut table = composed_group_with_value_and_node_table(484);
    table.groups[0].payload_len = 2;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::PayloadOutOfRange {
            group_index: 0,
            start: 0,
            len: 2,
            payload_count: 1,
        })
    );
}

#[test]
fn validate_reports_payload_location_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(482);
    let stale_payload_anchor = table.group_payload_record_at(0, 0).anchor;
    table.group_payload_record_at_mut(0, 0).anchor = stale_payload_anchor + 1;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::PayloadLocationMismatch {
            payload_anchor: stale_payload_anchor + 1,
            expected: (table.groups[0].anchor, 0),
            actual: None,
        })
    );
}

#[test]
fn validate_reports_payload_location_stale_owner_structurally() {
    let mut table = composed_group_with_value_and_node_table(491);
    let old_anchor = table.groups[0].anchor;
    let new_anchor = AnchorId::new(1_001);
    let payload_anchor = table.group_payload_record_at(0, 0).anchor;

    table.groups[0].anchor = new_anchor;
    table.group_payload_record_at_mut(0, 0).owner = new_anchor;
    table.anchors.invalidate(old_anchor);
    table.anchors.set_active(new_anchor, 0);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::PayloadLocationMismatch {
            payload_anchor,
            expected: (new_anchor, 0),
            actual: Some((old_anchor, 0)),
        })
    );
}

#[test]
fn validate_reports_payload_anchor_count_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(483);
    table
        .payload_locations
        .insert(999, table.groups[0].anchor, 0);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::PayloadAnchorCountMismatch {
            expected: 1,
            actual: 2,
        })
    );
}

#[test]
fn compact_storage_discards_removed_payload_location_entries() {
    let mut table = composed_group_with_value_and_node_table(601);
    let owner = table.groups[0].anchor;
    let payload_anchor = table.group_payload_record_at(0, 0).anchor;

    let removed = table.remove_payload_range(owner, 0, 1);
    assert_eq!(removed.len(), 1);
    assert_eq!(table.payload_locations.get(payload_anchor), None);

    let capacity_before = table.payload_locations.capacity();
    table.compact_storage();
    let capacity_after = table.payload_locations.capacity();

    assert_eq!(table.payload_locations.len(), 0);
    assert!(
        capacity_after < capacity_before,
        "compaction must drop removed payload-location entries: before={capacity_before} after={capacity_after}",
    );
    assert_eq!(table.validate(), Ok(()));
}

#[test]
fn compact_payload_namespace_remaps_retained_subtree_payloads() {
    const PARENT_KEY: Key = 610;
    const CHILD_KEY: Key = 611;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let parent_anchor = harness.session(SlotPassMode::Compose, |session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        let _ = session.value_slot(|| 10_i32);

        begin_unkeyed(session, CHILD_KEY, None);
        let _ = session.value_slot(|| 20_i32);
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        parent.anchor
    });
    harness.finish_pass(SlotPassMode::Compose);

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);
        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass(SlotPassMode::Compose);

    let retain_key = RetainKey {
        parent_scope: None,
        key: detached.root_key(),
    };
    let restore_key = detached.root_key();
    let mut retention = RetentionManager::default();
    retention.insert(retain_key, detached);

    harness
        .table
        .compact_payload_anchor_namespace(Some(&mut retention));
    let _ = harness.table.insert_value_payload(
        parent_anchor,
        0,
        1,
        super::PayloadKind::Internal,
        30_i32,
    );

    let restored = retention
        .take(retain_key)
        .expect("retained subtree must restore");
    harness
        .table
        .restore_subtree(1, parent_anchor, restore_key, restored);

    let payload_anchor_count = harness
        .table
        .payloads
        .iter()
        .map(|payload| payload.anchor)
        .collect::<HashSet<_>>()
        .len();
    assert_eq!(payload_anchor_count, harness.table.payloads.len());
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn validate_reports_node_owner_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(479);
    let node_id = table.group_node_record_at(0, 0).id;
    table.group_node_record_at_mut(0, 0).owner = AnchorId::INVALID;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::NodeOwnerMismatch {
            node_id,
            expected: table.groups[0].anchor,
            actual: AnchorId::INVALID,
        })
    );
}

#[test]
fn validate_reports_node_lifecycle_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(484);
    let node_id = table.group_node_record_at(0, 0).id;
    table.group_node_record_at_mut(0, 0).lifecycle = super::NodeLifecycle::Disposed;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::NodeLifecycleMismatch {
            node_id,
            expected: super::NodeLifecycle::Active,
            actual: super::NodeLifecycle::Disposed,
        })
    );
}

#[test]
fn validate_reports_node_start_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(485);
    table.groups[0].node_start = 1;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::NodeStartMismatch {
            group_index: 0,
            expected: 0,
            actual: 1,
        })
    );
}

#[test]
fn validate_reports_node_out_of_range_structurally() {
    let mut table = composed_group_with_value_and_node_table(486);
    table.groups[0].node_len = 2;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::NodeOutOfRange {
            group_index: 0,
            start: 0,
            len: 2,
            node_count: 1,
        })
    );
}

#[test]
fn validate_reports_node_count_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(487);
    table.nodes.push(super::NodeRecord {
        owner: table.groups[0].anchor,
        id: 999,
        parent_id: None,
        generation: 1,
        lifecycle: super::NodeLifecycle::Active,
    });

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::NodeCountMismatch {
            expected: 1,
            actual: 2,
        })
    );
}

#[test]
fn validate_reports_scope_index_stale_anchor_structurally() {
    const SCOPE_ID: ScopeId = 67;

    let mut table = composed_parent_child_table(492, 493, Some(SCOPE_ID));
    let old_anchor = table.groups[1].anchor;
    let new_anchor = AnchorId::new(1_002);

    table.groups[1].anchor = new_anchor;
    table.anchors.invalidate(old_anchor);
    table.anchors.set_active(new_anchor, 1);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::ScopeIndexMismatch {
            scope_id: SCOPE_ID,
            expected: new_anchor,
            actual: Some(old_anchor),
        })
    );
}

#[test]
fn validate_reports_scope_index_count_mismatch_structurally() {
    const SCOPE_ID: ScopeId = 66;

    let mut table = composed_parent_child_table(484, 485, Some(SCOPE_ID));
    table.groups[1].scope_id = None;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::ScopeIndexCountMismatch {
            expected: 0,
            actual: 1,
        })
    );
}

#[test]
fn validate_reports_bad_subtree_node_count_structurally() {
    let mut table = composed_group_with_value_and_node_table(486);
    table.groups[0].subtree_node_count = 0;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::BadSubtreeNodeCount {
            group_index: 0,
            expected: 1,
            actual: 0,
        })
    );
}

#[test]
fn skip_group_advances_by_exact_subtree_size_and_keeps_nodes_stable() {
    const PARENT_KEY: Key = 500;
    const CHILD_A_KEY: Key = 501;
    const GRANDCHILD_KEY: Key = 502;
    const CHILD_B_KEY: Key = 503;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        begin_unkeyed(session, CHILD_A_KEY, None);
        session.record_node_with_parent(10, 1, None);
        begin_unkeyed(session, GRANDCHILD_KEY, None);
        session.record_node_with_parent(20, 1, Some(10));
        let grandchild_result = session.finish_group_body();
        assert!(grandchild_result.detached_children.is_empty());
        session.end_group();
        let child_a_result = session.finish_group_body();
        assert!(child_a_result.detached_children.is_empty());
        session.end_group();

        begin_unkeyed(session, CHILD_B_KEY, None);
        session.record_node(30, 1);
        let child_b_result = session.finish_group_body();
        assert!(child_b_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass(SlotPassMode::Compose);

    harness.begin_pass(SlotPassMode::Compose);
    let child_b_kind = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let child_a = begin_unkeyed(session, CHILD_A_KEY, None);
        assert_eq!(child_a.kind, GroupStartKind::Reused);
        assert_eq!(session.nodes_in_current_group(), vec![10, 20]);
        session.skip_group();
        let child_a_result = session.finish_group_body();
        assert!(child_a_result.detached_children.is_empty());
        assert_eq!(child_a_result.subtree_nodes, vec![10, 20]);
        assert_eq!(child_a_result.root_nodes, vec![10]);
        assert!(child_a_result.was_skipped);
        session.end_group();

        let child_b = begin_unkeyed(session, CHILD_B_KEY, None);
        let child_b_result = session.finish_group_body();
        assert!(child_b_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        child_b.kind
    });
    harness.finish_pass(SlotPassMode::Compose);

    assert_eq!(child_b_kind, GroupStartKind::Reused);
}

#[test]
fn scope_index_resolves_active_groups_and_omits_detached_ones() {
    const GROUP_KEY: Key = 600;
    const SCOPE_ID: ScopeId = 55;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let group_anchor = harness.session(SlotPassMode::Compose, |session| {
        let started = begin_unkeyed(session, GROUP_KEY, None);
        session.set_group_scope(started.group, SCOPE_ID);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        started.anchor
    });
    harness.finish_pass(SlotPassMode::Compose);

    harness.begin_pass(SlotPassMode::Recompose);
    harness.session(SlotPassMode::Recompose, |session| {
        let group = session
            .begin_recompose_at_scope(SCOPE_ID)
            .expect("active scope must resolve through the scope index");
        assert_eq!(group.index(), 0);
        session.skip_group();
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_recompose();
    });
    harness.finish_pass(SlotPassMode::Recompose);

    harness.begin_pass(SlotPassMode::Compose);
    harness.finish_pass(SlotPassMode::Compose);

    assert!(harness.table.groups.is_empty());
    assert!(
        !harness.table.anchors.contains_active(group_anchor),
        "disposed groups must invalidate their active anchor lookup"
    );
    assert_eq!(
        harness.table.anchors.state(group_anchor),
        None,
        "disposed groups must remove their anchor lookup"
    );

    harness.begin_pass(SlotPassMode::Recompose);
    harness.session(SlotPassMode::Recompose, |session| {
        assert!(
            session.begin_recompose_at_scope(SCOPE_ID).is_none(),
            "detached or disposed scopes must not resolve through the active-table scope index"
        );
    });
    harness.finish_pass(SlotPassMode::Recompose);
}

#[test]
fn detached_subtree_preserves_root_nodes_from_stored_parent_links() {
    const PARENT_KEY: Key = 611;
    const CHILD_KEY: Key = 612;
    const GRANDCHILD_KEY: Key = 613;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        begin_unkeyed(session, CHILD_KEY, None);
        session.record_node_with_parent(41, 1, None);
        begin_unkeyed(session, GRANDCHILD_KEY, None);
        session.record_node_with_parent(42, 1, Some(41));
        let grandchild_result = session.finish_group_body();
        assert!(grandchild_result.detached_children.is_empty());
        session.end_group();
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass(SlotPassMode::Compose);

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(SlotPassMode::Compose, |session| {
        begin_unkeyed(session, PARENT_KEY, None);
        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass(SlotPassMode::Compose);

    assert_eq!(detached.node_ids_iter().collect::<Vec<_>>(), vec![41, 42]);
    assert_eq!(detached.root_nodes(), &[41]);
}

#[test]
fn set_group_scope_replaces_previous_scope_lookup() {
    const GROUP_KEY: Key = 610;
    const OLD_SCOPE_ID: ScopeId = 56;
    const NEW_SCOPE_ID: ScopeId = 57;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(SlotPassMode::Compose, |session| {
        let started = begin_unkeyed(session, GROUP_KEY, None);
        session.set_group_scope(started.group, OLD_SCOPE_ID);
        session.set_group_scope(started.group, NEW_SCOPE_ID);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass(SlotPassMode::Compose);

    harness.begin_pass(SlotPassMode::Recompose);
    harness.session(SlotPassMode::Recompose, |session| {
        assert!(
            session.begin_recompose_at_scope(OLD_SCOPE_ID).is_none(),
            "replaced scope ids must leave no stale active lookup"
        );
        let group = session
            .begin_recompose_at_scope(NEW_SCOPE_ID)
            .expect("the latest scope id must resolve through the scope index");
        assert_eq!(group.index(), 0);
        session.skip_group();
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_recompose();
    });
    harness.finish_pass(SlotPassMode::Recompose);
}

#[test]
fn released_anchors_are_reused_without_sparse_growth() {
    const GROUP_COUNT: usize = 2048;

    let mut harness = SlotHarness::new();
    let first_anchor = harness.session(SlotPassMode::Compose, |session| {
        let mut first_anchor = None;
        for key in 0..GROUP_COUNT {
            let started = begin_unkeyed(session, key as Key + 10_000, None);
            first_anchor.get_or_insert(started.anchor);
            let result = session.finish_group_body();
            assert!(result.detached_children.is_empty());
            session.end_group();
        }
        first_anchor.expect("first anchor")
    });
    harness.finish_pass(SlotPassMode::Compose);

    harness.begin_pass(SlotPassMode::Compose);
    harness.finish_pass(SlotPassMode::Compose);
    assert_eq!(harness.table.anchor_state(first_anchor), None);
    let stats_after_clear = harness.table.debug_stats();
    let capacity_after_clear = stats_after_clear.anchor_capacity;
    assert!(
        capacity_after_clear <= GROUP_COUNT * 2,
        "releasing groups must not leave sparse anchor storage behind: after_clear={capacity_after_clear}",
    );
    assert_eq!(stats_after_clear.active_anchor_count, 0);
    assert_eq!(stats_after_clear.anchor_slot_count, 0);
    assert_eq!(stats_after_clear.anchor_sparse_count, 0);
    assert_eq!(stats_after_clear.detached_anchor_count, 0);
    assert_eq!(stats_after_clear.invalidated_anchor_count, 0);
    assert_eq!(stats_after_clear.free_anchor_count, GROUP_COUNT);

    harness.begin_pass(SlotPassMode::Compose);
    let reused_anchor = harness.session(SlotPassMode::Compose, |session| {
        let started = begin_unkeyed(session, 90_000, None);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        started.anchor
    });
    harness.finish_pass(SlotPassMode::Compose);

    let stats_after_reuse = harness.table.debug_stats();
    let capacity_after_reuse = stats_after_reuse.anchor_capacity;
    assert!(reused_anchor.generation > 1);
    assert_ne!(reused_anchor, first_anchor);
    assert!(
        capacity_after_reuse <= GROUP_COUNT * 2,
        "reusing released anchors must keep storage bounded: after_reuse={capacity_after_reuse}",
    );
    assert_eq!(stats_after_reuse.active_anchor_count, 1);
    assert_eq!(stats_after_reuse.anchor_slot_count, 1);
    assert_eq!(stats_after_reuse.anchor_sparse_count, 0);
    assert_eq!(stats_after_reuse.detached_anchor_count, 0);
    assert_eq!(stats_after_reuse.invalidated_anchor_count, 0);
    assert_eq!(stats_after_reuse.free_anchor_count, GROUP_COUNT - 1);
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn stale_group_handle_does_not_alias_recreated_group() {
    const OLD_KEY: Key = 611;
    const NEW_KEY: Key = 612;
    const SCOPE_ID: ScopeId = 58;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let stale_group = harness.session(SlotPassMode::Compose, |session| {
        let started = begin_unkeyed(session, OLD_KEY, None);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        started.group
    });
    harness.finish_pass(SlotPassMode::Compose);

    harness.begin_pass(SlotPassMode::Compose);
    harness.finish_pass(SlotPassMode::Compose);
    assert!(harness.table.groups.is_empty());

    harness.begin_pass(SlotPassMode::Compose);
    let recreated_group = harness.session(SlotPassMode::Compose, |session| {
        let started = begin_unkeyed(session, NEW_KEY, None);
        assert_eq!(started.group.index(), stale_group.index());
        assert_ne!(started.group.generation(), stale_group.generation());

        let stale_assign = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.set_group_scope(stale_group, SCOPE_ID);
        }));
        assert!(
            stale_assign.is_err(),
            "stale group handles must not alias a newly created group"
        );

        session.set_group_scope(started.group, SCOPE_ID);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        started.group
    });
    harness.finish_pass(SlotPassMode::Compose);

    harness.begin_pass(SlotPassMode::Recompose);
    harness.session(SlotPassMode::Recompose, |session| {
        let group = session
            .begin_recompose_at_scope(SCOPE_ID)
            .expect("the recreated group must resolve through the scope index");
        assert_eq!(group, recreated_group);
        assert_ne!(group.generation(), stale_group.generation());
        session.skip_group();
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_recompose();
    });
    harness.finish_pass(SlotPassMode::Recompose);
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ModelChild {
    value: i32,
    node_id: NodeId,
    scope_id: ScopeId,
}

#[derive(Debug)]
enum ModelOperation {
    Compose {
        order: Vec<Key>,
        retain_detached: HashSet<Key>,
        mutate_values: HashSet<Key>,
    },
    RecomposeSkip {
        key: Key,
    },
}

#[derive(Debug, Default)]
struct ModelState {
    active_order: Vec<Key>,
    active: BTreeMap<Key, ModelChild>,
    retained: BTreeMap<Key, ModelChild>,
    next_node_id: NodeId,
}

impl ModelState {
    fn scope_id_for(key: Key) -> ScopeId {
        10_000 + key as ScopeId
    }
}

fn next_index(seed: &mut u64, len: usize) -> usize {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    (*seed as usize) % len
}

fn generate_model_operation(
    seed: &mut u64,
    model: &ModelState,
    children: &[Key],
) -> ModelOperation {
    if !model.active_order.is_empty() && next_bool(seed) {
        let key = model.active_order[next_index(seed, model.active_order.len())];
        return ModelOperation::RecomposeSkip { key };
    }

    let mut order = children
        .iter()
        .copied()
        .filter(|_| next_bool(seed))
        .collect::<Vec<_>>();
    shuffle(&mut order, seed);

    let order_set = order.iter().copied().collect::<HashSet<_>>();
    let retain_detached = model
        .active_order
        .iter()
        .copied()
        .filter(|key| !order_set.contains(key) && next_bool(seed))
        .collect::<HashSet<_>>();
    let mutate_values = order
        .iter()
        .copied()
        .filter(|_| next_bool(seed))
        .collect::<HashSet<_>>();

    ModelOperation::Compose {
        order,
        retain_detached,
        mutate_values,
    }
}

fn assert_model_matches_slot_table(
    harness: &SlotHarness,
    retained_subtrees: &BTreeMap<Key, DetachedSubtree>,
    model: &ModelState,
    parent_key: Key,
    child_static_key: Key,
) {
    let snapshot = harness.table.debug_snapshot();
    assert_eq!(snapshot.active_groups.len(), model.active_order.len() + 1);
    assert_eq!(snapshot.active_payload_count, model.active_order.len());
    assert_eq!(snapshot.active_node_count, model.active_order.len());
    assert_eq!(snapshot.active_scope_count, model.active_order.len());
    assert_eq!(snapshot.scope_registry_count, model.active_order.len());

    let root = snapshot
        .active_groups
        .first()
        .expect("model test root group must exist");
    assert_eq!(root.static_key, parent_key);
    assert_eq!(root.depth, 0);
    assert_eq!(root.subtree_len as usize, model.active_order.len() + 1);
    assert_eq!(root.payload_len, 0);
    assert_eq!(root.node_len, 0);

    let active_children = snapshot
        .active_groups
        .iter()
        .skip(1)
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(active_children.len(), model.active_order.len());

    for (group, key) in active_children.iter().zip(model.active_order.iter()) {
        let expected = model.active.get(key).expect("active model child");
        assert_eq!(group.static_key, child_static_key);
        assert_eq!(group.explicit_key, Some(*key));
        assert_eq!(group.ordinal, 0);
        assert_eq!(group.scope_id, Some(expected.scope_id));
        assert_eq!(group.depth, 1);
        assert_eq!(group.subtree_len, 1);
        assert_eq!(group.payload_len, 1);
        assert_eq!(group.node_len, 1);
        assert_eq!(group.subtree_node_count, 1);
        assert_eq!(group.parent_anchor, root.anchor);

        let payload = harness
            .table
            .group_payload_records_at(group.index)
            .first()
            .expect("model child payload must exist")
            .value
            .downcast_ref::<i32>()
            .expect("model child payload must be i32");
        assert_eq!(
            *payload, expected.value,
            "remembered values must match the reference model for key {key}"
        );
        assert_eq!(
            harness
                .table
                .group_node_records_at(group.index)
                .first()
                .expect("model child node must exist")
                .id,
            expected.node_id,
            "node identity must match the reference model for key {key}"
        );
    }

    let active_anchors = snapshot
        .active_groups
        .iter()
        .map(|group| group.anchor)
        .collect::<HashSet<_>>();
    assert_eq!(
        active_anchors.len(),
        snapshot.active_groups.len(),
        "generated model operations must not create duplicate active anchors"
    );

    let retained_keys = retained_subtrees.keys().copied().collect::<HashSet<_>>();
    let active_keys = model.active.keys().copied().collect::<HashSet<_>>();
    assert!(
        active_keys.is_disjoint(&retained_keys),
        "active groups and retained groups must stay disjoint"
    );
    assert_eq!(retained_keys.len(), model.retained.len());

    for (&key, subtree) in retained_subtrees {
        let expected = model.retained.get(&key).expect("retained model child");
        let root_group = subtree
            .groups
            .first()
            .expect("retained subtree must contain a root group");
        assert_eq!(root_group.key.explicit_key, Some(key));
        assert_eq!(
            subtree.root_parent_anchor(),
            AnchorId::INVALID,
            "retained subtree roots must detach from the active parent chain"
        );
        for anchor in subtree.group_anchors() {
            assert!(
                !active_anchors.contains(&anchor),
                "retained subtree anchors must not remain active: key={key} anchor={anchor:?}"
            );
        }
        let payload = detached_group_payloads(subtree, 0)
            .first()
            .expect("retained subtree payload must exist")
            .value
            .downcast_ref::<i32>()
            .expect("retained subtree payload must be i32");
        assert_eq!(
            *payload, expected.value,
            "retained values must match the reference model for key {key}"
        );
        assert_eq!(
            detached_group_nodes(subtree, 0)
                .first()
                .expect("retained subtree node must exist")
                .id,
            expected.node_id,
            "retained node identity must match the reference model for key {key}"
        );
        for group in &subtree.groups {
            assert!(
                !active_anchors.contains(&group.anchor),
                "active and retained storage must stay disjoint"
            );
        }
    }

    assert_eq!(harness.table.validate(), Ok(()));
}

fn apply_model_operation(
    harness: &mut SlotHarness,
    retained_subtrees: &mut BTreeMap<Key, DetachedSubtree>,
    model: &mut ModelState,
    operation: ModelOperation,
    parent_key: Key,
    child_static_key: Key,
) {
    match operation {
        ModelOperation::Compose {
            order,
            retain_detached,
            mutate_values,
        } => {
            let previous_active = model.active.clone();
            let previous_retained = model.retained.clone();
            let mut next_retained = previous_retained.clone();
            let mut next_active = BTreeMap::<Key, ModelChild>::new();
            let mut next_node_id = model.next_node_id;
            let mut carried_retained = std::mem::take(retained_subtrees);

            harness.begin_pass(SlotPassMode::Compose);
            let detached_children = harness.session(SlotPassMode::Compose, |session| {
                begin_unkeyed(session, parent_key, None);

                for key in order.iter().copied() {
                    let restored = carried_retained.remove(&key);
                    let started = begin_keyed(session, child_static_key, key, restored);
                    let existing_active = previous_active.get(&key).cloned();
                    let existing_retained = previous_retained.get(&key).cloned();

                    match (existing_active.is_some(), existing_retained.is_some()) {
                        (_, true) => assert_eq!(
                            started.kind,
                            GroupStartKind::Restored,
                            "retained children must restore instead of inserting or reusing"
                        ),
                        (true, false) => assert!(
                            matches!(started.kind, GroupStartKind::Reused | GroupStartKind::Moved),
                            "active keyed children must reuse or move; got {:?} for key {key}",
                            started.kind
                        ),
                        (false, false) => assert_eq!(
                            started.kind,
                            GroupStartKind::Inserted,
                            "new keyed children must insert"
                        ),
                    }

                    let mut child =
                        existing_active
                            .or(existing_retained)
                            .unwrap_or_else(|| ModelChild {
                                value: (key as i32) * 10,
                                node_id: {
                                    let node_id = next_node_id;
                                    next_node_id += 1;
                                    node_id
                                },
                                scope_id: ModelState::scope_id_for(key),
                            });

                    next_retained.remove(&key);
                    session.set_group_scope(started.group, child.scope_id);
                    let slot = session.value_slot(|| (key as i32) * 10);
                    session.record_node(child.node_id, 1);
                    if mutate_values.contains(&key) {
                        child.value += 1;
                        session.table.write_value(slot, child.value);
                    }

                    let child_result = session.finish_group_body();
                    assert!(child_result.detached_children.is_empty());
                    session.end_group();
                    next_active.insert(key, child);
                }

                let parent_result = session.finish_group_body();
                session.end_group();
                parent_result.detached_children
            });
            harness.finish_pass(SlotPassMode::Compose);

            for subtree in detached_children {
                let key = subtree
                    .root_key()
                    .explicit_key
                    .expect("generated model test uses explicit child keys");
                let expected_child = previous_active
                    .get(&key)
                    .cloned()
                    .expect("detached child must have been active previously");
                if retain_detached.contains(&key) {
                    next_retained.insert(key, expected_child);
                    carried_retained.insert(key, subtree);
                }
            }

            model.active_order = order;
            model.active = next_active;
            model.retained = next_retained;
            model.next_node_id = next_node_id;
            *retained_subtrees = carried_retained;
        }
        ModelOperation::RecomposeSkip { key } => {
            let before = harness.table.debug_snapshot();
            harness.begin_pass(SlotPassMode::Recompose);
            let group_index = harness.session(SlotPassMode::Recompose, |session| {
                let group = session
                    .begin_recompose_at_scope(ModelState::scope_id_for(key))
                    .expect("active scopes must resolve through the scope index");
                session.skip_group();
                let result = session.finish_group_body();
                assert!(result.detached_children.is_empty());
                session.end_recompose();
                group.index()
            });
            harness.finish_pass(SlotPassMode::Recompose);
            assert_eq!(
                harness.table.groups[group_index].key.explicit_key,
                Some(key),
                "recompose must target the expected keyed child",
            );
            assert_eq!(
                harness.table.debug_snapshot(),
                before,
                "skipping a recomposed keyed group must leave the active tree unchanged",
            );
        }
    }

    assert_model_matches_slot_table(
        harness,
        retained_subtrees,
        model,
        parent_key,
        child_static_key,
    );
}

#[test]
fn generated_model_operations_match_slot_table() {
    const PARENT_KEY: Key = 700;
    const STATIC_KEY: Key = 701;
    const CHILDREN: [Key; 4] = [1, 2, 3, 4];

    let mut harness = SlotHarness::new();
    let mut retained_subtrees = BTreeMap::<Key, DetachedSubtree>::new();
    let mut model = ModelState::default();
    let mut seed = 0x9E37_79B9_7F4A_7C15_u64;

    for _ in 0..96 {
        let operation = generate_model_operation(&mut seed, &model, &CHILDREN);
        apply_model_operation(
            &mut harness,
            &mut retained_subtrees,
            &mut model,
            operation,
            PARENT_KEY,
            STATIC_KEY,
        );
    }
}
