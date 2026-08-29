use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use cranpose_macros::composable;
use smallvec::SmallVec;

use super::*;
use crate as cranpose_core;
#[cfg(test)]
use crate::snapshot_v2::{TestRuntimeGuard, reset_runtime_for_tests};
use crate::{
    SnapshotStateObserver,
    slot::ActiveGroupId,
    snapshot_v2::take_mutable_snapshot,
    state::{MutationPolicy, SnapshotMutableState},
};

#[cfg(test)]
fn reset_snapshot_runtime() -> TestRuntimeGuard {
    crate::snapshot_pinning::reset_pinning_table();
    reset_runtime_for_tests()
}

#[test]
fn debug_scope_env_flag_is_not_process_cached() {
    let source = include_str!("../debug_trace.rs");
    let once_lock = ["Once", "Lock"].concat();
    let tracking_static = ["static ", "DEBUG_SCOPE_TRACKING"].concat();

    assert!(
        !source.contains(&once_lock) && !source.contains(&tracking_static),
        "debug scope env flag must not be latched in a process-global cache"
    );
}

#[test]
fn state_chain_assert_env_flag_is_not_process_cached() {
    let source = include_str!("../state.rs");
    let once_lock = ["Once", "Lock"].concat();
    let check_static = ["static ", "CHECK"].concat();

    assert!(
        !source.contains(&once_lock) && !source.contains(&check_static),
        "state chain assertion env flag must not be latched in a process-global cache"
    );
}

#[test]
fn core_diagnostic_env_flags_are_not_process_cached() {
    let source = include_str!("../lib.rs");

    assert!(
        !source.contains("OnceLock<bool>") && !source.contains("get_or_init(|| std::env::var_os"),
        "core diagnostic env flags must be read at the diagnostic boundary"
    );
}

#[test]
fn location_key_debug_state_is_not_process_global() {
    let source = include_str!("../lib.rs");
    assert!(!source.contains("location_key_registry() -> &'static"));
    assert!(!source.contains("location_key_collision_count() -> &'static"));
}

#[test]
fn composition_local_keys_do_not_use_process_global_counter() {
    let lib_source = include_str!("../lib.rs");
    let local_source = include_str!("../composition_locals.rs");

    assert!(!lib_source.contains("NEXT_LOCAL_KEY"));
    assert!(!lib_source.contains("next_local_key"));
    assert!(!local_source.contains("next_local_key"));
    assert!(
        lib_source.contains("struct LocalKey(Rc<()>)"),
        "a local's identity is the allocation its clones share, never a \
         process-global counter"
    );
}

#[test]
fn composition_local_key_identity_is_retained_per_local() {
    let first = compositionLocalOf(|| 1_i32);
    let first_clone = first.clone();
    let second = compositionLocalOf(|| 1_i32);
    let static_first = staticCompositionLocalOf(|| 1_i32);
    let static_first_clone = static_first.clone();
    let static_second = staticCompositionLocalOf(|| 1_i32);

    assert_eq!(first.key, first_clone.key);
    assert_ne!(first.key, second.key);
    assert_eq!(static_first.key, static_first_clone.key);
    assert_ne!(static_first.key, static_second.key);
}

#[test]
fn recompose_scope_live_count_is_runtime_owned() {
    let lib_source = include_str!("../lib.rs");
    let debug_source = include_str!("../debug_trace.rs");

    assert!(!lib_source.contains("LIVE_RECOMPOSE_SCOPE_COUNT"));
    assert!(!debug_source.contains("LIVE_RECOMPOSE_SCOPE_COUNT"));
}

#[test]
fn recompose_scope_live_count_tracks_runtime_owned_scopes() {
    let before = debug_live_recompose_scope_count();
    let runtime = runtime::TestRuntime::new();
    let scope = RecomposeScope::new(runtime.handle());

    assert_eq!(debug_live_recompose_scope_count(), before + 1);

    drop(scope);
    assert_eq!(debug_live_recompose_scope_count(), before);
}

#[test]
fn runtime_ids_do_not_use_process_global_counter() {
    let source = include_str!("../runtime.rs");
    assert!(!source.contains("AtomicU32"));
    assert!(!source.contains("fetch_add(1, Ordering::Relaxed)"));
}

