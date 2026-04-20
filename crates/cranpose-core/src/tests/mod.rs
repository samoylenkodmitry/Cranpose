use super::*;
use crate as cranpose_core;
use crate::snapshot_v2::take_mutable_snapshot;
#[cfg(test)]
use crate::snapshot_v2::{reset_runtime_for_tests, TestRuntimeGuard};
use crate::state::{MutationPolicy, SnapshotMutableState};
use crate::SnapshotStateObserver;
use cranpose_macros::composable;
use smallvec::SmallVec;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Reset the snapshot runtime for tests to ensure clean state.
/// This matches the pattern used in integration_tests.rs.
#[cfg(test)]
fn reset_snapshot_runtime() -> TestRuntimeGuard {
    crate::snapshot_pinning::reset_pinning_table();
    reset_runtime_for_tests()
}

#[derive(Default)]
struct TestTextNode {
    text: String,
}

impl Node for TestTextNode {}

#[derive(Default)]
struct TestDummyNode;

impl Node for TestDummyNode {}

#[derive(Default)]
struct RehousingDummyNode {
    marker: usize,
}

impl Node for RehousingDummyNode {
    fn rehouse_for_live_compaction(&mut self) -> Option<Box<dyn Node>> {
        let live = std::mem::take(self);
        Some(Box::new(live))
    }
}

struct MountTrackingNode {
    mounted: Rc<Cell<usize>>,
}

impl Node for MountTrackingNode {
    fn mount(&mut self) {
        self.mounted.set(self.mounted.get() + 1);
    }
}

fn runtime_handle() -> (RuntimeHandle, Runtime) {
    let runtime = Runtime::new(Arc::new(TestScheduler));
    (runtime.handle(), runtime)
}

pub(crate) fn test_applier() -> MemoryApplier {
    MemoryApplier::new()
}

pub(crate) fn test_composition() -> Composition<MemoryApplier> {
    Composition::new(test_applier())
}

pub(crate) fn test_slot_table() -> SlotTable {
    with_test_slot_lifecycle(|lifecycle| {
        *lifecycle = crate::slot_table::SlotLifecycleCoordinator::default()
    });
    SlotTable::new()
}

pub(crate) fn test_slot_session() -> crate::slot_table::SlotWriteSessionState {
    crate::slot_table::SlotWriteSessionState::default()
}

thread_local! {
    static TEST_SLOT_LIFECYCLE: RefCell<crate::slot_table::SlotLifecycleCoordinator> =
        RefCell::new(crate::slot_table::SlotLifecycleCoordinator::default());
}

fn with_test_slot_lifecycle<R>(
    f: impl FnOnce(&mut crate::slot_table::SlotLifecycleCoordinator) -> R,
) -> R {
    TEST_SLOT_LIFECYCLE.with(|slot| f(&mut slot.borrow_mut()))
}

pub(crate) fn begin_test_group(
    slots: &mut SlotTable,
    state: &mut crate::slot_table::SlotWriteSessionState,
    key: Key,
) -> GroupId {
    crate::slot_table::begin_group_for_test(slots, state, key)
}

pub(crate) fn remember_test_value<T: 'static>(
    slots: &mut SlotTable,
    state: &mut crate::slot_table::SlotWriteSessionState,
    init: impl FnOnce() -> T,
) -> Owned<T> {
    with_test_slot_lifecycle(|lifecycle| {
        slots
            .write_session(lifecycle, state, crate::slot_table::SlotPassMode::Compose)
            .remember(init)
    })
}

pub(crate) fn end_test_group(
    slots: &mut SlotTable,
    state: &mut crate::slot_table::SlotWriteSessionState,
) {
    with_test_slot_lifecycle(|lifecycle| {
        slots
            .write_session(lifecycle, state, crate::slot_table::SlotPassMode::Compose)
            .end_group();
    });
}

thread_local! {
    static INVOCATIONS: Cell<usize> = const { Cell::new(0) };
}

