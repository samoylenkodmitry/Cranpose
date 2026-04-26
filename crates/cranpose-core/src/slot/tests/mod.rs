use super::{
    AnchorState, DetachedSubtree, GroupRecord, NodeLifecycle, NodeRecord, PayloadKind,
    PayloadRecord, SlotDebugEntryKind, SlotInvariantError, SlotLifecycleCoordinator, SlotPassMode,
    SlotTable, SlotWriteSession, SlotWriteSessionState,
};
use crate::{
    retention::{RetainKey, RetentionManager},
    slot_storage::{GroupId, GroupKey, GroupKeySeed, GroupStart, GroupStartKind, ValueSlotId},
    AnchorId, Applier, BeginGroupInput, Key, MemoryApplier, Node, NodeId, ScopeId,
};
use std::any::{Any, TypeId};
use std::cell::Cell;
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::mem;
use std::panic::{self, AssertUnwindSafe};
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
        self.state.reset_for_pass(mode);
    }

    fn session<R>(&mut self, f: impl FnOnce(&mut SlotWriteSession<'_>) -> R) -> R {
        let mut session = self
            .table
            .write_session(&mut self.lifecycle, &mut self.state);
        let result = f(&mut session);
        self.state
            .validate(&self.table)
            .expect("slot writer state must stay within active table bounds");
        result
    }

    fn finish_pass(&mut self) {
        let detached_root_children = {
            let mut session = self
                .table
                .write_session(&mut self.lifecycle, &mut self.state);
            session.finalize_pass(&mut self.applier)
        };
        for subtree in detached_root_children {
            self.table.invalidate_detached_subtree_anchors(&subtree);
            self.lifecycle.queue_subtree_disposal(subtree);
        }
        self.lifecycle.flush_pending_drops();
        self.table.debug_verify();
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
    harness.session(|session| {
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
    harness.finish_pass();
    harness.table
}

fn detached_single_child_with_options(
    parent_key: Key,
    child_key: Key,
    child_scope: Option<ScopeId>,
    record_child_payload: bool,
    record_child_node: bool,
) -> (SlotHarness, DetachedSubtree, Option<NodeId>) {
    let mut harness = SlotHarness::new();
    let child_node = record_child_node.then(|| {
        harness
            .applier
            .create(Box::new(UnmountTrackingNode::new(Rc::new(Cell::new(0)))))
    });
    let child_generation = child_node.map(|id| harness.applier.node_generation(id));

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, parent_key, None);

        let child = begin_unkeyed(session, child_key, None);
        if let Some(scope_id) = child_scope {
            session.set_group_scope(child.group, scope_id);
        }
        if record_child_payload {
            let _ = session.value_slot(|| 17_i32);
        }
        if let (Some(node_id), Some(generation)) = (child_node, child_generation) {
            session.record_node(node_id, generation);
        }
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();
        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(|session| {
        begin_unkeyed(session, parent_key, None);
        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass();
    (harness, detached, child_node)
}

fn detached_single_child(parent_key: Key, child_key: Key) -> (SlotHarness, DetachedSubtree) {
    let (harness, detached, _) =
        detached_single_child_with_options(parent_key, child_key, None, false, false);
    (harness, detached)
}

fn detached_child_with_grandchild(
    parent_key: Key,
    child_key: Key,
    grandchild_key: Key,
) -> (SlotHarness, DetachedSubtree) {
    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, parent_key, None);
        begin_unkeyed(session, child_key, None);
        begin_unkeyed(session, grandchild_key, None);
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
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(|session| {
        begin_unkeyed(session, parent_key, None);
        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass();
    (harness, detached)
}

fn composed_group_with_value_and_node_table(group_key: Key) -> SlotTable {
    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, group_key, None);
        let _ = session.value_slot(|| 17_i32);
        session.record_node(31, 1);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();
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

fn exercise_slot_write_session_surface(
    slots: &mut SlotWriteSession<'_>,
    group_key: GroupKey,
    scope_id: ScopeId,
) -> (GroupId, ValueSlotId) {
    let started = slots.begin_group(BeginGroupInput::new(group_key, None));
    assert_eq!(started.kind, GroupStartKind::Inserted);
    slots.set_group_scope(started.group, scope_id);

    let slot = slots.value_slot(|| 7_i32);

    let recorded = slots.record_node(55, 1);
    assert!(!recorded.reused);
    assert_eq!(recorded.id, 55);
    assert_eq!(slots.nodes_in_current_group(), vec![55]);

    let result = slots.finish_group_body();
    assert!(result.detached_children.is_empty());
    slots.end_group();
    (started.group, slot)
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

mod basic;
mod detach_restore;
mod keyed_reorder;
mod model;
mod nodes;
mod payloads;
mod retention;
mod validation;
mod writer_state;