#[test]
fn recompose_scope_ids_do_not_use_process_global_counter() {
    let source = include_str!("../lib.rs");
    assert!(!source.contains("NEXT_SCOPE_ID"));
    assert!(!source.contains("next_scope_id"));
}

#[test]
fn recompose_scope_id_is_retained_scope_identity() {
    let runtime = runtime::TestRuntime::new();
    let first = RecomposeScope::new(runtime.handle());
    let first_clone = first.clone();
    let second = RecomposeScope::new(runtime.handle());

    assert_eq!(first.id(), first_clone.id());
    assert_ne!(first.id(), second.id());
}

#[test]
fn location_key_debug_registry_reports_collisions_without_panicking() {
    let forced_key = Key::MAX - 17;
    let collision_count_before = location_key_debug_collision_count_for_test();
    register_location_key_debug_info_for_test(forced_key, "forced-collision-first.rs", 10, 20);

    let result = std::panic::catch_unwind(|| {
        register_location_key_debug_info_for_test(forced_key, "forced-collision-second.rs", 30, 40);
    });
    assert!(
        result.is_ok(),
        "debug collision tracking must report different call sites with the same location key without panicking",
    );
    assert_eq!(
        location_key_debug_collision_count_for_test(),
        collision_count_before + 1
    );

    let registered = location_key_debug_info_for_test(forced_key)
        .expect("first debug location should remain registered");
    assert_eq!(registered.file, "forced-collision-first.rs");
    assert_eq!(registered.line, 10);
    assert_eq!(registered.column, 20);
}

#[test]
fn location_key_hashes_file_contents_not_string_addresses() {
    let static_file = "src/example.rs";
    let owned_file = String::from(static_file);

    assert_eq!(
        location_key(static_file, 42, 9),
        location_key(&owned_file, 42, 9),
        "source-location keys must depend on file contents, not the string allocation address",
    );
    assert_ne!(
        location_key(static_file, 42, 9),
        location_key(static_file, 43, 9)
    );
    assert_ne!(
        location_key(static_file, 42, 9),
        location_key(static_file, 42, 10)
    );
    assert_ne!(
        location_key(static_file, 42, 9),
        location_key("src/other.rs", 42, 9)
    );
}

#[test]
fn location_key_generated_source_grid_has_no_collisions() {
    let mut keys = std::collections::HashSet::new();
    for file_index in 0..64 {
        let file = format!("src/generated/module_{file_index}.rs");
        for line in 1..=64 {
            for column in [1, 8, 16, 32] {
                let key = location_key(&file, line, column);
                assert!(
                    keys.insert(key),
                    "generated source-location grid produced a duplicate key: file={file} line={line} column={column} key={key}",
                );
            }
        }
    }
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

pub(crate) fn assert_composition_valid(composition: &Composition<MemoryApplier>) {
    composition
        .debug_validate_slots()
        .expect("slot table must validate");
}

pub(crate) fn test_slot_table() -> SlotTable {
    with_test_slot_lifecycle(|lifecycle| {
        *lifecycle = crate::slot::SlotLifecycleCoordinator::default()
    });
    SlotTable::new()
}

pub(crate) fn test_slot_session() -> crate::slot::SlotWriteSessionState {
    crate::slot::SlotWriteSessionState::default()
}

thread_local! {
    static TEST_SLOT_LIFECYCLE: RefCell<crate::slot::SlotLifecycleCoordinator> =
        RefCell::new(crate::slot::SlotLifecycleCoordinator::default());
}

fn with_test_slot_lifecycle<R>(
    f: impl FnOnce(&mut crate::slot::SlotLifecycleCoordinator) -> R,
) -> R {
    TEST_SLOT_LIFECYCLE.with(|slot| f(&mut slot.borrow_mut()))
}

pub(crate) fn begin_test_group(
    slots: &mut SlotTable,
    state: &mut crate::slot::SlotWriteSessionState,
    key: Key,
) -> ActiveGroupId {
    with_test_slot_lifecycle(|lifecycle| {
        let mut session = slots.write_session(lifecycle, state);
        let group_key = session.preview_group_key(crate::slot::GroupKeySeed::unkeyed(key));
        session.begin_group(group_key, None).group
    })
}

pub(crate) fn remember_test_value<T: 'static>(
    slots: &mut SlotTable,
    state: &mut crate::slot::SlotWriteSessionState,
    init: impl FnOnce() -> T,
) -> Owned<T> {
    with_test_slot_lifecycle(|lifecycle| {
        slots
            .write_session(lifecycle, state)
            .remember(crate::slot::BRANCH_PATH_ROOT, init)
    })
}