thread_local! {
    static PARENT_RECOMPOSITIONS: Cell<usize> = const { Cell::new(0) };
    static CHILD_RECOMPOSITIONS: Cell<usize> = const { Cell::new(0) };
    static CAPTURED_PARENT_STATE: RefCell<Option<cranpose_core::MutableState<i32>>> =
        const { RefCell::new(None) };
    static SIDE_EFFECT_LOG: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    static DISPOSABLE_EFFECT_LOG: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    static DISPOSABLE_STATE: RefCell<Option<cranpose_core::MutableState<i32>>> =
        const { RefCell::new(None) };
    static SIDE_EFFECT_STATE: RefCell<Option<cranpose_core::MutableState<i32>>> =
        const { RefCell::new(None) };
}

thread_local! {
    static DROP_REENTRY_STATE: RefCell<Option<cranpose_core::MutableState<ReentrantDropState>>> =
        const { RefCell::new(None) };
    static DROP_REENTRY_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static DROP_REENTRY_LAST_VALUE: Cell<Option<usize>> = const { Cell::new(None) };
    static STABLE_OUTER_PARENT_ID: Cell<Option<NodeId>> = const { Cell::new(None) };
}

struct ReentrantDropState {
    id: usize,
    drops: Rc<Cell<usize>>,
    reenter_on_drop: Rc<Cell<bool>>,
}

impl ReentrantDropState {
    fn new(id: usize, drops: Rc<Cell<usize>>, reenter_on_drop: bool) -> Self {
        Self {
            id,
            drops,
            reenter_on_drop: Rc::new(Cell::new(reenter_on_drop)),
        }
    }
}

impl Clone for ReentrantDropState {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            drops: Rc::clone(&self.drops),
            reenter_on_drop: Rc::new(Cell::new(false)),
        }
    }
}

impl Drop for ReentrantDropState {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
        if !self.reenter_on_drop.replace(false) {
            return;
        }

        DROP_REENTRY_ACTIVE.with(|active| {
            if active.replace(true) {
                return;
            }

            DROP_REENTRY_STATE.with(|slot| {
                if let Some(state) = slot.borrow().as_ref() {
                    let value = state.value();
                    DROP_REENTRY_LAST_VALUE.with(|last| last.set(Some(value.id)));
                }
            });

            active.set(false);
        });
    }
}

fn cranpose_test_node<N: Node + 'static>(init: impl FnOnce() -> N) -> NodeId {
    cranpose_core::with_current_composer(|composer| composer.emit_node(init))
}

fn setup_composer(
    slots: &mut SlotTable,
    applier: &mut MemoryApplier,
    handle: RuntimeHandle,
    root: Option<NodeId>,
) -> (
    Composer,
    Rc<SlotsHost>,
    Rc<ConcreteApplierHost<MemoryApplier>>,
) {
    let slots_host = Rc::new(SlotsHost::new(std::mem::take(slots)));
    let applier_host = Rc::new(ConcreteApplierHost::new(std::mem::replace(
        applier,
        MemoryApplier::new(),
    )));
    let observer = SnapshotStateObserver::new(|callback| callback());
    let composer = Composer::new(
        Rc::clone(&slots_host),
        applier_host.clone(),
        handle,
        observer,
        root,
    );
    (composer, slots_host, applier_host)
}