pub(crate) fn end_test_group(
    slots: &mut SlotTable,
    state: &mut crate::slot::SlotWriteSessionState,
) {
    with_test_slot_lifecycle(|lifecycle| {
        slots.write_session(lifecycle, state).end_group();
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
    *slots = slots_host.into_table().expect("restore composer slots");
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

    fn insert_child(&mut self, child: NodeId) -> bool {
        if self.children.contains(&child) {
            return false;
        }
        self.children.push(child);
        self.operations.push(Operation::Insert(child));
        true
    }

    fn remove_child(&mut self, child: NodeId) -> bool {
        let before = self.children.len();
        self.children.retain(|&c| c != child);
        let removed = self.children.len() < before;
        if removed {
            self.operations.push(Operation::Remove(child));
        }
        removed
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

    fn insert_child(&mut self, child: NodeId) -> bool {
        if self.children.contains(&child) {
            return false;
        }
        self.children.push(child);
        true
    }

    fn remove_child(&mut self, child: NodeId) -> bool {
        let before = self.children.len();
        self.children.retain(|&id| id != child);
        self.children.len() < before
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
    let state = cranpose_core::rememberMutableStateOf(|| 0);
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
    let state = cranpose_core::rememberMutableStateOf(|| 0);
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
    let state = cranpose_core::rememberMutableStateOf(|| 0);
    DISPOSABLE_STATE.with(|slot| *slot.borrow_mut() = Some(state));
    DisposableEffect!(state.value(), |scope| {
        DISPOSABLE_EFFECT_LOG.with(|log| log.borrow_mut().push("start"));
        scope.on_dispose(|| {
            DISPOSABLE_EFFECT_LOG.with(|log| log.borrow_mut().push("dispose"));
        })
    });
    cranpose_test_node(TestTextNode::default)
}

#[test]
fn exact_subcompose_activation_does_not_take_compatible_cross_slot_nodes() {
    let mut state = SubcomposeState::new(Box::new(ContentTypeReusePolicy::new()));
    let exact_slot = SlotId::new(1);
    let compatible_slot = SlotId::new(2);
    state.register_content_type(exact_slot, 7);
    state.register_content_type(compatible_slot, 7);
    state.register_active(exact_slot, &[10], &[]);
    state.recycle_prefetched_active_slot(exact_slot);

    assert!(
        state
            .take_exact_slot_for_activation(compatible_slot)
            .is_none(),
        "exact activation must not consume a compatible cross-slot reusable node"
    );
    assert_eq!(state.reusable(), &[10]);

    let (nodes, scopes) = state
        .take_exact_slot_for_activation(exact_slot)
        .expect("exact retained slot should activate");
    assert_eq!(nodes, vec![10]);
    assert!(scopes.is_empty());
    assert!(state.reusable().is_empty());
    assert_eq!(state.reusable_count, 0);
}

#[test]
fn exact_subcompose_activation_rejects_invalidated_slot_scopes() {
    let runtime = runtime::TestRuntime::new();
    let scope = RecomposeScope::new_for_test(runtime.handle());
    let mut state = SubcomposeState::default();
    let slot = SlotId::new(1);
    state.register_active(slot, &[10], std::slice::from_ref(&scope));
    state.dispose_or_reuse_starting_from_index(0);

    scope.invalidate();

    assert!(
        state.take_exact_slot_for_activation(slot).is_none(),
        "invalidated item scopes must force subcomposition instead of exact activation"
    );
    assert_eq!(state.reusable(), &[10]);
}

mod branch_group_tests;
mod composer_applier_tests;
mod composition_and_recompose_scope_tests;
mod internal_surface_tests;
mod recompose_and_diff_tests;
mod snapshot_state_tests;
mod state_and_effect_tests;