fn teardown_composer(
    slots: &mut SlotTable,
    applier: &mut MemoryApplier,
    slots_host: Rc<SlotsHost>,
    applier_host: Rc<ConcreteApplierHost<MemoryApplier>>,
) {
    *slots = Rc::try_unwrap(slots_host)
        .unwrap_or_else(|_| panic!("slots host still has outstanding references"))
        .take();
    *applier = Rc::try_unwrap(applier_host)
        .unwrap_or_else(|_| panic!("applier host still has outstanding references"))
        .into_inner();
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Operation {
    Insert(NodeId),
    Remove(NodeId),
    Move { from: usize, to: usize },
}

#[derive(Default)]
struct RecordingNode {
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    operations: Vec<Operation>,
}

impl Node for RecordingNode {
    fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    fn on_attached_to_parent(&mut self, parent: NodeId) {
        self.parent = Some(parent);
    }

    fn on_removed_from_parent(&mut self) {
        self.parent = None;
    }

    fn children(&self) -> Vec<NodeId> {
        self.children.clone()
    }

    fn insert_child(&mut self, child: NodeId) {
        self.children.push(child);
        self.operations.push(Operation::Insert(child));
    }

    fn remove_child(&mut self, child: NodeId) {
        self.children.retain(|&c| c != child);
        self.operations.push(Operation::Remove(child));
    }

    fn move_child(&mut self, from: usize, to: usize) {
        if from == to || from >= self.children.len() {
            return;
        }
        let child = self.children.remove(from);
        let target = to.min(self.children.len());
        if target >= self.children.len() {
            self.children.push(child);
        } else {
            self.children.insert(target, child);
        }
        self.operations.push(Operation::Move { from, to });
    }
}

#[derive(Default)]
struct TrackingChild {
    label: String,
    mount_count: usize,
    parent: Option<NodeId>,
}

impl Node for TrackingChild {
    fn mount(&mut self) {
        self.mount_count += 1;
    }

    fn on_attached_to_parent(&mut self, parent: NodeId) {
        self.parent = Some(parent);
    }

    fn on_removed_from_parent(&mut self) {
        self.parent = None;
    }

    fn parent(&self) -> Option<NodeId> {
        self.parent
    }
}

struct UnmountTrackingNode {
    children: Vec<NodeId>,
    parent: Option<NodeId>,
    unmounts: Rc<Cell<usize>>,
}

impl UnmountTrackingNode {
    fn new(unmounts: Rc<Cell<usize>>) -> Self {
        Self {
            children: Vec::new(),
            parent: None,
            unmounts,
        }
    }
}

impl Node for UnmountTrackingNode {
    fn unmount(&mut self) {
        self.unmounts.set(self.unmounts.get() + 1);
    }

    fn insert_child(&mut self, child: NodeId) {
        self.children.push(child);
    }

    fn remove_child(&mut self, child: NodeId) {
        self.children.retain(|&id| id != child);
    }

    fn children(&self) -> Vec<NodeId> {
        self.children.clone()
    }

    fn on_attached_to_parent(&mut self, parent: NodeId) {
        self.parent = Some(parent);
    }

    fn on_removed_from_parent(&mut self) {
        self.parent = None;
    }

    fn parent(&self) -> Option<NodeId> {
        self.parent
    }
}

#[composable]
fn counted_text(value: i32) -> NodeId {
    INVOCATIONS.with(|calls| calls.set(calls.get() + 1));
    let id = cranpose_test_node(TestTextNode::default);
    with_node_mut(id, |node: &mut TestTextNode| {
        node.text = format!("{}", value);
    })
    .expect("update text node");
    id
}

#[composable]
fn child_reads_state(state: cranpose_core::State<i32>) -> NodeId {
    CHILD_RECOMPOSITIONS.with(|calls| calls.set(calls.get() + 1));
    counted_text(state.value())
}

#[composable]
fn parent_passes_state() -> NodeId {
    PARENT_RECOMPOSITIONS.with(|calls| calls.set(calls.get() + 1));
    let state = cranpose_core::useState(|| 0);
    CAPTURED_PARENT_STATE.with(|slot| {
        if slot.borrow().is_none() {
            *slot.borrow_mut() = Some(state);
        }
    });
    child_reads_state(state.as_state())
}

#[composable]
fn side_effect_component() -> NodeId {
    SIDE_EFFECT_LOG.with(|log| log.borrow_mut().push("compose"));
    let state = cranpose_core::useState(|| 0);
    let _ = state.value();
    SIDE_EFFECT_STATE.with(|slot| {
        if slot.borrow().is_none() {
            *slot.borrow_mut() = Some(state);
        }
    });
    cranpose_core::SideEffect(|| {
        SIDE_EFFECT_LOG.with(|log| log.borrow_mut().push("effect"));
    });
    cranpose_test_node(TestTextNode::default)
}

#[composable]
fn disposable_effect_host() -> NodeId {
    let state = cranpose_core::useState(|| 0);
    DISPOSABLE_STATE.with(|slot| *slot.borrow_mut() = Some(state));
    DisposableEffect!(state.value(), |scope| {
        DISPOSABLE_EFFECT_LOG.with(|log| log.borrow_mut().push("start"));
        scope.on_dispose(|| {
            DISPOSABLE_EFFECT_LOG.with(|log| log.borrow_mut().push("dispose"));
        })
    });
    cranpose_test_node(TestTextNode::default)
}

mod composer_applier_tests;
mod composition_and_recompose_scope_tests;
mod recompose_and_diff_tests;
mod snapshot_state_tests;
mod state_and_effect_tests;
