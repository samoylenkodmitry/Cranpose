use crate::collections::map::{HashMap, HashSet};
use crate::retention::{RetainKey, RetentionManager};
use crate::slot::PayloadKind;
use crate::{
    composer_context, empty_local_stack, explicit_group_key_seed, runtime, Applier, ApplierHost,
    BeginGroupInput, ChildList, Command, CommandQueue, CompositionLocal, DirtyBubble, GroupId,
    GroupStart, GroupStartKind, Key, LocalKey, LocalStackSnapshot, LocalStateEntry, MutableState,
    Node, NodeError, NodeId, Owned, ProvidedValue, RecomposeOptions, RecomposeScope, RecycledNode,
    RetentionMode, RetentionPolicy, RuntimeHandle, ScopeId, SlotId, SlotPassOutcome, SlotTable,
    SlotsHost, SnapshotStateList, SnapshotStateMap, SnapshotStateObserver, StaticCompositionLocal,
    StaticLocalEntry, SubcomposeState, ValueSlotId, COMMAND_FLUSH_THRESHOLD,
};
use smallvec::SmallVec;
use std::any::Any;
use std::cell::{Cell, RefCell, RefMut};
use std::hash::Hash;
use std::marker::PhantomData;
use std::rc::Rc;

#[derive(Clone, Copy)]
pub struct ValueSlotHandle<'pass> {
    slot: ValueSlotId,
    _pass: PhantomData<&'pass Composer>,
}

impl ValueSlotHandle<'_> {
    pub(crate) fn new(slot: ValueSlotId) -> Self {
        Self {
            slot,
            _pass: PhantomData,
        }
    }

    pub(crate) fn slot(self) -> ValueSlotId {
        self.slot
    }
}

fn reserve_value_slot<T: 'static, S>(
    slots: &mut S,
    kind: PayloadKind,
    init: impl FnOnce() -> T,
) -> ValueSlotId
where
    S: crate::slot_storage::SlotStorage<Group = GroupId, ValueSlot = ValueSlotId>,
{
    slots.value_slot_with_kind(kind, init)
}

fn skip_slot_group<S>(slots: &mut S)
where
    S: crate::slot_storage::SlotStorage<Group = GroupId, ValueSlot = ValueSlotId>,
{
    slots.skip_group();
}

fn slots_storage_key(host: &Rc<SlotsHost>) -> usize {
    host.storage_key()
}

pub(crate) struct ComposerRuntimeState {
    scope_registry: RefCell<HashMap<ScopeId, RecomposeScope>>,
    retention_by_host: RefCell<HashMap<usize, RetentionManager>>,
    retention_policy: Cell<RetentionPolicy>,
    live_hosts: RefCell<HashMap<usize, std::rc::Weak<SlotsHost>>>,
    applier_host: RefCell<Option<std::rc::Weak<dyn ApplierHost>>>,
}

impl Default for ComposerRuntimeState {
    fn default() -> Self {
        Self {
            scope_registry: RefCell::new(HashMap::default()),
            retention_by_host: RefCell::new(HashMap::default()),
            retention_policy: Cell::new(RetentionPolicy::default()),
            live_hosts: RefCell::new(HashMap::default()),
            applier_host: RefCell::new(None),
        }
    }
}

impl ComposerRuntimeState {
    pub(crate) fn clear_host_storage_key(&self, host_key: usize) {
        self.retention_by_host.borrow_mut().remove(&host_key);
        self.live_hosts.borrow_mut().remove(&host_key);
        self.scope_registry
            .borrow_mut()
            .retain(|_, scope| scope.slots_storage_key() != Some(host_key));
    }

    pub(crate) fn bind_applier_host(&self, applier: &Rc<dyn ApplierHost>) {
        *self.applier_host.borrow_mut() = Some(Rc::downgrade(applier));
    }

    pub(crate) fn bind_slots_host(self: &Rc<Self>, host: &Rc<SlotsHost>) {
        host.bind_runtime_state(self);
        self.live_hosts
            .borrow_mut()
            .insert(host.storage_key(), Rc::downgrade(host));
    }

    pub(crate) fn scope_for_id(&self, scope_id: ScopeId) -> Option<RecomposeScope> {
        self.scope_registry.borrow().get(&scope_id).cloned()
    }

    pub(crate) fn register_scope(&self, scope: &RecomposeScope) {
        self.scope_registry
            .borrow_mut()
            .insert(scope.id(), scope.clone());
    }

    pub(crate) fn remove_scope(&self, scope_id: ScopeId) -> Option<RecomposeScope> {
        self.scope_registry.borrow_mut().remove(&scope_id)
    }

    pub(crate) fn set_retention_policy(&self, policy: RetentionPolicy) {
        self.retention_policy.set(policy);
    }

    pub(crate) fn retention_policy(&self) -> RetentionPolicy {
        self.retention_policy.get()
    }

    pub(crate) fn scope_registry_len(&self) -> usize {
        self.scope_registry.borrow().len()
    }

    pub(crate) fn take_retained(
        &self,
        host: &Rc<SlotsHost>,
        key: RetainKey,
    ) -> Option<crate::slot::DetachedSubtree> {
        let host_key = slots_storage_key(host);
        let mut retention = self.retention_by_host.borrow_mut();
        let subtree = retention.get_mut(&host_key)?.take(key);
        if retention
            .get(&host_key)
            .is_some_and(|manager| manager.is_empty() && manager.evictions_total() == 0)
        {
            retention.remove(&host_key);
        }
        subtree
    }

    pub(crate) fn insert_retained(
        &self,
        host: &Rc<SlotsHost>,
        key: RetainKey,
        subtree: crate::slot::DetachedSubtree,
    ) -> Vec<crate::slot::DetachedSubtree> {
        let policy = self.retention_policy();
        let mut retention_by_host = self.retention_by_host.borrow_mut();
        let manager = retention_by_host
            .entry(slots_storage_key(host))
            .or_insert_with(|| RetentionManager::new(policy));
        manager.set_policy(policy);
        manager.insert(key, subtree)
    }

    pub(crate) fn advance_retention_pass(
        &self,
        host: &Rc<SlotsHost>,
    ) -> Vec<crate::slot::DetachedSubtree> {
        let host_key = slots_storage_key(host);
        let policy = self.retention_policy();
        let mut retention_by_host = self.retention_by_host.borrow_mut();
        let Some(manager) = retention_by_host.get_mut(&host_key) else {
            return Vec::new();
        };
        manager.set_policy(policy);
        manager.advance_pass()
    }

    pub(crate) fn fill_slot_debug_snapshot(
        &self,
        host: &SlotsHost,
        snapshot: &mut crate::SlotDebugSnapshot,
    ) {
        let retention = self.retention_debug_stats(host.storage_key());
        snapshot.scope_registry_count = self.scope_registry_len();
        snapshot.retained_subtree_count = retention.subtree_count;
        snapshot.retained_group_count = retention.group_count;
        snapshot.retained_node_count = retention.node_count;
        snapshot.retained_scope_count = retention.scope_count;
    }

    pub(crate) fn fill_slot_debug_stats(
        &self,
        host: &SlotsHost,
        stats: &mut crate::SlotTableDebugStats,
    ) {
        let retention = self.retention_debug_stats(host.storage_key());
        stats.retained_subtree_count = retention.subtree_count;
        stats.retained_group_count = retention.group_count;
        stats.retained_payload_count = retention.payload_count;
        stats.retained_node_count = retention.node_count;
        stats.retained_scope_count = retention.scope_count;
        stats.retained_anchor_count = retention.anchor_count;
        stats.retained_heap_bytes = retention.heap_bytes;
        stats.retained_evictions_total = retention.evictions_total;
    }

    pub(crate) fn compact_table_namespaces_for_host(
        &self,
        host: &SlotsHost,
        table: &mut SlotTable,
    ) {
        let host_key = host.storage_key();
        let mut retention = self.retention_by_host.borrow_mut();
        if let Some(retained) = retention.get_mut(&host_key) {
            table.compact_anchor_namespace(Some(retained), |scope_id| self.scope_for_id(scope_id));
            table.compact_payload_anchor_namespace(Some(retained));
        } else {
            table.compact_anchor_namespace(None, |scope_id| self.scope_for_id(scope_id));
            table.compact_payload_anchor_namespace(None);
        }
    }

    pub(crate) fn clear_host(&self, host: &SlotsHost) {
        let host_key = host.storage_key();
        debug_assert!(
            self.host_retention_is_empty(host),
            "host retention must be drained before clearing host ownership"
        );
        self.clear_host_storage_key(host_key);
    }

    pub(crate) fn dispose_retained_subtrees_for_host(
        &self,
        host_key: usize,
        table: &mut SlotTable,
        lifecycle: &mut crate::slot::SlotLifecycleCoordinator,
    ) {
        let Some(retention) = self.retention_by_host.borrow_mut().remove(&host_key) else {
            return;
        };
        let applier_host = self
            .applier_host
            .borrow()
            .as_ref()
            .and_then(std::rc::Weak::upgrade);
        for subtree in retention.into_subtrees() {
            for scope_id in subtree.scope_ids() {
                if let Some(scope) = self.remove_scope(scope_id) {
                    scope.deactivate();
                    scope.set_group_anchor(crate::AnchorId::INVALID);
                }
            }
            if let Some(applier_host) = applier_host.as_ref() {
                let mut applier = applier_host.borrow_dyn();
                crate::slot::dispose_detached_subtree_now(&mut *applier, &subtree);
            }
            table.invalidate_detached_subtree_anchors(&subtree);
            lifecycle.queue_subtree_disposal(subtree);
        }
    }

    pub(crate) fn host_retention_is_empty(&self, host: &SlotsHost) -> bool {
        self.retention_by_host
            .borrow()
            .get(&host.storage_key())
            .is_none_or(RetentionManager::is_empty)
    }

    pub(crate) fn debug_verify_host(&self, host: &SlotsHost, table: &SlotTable) {
        if let Some(retention) = self.retention_by_host.borrow().get(&host.storage_key()) {
            retention.debug_verify(table);
        }
    }

    pub(crate) fn validate_host_retention(
        &self,
        host: &SlotsHost,
        table: &SlotTable,
    ) -> Result<(), crate::slot::SlotInvariantError> {
        if let Some(retention) = self.retention_by_host.borrow().get(&host.storage_key()) {
            retention.validate(table)?;
        }
        Ok(())
    }

    pub(crate) fn host_for_storage_key(&self, storage_key: usize) -> Option<Rc<SlotsHost>> {
        self.live_hosts
            .borrow()
            .get(&storage_key)
            .and_then(std::rc::Weak::upgrade)
    }

    fn retention_debug_stats(&self, host_key: usize) -> crate::retention::RetentionDebugStats {
        self.retention_by_host
            .borrow()
            .get(&host_key)
            .map(RetentionManager::debug_stats)
            .unwrap_or_default()
    }
}

pub(crate) struct ParentFrame {
    pub(crate) id: NodeId,
    pub(crate) previous: ChildList,
    pub(crate) new_children: ChildList,
    pub(crate) new_children_membership: Option<HashSet<NodeId>>,
    pub(crate) attach_mode: ParentAttachMode,
}

const LARGE_DEFERRED_CHILD_TRACKING_THRESHOLD: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParentAttachMode {
    ImmediateAppend,
    DeferredSync,
}

#[derive(Default)]
pub(crate) struct SubcomposeFrame {
    pub(crate) nodes: Vec<NodeId>,
    pub(crate) scopes: Vec<RecomposeScope>,
}

#[derive(Default, Clone)]
pub(crate) struct LocalContext {
    pub(crate) values: HashMap<LocalKey, Rc<dyn Any>>,
}

pub(crate) struct ComposerCore {
    pub(crate) shared_state: Rc<ComposerRuntimeState>,
    pub(crate) slots: Rc<SlotsHost>,
    slot_hosts: RefCell<Vec<Rc<SlotsHost>>>,
    pub(crate) applier: Rc<dyn ApplierHost>,
    pub(crate) runtime: RuntimeHandle,
    pub(crate) observer: SnapshotStateObserver,
    pub(crate) parent_stack: RefCell<Vec<ParentFrame>>,
    pub(crate) subcompose_stack: RefCell<Vec<SubcomposeFrame>>,
    pub(crate) root: Cell<Option<NodeId>>,
    pub(crate) commands: RefCell<CommandQueue>,
    pub(crate) scope_stack: RefCell<Vec<RecomposeScope>>,
    pub(crate) local_stack: RefCell<LocalStackSnapshot>,
    pub(crate) side_effects: RefCell<Vec<Box<dyn FnOnce()>>>,
    pub(crate) pending_scope_options: RefCell<Option<RecomposeOptions>>,
    pub(crate) phase: Cell<crate::Phase>,
    pub(crate) last_node_reused: Cell<Option<bool>>,
    pub(crate) recranpose_parent_hint: Cell<Option<NodeId>>,
    pub(crate) root_render_requested: Cell<bool>,
    pub(crate) _not_send: PhantomData<*const ()>,
}

impl ComposerCore {
    pub(crate) fn new(
        shared_state: Rc<ComposerRuntimeState>,
        slots: Rc<SlotsHost>,
        applier: Rc<dyn ApplierHost>,
        runtime: RuntimeHandle,
        observer: SnapshotStateObserver,
        root: Option<NodeId>,
    ) -> Self {
        let parent_stack = if let Some(root_id) = root {
            vec![ParentFrame {
                id: root_id,
                previous: ChildList::new(),
                new_children: ChildList::new(),
                new_children_membership: None,
                attach_mode: ParentAttachMode::DeferredSync,
            }]
        } else {
            Vec::new()
        };

        Self {
            shared_state,
            slots,
            slot_hosts: RefCell::new(Vec::new()),
            applier,
            runtime,
            observer,
            parent_stack: RefCell::new(parent_stack),
            subcompose_stack: RefCell::new(Vec::new()),
            root: Cell::new(root),
            commands: RefCell::new(CommandQueue::default()),
            scope_stack: RefCell::new(Vec::new()),
            local_stack: RefCell::new(empty_local_stack()),
            side_effects: RefCell::new(Vec::new()),
            pending_scope_options: RefCell::new(None),
            phase: Cell::new(crate::Phase::Compose),
            last_node_reused: Cell::new(None),
            recranpose_parent_hint: Cell::new(None),
            root_render_requested: Cell::new(false),
            _not_send: PhantomData,
        }
    }
}

#[derive(Clone)]
pub struct Composer {
    pub(crate) core: Rc<ComposerCore>,
}

pub(crate) enum EmittedNode {
    Fresh(Box<dyn Node>),
    Recycled(RecycledNode),
}

impl Composer {
    pub(crate) fn new_with_shared_state(
        shared_state: Rc<ComposerRuntimeState>,
        slots: Rc<SlotsHost>,
        applier: Rc<dyn ApplierHost>,
        runtime: RuntimeHandle,
        observer: SnapshotStateObserver,
        root: Option<NodeId>,
    ) -> Self {
        let shared_state = slots.runtime_state().unwrap_or(shared_state);
        shared_state.bind_applier_host(&applier);
        shared_state.bind_slots_host(&slots);
        let core = Rc::new(ComposerCore::new(
            shared_state,
            slots,
            applier,
            runtime,
            observer,
            root,
        ));
        Self { core }
    }

    pub fn new(
        slots: Rc<SlotsHost>,
        applier: Rc<dyn ApplierHost>,
        runtime: RuntimeHandle,
        observer: SnapshotStateObserver,
        root: Option<NodeId>,
    ) -> Self {
        Self::new_with_shared_state(
            slots
                .runtime_state()
                .unwrap_or_else(|| Rc::new(ComposerRuntimeState::default())),
            slots,
            applier,
            runtime,
            observer,
            root,
        )
    }

    pub(crate) fn from_core(core: Rc<ComposerCore>) -> Self {
        Self { core }
    }

    pub(crate) fn clone_core(&self) -> Rc<ComposerCore> {
        Rc::clone(&self.core)
    }

    fn observer(&self) -> SnapshotStateObserver {
        self.core.observer.clone()
    }

    pub(crate) fn request_root_render(&self) {
        self.core.root_render_requested.set(true);
    }

    pub(crate) fn take_root_render_request(&self) -> bool {
        self.core.root_render_requested.replace(false)
    }

    pub(crate) fn observe_scope<R>(&self, scope: &RecomposeScope, block: impl FnOnce() -> R) -> R {
        let observer = self.observer();
        let scope_clone = scope.clone();
        observer.observe_reads(scope_clone, move |scope_ref| scope_ref.invalidate(), block)
    }

    fn active_slots_host(&self) -> Rc<SlotsHost> {
        self.core
            .slot_hosts
            .borrow()
            .last()
            .cloned()
            .unwrap_or_else(|| Rc::clone(&self.core.slots))
    }

    pub(crate) fn with_slots<R>(&self, f: impl FnOnce(&SlotTable) -> R) -> R {
        let host = self.active_slots_host();
        let slots = host.borrow();
        f(&slots)
    }

    pub(crate) fn with_slots_mut<R>(&self, f: impl FnOnce(&mut SlotTable) -> R) -> R {
        let host = self.active_slots_host();
        let mut slots = host.borrow_mut();
        f(&mut slots)
    }

    pub(crate) fn with_slot_session_mut<R>(
        &self,
        f: impl FnOnce(&mut crate::slot::SlotWriteSession<'_>) -> R,
    ) -> R {
        self.active_slots_host().with_write_session(f)
    }

    pub(crate) fn with_slot_host_pass<R>(
        &self,
        slots: Rc<SlotsHost>,
        mode: crate::slot::SlotPassMode,
        f: impl FnOnce(&Composer) -> R,
    ) -> (R, SlotPassOutcome) {
        match slots.runtime_state() {
            Some(state) => {
                assert!(
                    Rc::ptr_eq(&state, &self.core.shared_state),
                    "slot host already belongs to a different composer runtime state"
                );
                state.bind_slots_host(&slots);
            }
            None => self.core.shared_state.bind_slots_host(&slots),
        }
        slots.begin_pass(mode);
        self.core.slot_hosts.borrow_mut().push(Rc::clone(&slots));

        struct Guard {
            core: Rc<ComposerCore>,
            host: Rc<SlotsHost>,
            outcome: Rc<RefCell<Option<SlotPassOutcome>>>,
        }
        impl Drop for Guard {
            fn drop(&mut self) {
                let finished = {
                    let mut applier = self.core.applier.borrow_dyn();
                    self.host.finish_pass(&mut *applier)
                };
                let composer = Composer::from_core(Rc::clone(&self.core));
                composer.handle_detached_children_in_host(
                    &self.host,
                    None,
                    finished.detached_root_children,
                );
                composer.evict_retained_subtrees_for_host(&self.host);
                self.host.complete_pass_cleanup(finished.outcome.compacted);
                let host = self
                    .core
                    .slot_hosts
                    .borrow_mut()
                    .pop()
                    .expect("slot host underflow");
                debug_assert!(Rc::ptr_eq(&host, &self.host));

                *self.outcome.borrow_mut() = Some(finished.outcome);
            }
        }
        let outcome = Rc::new(RefCell::new(None));
        let guard = Guard {
            core: self.clone_core(),
            host: Rc::clone(&slots),
            outcome: Rc::clone(&outcome),
        };
        let result = f(self);
        drop(guard);
        let outcome = outcome
            .borrow_mut()
            .take()
            .expect("slot pass should produce an outcome");
        (result, outcome)
    }

    pub(crate) fn with_slot_override<R>(
        &self,
        slots: Rc<SlotsHost>,
        f: impl FnOnce(&Composer) -> R,
    ) -> (R, SlotPassOutcome) {
        self.with_slot_host_pass(slots, crate::slot::SlotPassMode::Compose, f)
    }

    pub(crate) fn parent_stack(&self) -> RefMut<'_, Vec<ParentFrame>> {
        self.core.parent_stack.borrow_mut()
    }

    pub(crate) fn subcompose_stack(&self) -> RefMut<'_, Vec<SubcomposeFrame>> {
        self.core.subcompose_stack.borrow_mut()
    }

    pub(crate) fn commands_mut(&self) -> RefMut<'_, CommandQueue> {
        self.core.commands.borrow_mut()
    }

    pub(crate) fn enqueue_semantics_invalidation(&self, id: NodeId) {
        self.commands_mut().push(Command::BubbleDirty {
            node_id: id,
            bubble: DirtyBubble::SEMANTICS,
        });
    }

    pub(crate) fn scope_stack(&self) -> RefMut<'_, Vec<RecomposeScope>> {
        self.core.scope_stack.borrow_mut()
    }

    fn scope_for_id(&self, scope_id: ScopeId) -> Option<RecomposeScope> {
        self.core.shared_state.scope_for_id(scope_id)
    }

    fn register_scope(&self, scope: &RecomposeScope) {
        self.core.shared_state.register_scope(scope);
    }

    fn remove_scope(&self, scope_id: ScopeId) -> Option<RecomposeScope> {
        self.core.shared_state.remove_scope(scope_id)
    }

    pub(crate) fn local_stack(&self) -> RefMut<'_, LocalStackSnapshot> {
        self.core.local_stack.borrow_mut()
    }

    pub(crate) fn current_local_stack(&self) -> LocalStackSnapshot {
        self.core.local_stack.borrow().clone()
    }

    pub(crate) fn side_effects_mut(&self) -> RefMut<'_, Vec<Box<dyn FnOnce()>>> {
        self.core.side_effects.borrow_mut()
    }

    fn pending_scope_options(&self) -> RefMut<'_, Option<RecomposeOptions>> {
        self.core.pending_scope_options.borrow_mut()
    }

    pub(crate) fn borrow_applier(&self) -> RefMut<'_, dyn Applier> {
        self.core.applier.borrow_dyn()
    }

    /// Registers a virtual node in the Applier.
    ///
    /// This is used by SubcomposeLayoutNode to register virtual container nodes
    /// so that subsequent insert_child commands can find them and attach children.
    /// Without this, virtual nodes would only exist in SubcomposeLayoutNodeInner.virtual_nodes
    /// and applier.get_mut(virtual_node_id) would fail, breaking child attachment.
    pub fn register_virtual_node(
        &self,
        node_id: NodeId,
        node: Box<dyn Node>,
    ) -> Result<(), NodeError> {
        let mut applier = self.borrow_applier();
        applier.insert_with_id(node_id, node)
    }

    /// Checks if a node has no parent (is a root node).
    /// Used by SubcomposeMeasureScope to filter subcompose results.
    pub fn node_has_no_parent(&self, node_id: NodeId) -> bool {
        let mut applier = self.borrow_applier();
        match applier.get_mut(node_id) {
            Ok(node) => node.parent().is_none(),
            Err(_) => true,
        }
    }

    /// Gets the children of a node from the Applier.
    ///
    /// This is used by SubcomposeLayoutNode to get children of virtual nodes
    /// directly from the Applier, where insert_child commands have been applied.
    pub fn get_node_children(&self, node_id: NodeId) -> SmallVec<[NodeId; 8]> {
        let mut applier = self.borrow_applier();
        match applier.get_mut(node_id) {
            Ok(node) => {
                let mut children = SmallVec::<[NodeId; 8]>::new();
                node.collect_children_into(&mut children);
                children
            }
            Err(_) => SmallVec::<[NodeId; 8]>::new(),
        }
    }

    /// Records a child node in the current parent frame's expected children list.
    ///
    /// Used by SubcomposeLayout's `perform_subcompose` to register virtual nodes
    /// with the outer composer's parent frame. This ensures that the `pop_parent`
    /// call at the end of `subcompose_slot` generates a correct `SyncChildren`
    /// command that preserves (rather than removes) the virtual nodes.
    ///
    /// Without this, `pop_parent` would generate `SyncChildren { expected: [] }`,
    /// which removes all virtual nodes and their subtrees from the applier.
    pub fn record_subcompose_child(&self, child_id: NodeId) {
        let mut parent_stack = self.parent_stack();
        if let Some(frame) = parent_stack.last_mut() {
            if matches!(frame.attach_mode, ParentAttachMode::DeferredSync) {
                if let Some(membership) = frame.new_children_membership.as_mut() {
                    if membership.insert(child_id) {
                        frame.new_children.push(child_id);
                    }
                } else if frame.new_children.len() >= LARGE_DEFERRED_CHILD_TRACKING_THRESHOLD {
                    let mut membership = HashSet::default();
                    membership.reserve(frame.new_children.len() + 1);
                    membership.extend(frame.new_children.iter().copied());
                    if membership.insert(child_id) {
                        frame.new_children.push(child_id);
                    }
                    frame.new_children_membership = Some(membership);
                } else if !frame.new_children.contains(&child_id) {
                    frame.new_children.push(child_id);
                }
            }
        }
    }

    /// Clears all children of a node in the Applier.
    ///
    /// This is used by SubcomposeLayoutNode when reusing a virtual node for
    /// different content. Without clearing, old children remain attached,
    /// causing duplicate/interleaved items in lazy lists after scrolling.
    pub fn clear_node_children(&self, node_id: NodeId) {
        let mut applier = self.borrow_applier();
        if let Ok(node) = applier.get_mut(node_id) {
            node.update_children(&[]);
        }
    }

    pub fn install<R>(&self, f: impl FnOnce(&Composer) -> R) -> R {
        let _composer_guard = composer_context::enter(self);
        runtime::push_active_runtime(&self.core.runtime);
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                runtime::pop_active_runtime();
            }
        }
        let guard = Guard;
        let result = f(self);
        drop(guard);
        result
    }

    pub(crate) fn flush_pending_commands_if_large(&self) {
        let queued = self.core.commands.borrow().len();
        if queued < COMMAND_FLUSH_THRESHOLD {
            return;
        }
        self.apply_pending_commands()
            .expect("mid-composition command flush failed");
    }

    fn with_group_in_active_pass<R>(
        &self,
        key: crate::slot_storage::GroupKeySeed,
        f: impl FnOnce(&Composer) -> R,
    ) -> R {
        struct GroupGuard {
            composer: Composer,
            scope: RecomposeScope,
        }

        impl Drop for GroupGuard {
            fn drop(&mut self) {
                let crate::slot::FinishGroupResult {
                    detached_children,
                    direct_nodes,
                    root_nodes,
                    was_skipped,
                } = self
                    .composer
                    .with_slot_session_mut(|slots| slots.finish_group_body());
                if was_skipped {
                    self.composer.attach_root_nodes(root_nodes);
                }
                self.composer.dispose_detached_nodes(direct_nodes);
                self.composer
                    .handle_detached_children(Some(self.scope.id()), detached_children);
                self.composer.scope_stack().pop();
                self.scope.mark_recomposed();
                self.composer
                    .with_slot_session_mut(|slots| slots.end_group());
                self.composer.flush_pending_commands_if_large();
            }
        }

        let parent_scope = self.current_recranpose_scope();
        let options = self.pending_scope_options().take().unwrap_or_default();
        let parent_scope_id = parent_scope.as_ref().map(RecomposeScope::id);
        let reserved_key = self.with_slot_session_mut(|slots| slots.preview_group_key(key));
        let host = self.active_slots_host();
        let restored = self.core.shared_state.take_retained(
            &host,
            RetainKey {
                parent_scope: parent_scope_id,
                key: reserved_key,
            },
        );
        let (group, group_anchor, start_scope_id, start_kind) =
            self.with_slot_session_mut(|slots| {
                let GroupStart {
                    group,
                    anchor,
                    scope_id,
                    kind,
                    ..
                } = slots.begin_group(BeginGroupInput::new(reserved_key, restored));
                (group, anchor, scope_id, kind)
            });
        let scope_ref = if let Some(scope_id) = start_scope_id {
            self.scope_for_id(scope_id)
                .expect("group scope id must resolve through composer registry")
        } else {
            let scope = RecomposeScope::new(self.runtime_handle());
            self.register_scope(&scope);
            self.with_slot_session_mut(|slots| slots.set_group_scope(group, scope.id()));
            scope
        };

        scope_ref.reactivate();
        scope_ref.set_group_anchor(group_anchor);
        scope_ref.set_parent_scope(parent_scope);
        scope_ref.set_retention_mode(options.retention);

        if options.force_recompose {
            scope_ref.force_recompose();
        } else if options.force_reuse {
            scope_ref.force_reuse();
        }
        if matches!(start_kind, GroupStartKind::Restored) {
            scope_ref.force_recompose();
        }

        scope_ref.set_slots_host(&host);

        {
            let mut stack = self.scope_stack();
            stack.push(scope_ref.clone());
        }

        {
            let mut stack = self.subcompose_stack();
            if let Some(frame) = stack.last_mut() {
                frame.scopes.push(scope_ref.clone());
            }
        }

        scope_ref.snapshot_locals(self.current_local_stack());
        {
            let parent_hint = self.parent_stack().last().map(|frame| frame.id);
            scope_ref.set_parent_hint(parent_hint);
        }

        let guard = GroupGuard {
            composer: self.clone(),
            scope: scope_ref.clone(),
        };
        let result = self.observe_scope(&scope_ref, || f(self));
        scope_ref.mark_composed_once();
        drop(guard);
        result
    }

    pub(crate) fn with_group_seed<R>(
        &self,
        key: crate::slot_storage::GroupKeySeed,
        f: impl FnOnce(&Composer) -> R,
    ) -> R {
        let host = self.active_slots_host();
        if host.has_active_pass() {
            return self.with_group_in_active_pass(key, f);
        }
        let (result, _) =
            self.with_slot_host_pass(host, crate::slot::SlotPassMode::Compose, |composer| {
                composer.with_group_in_active_pass(key, f)
            });
        result
    }

    pub fn with_group<R>(&self, key: Key, f: impl FnOnce(&Composer) -> R) -> R {
        self.with_group_seed(crate::slot_storage::GroupKeySeed::unkeyed(key), f)
    }

    pub fn cranpose_with_reuse<R>(
        &self,
        key: Key,
        mut options: RecomposeOptions,
        f: impl FnOnce(&Composer) -> R,
    ) -> R {
        options.retention = RetentionMode::RetainWhenInactive;
        self.pending_scope_options().replace(options);
        self.with_group(key, f)
    }

    #[track_caller]
    pub fn with_key<K: Hash, R>(&self, key: &K, f: impl FnOnce(&Composer) -> R) -> R {
        let seed = explicit_group_key_seed(key, std::panic::Location::caller());
        self.with_group_seed(seed, f)
    }

    pub(crate) fn dispose_detached_nodes(&self, nodes: Vec<NodeId>) {
        for node_id in nodes {
            self.commands_mut().push(Command::callback(move |applier| {
                crate::slot::dispose_detached_node_now(applier, node_id)
            }));
        }
    }

    fn debug_assert_detached_subtree_metadata(&self, subtree: &crate::slot::DetachedSubtree) {
        debug_assert!(
            subtree.generation() > 0,
            "detached subtrees must carry a non-zero generation",
        );
        debug_assert!(
            subtree.node_count() == 0 || !subtree.root_nodes().is_empty(),
            "detached subtree nodes must carry stored root metadata",
        );
        debug_assert!(
            {
                let node_ids = subtree.node_ids_iter().collect::<HashSet<_>>();
                subtree.root_nodes().iter().all(|id| node_ids.contains(id))
            },
            "detached subtree root ids must belong to the subtree node set",
        );
    }

    fn deactivate_scope_ids(&self, scope_ids: impl IntoIterator<Item = ScopeId>) {
        for scope_id in scope_ids {
            if let Some(scope) = self.scope_for_id(scope_id) {
                scope.deactivate();
            }
        }
    }

    fn dispose_scope_ids(&self, scope_ids: impl IntoIterator<Item = ScopeId>) {
        for scope_id in scope_ids {
            if let Some(scope) = self.remove_scope(scope_id) {
                scope.deactivate();
                scope.set_group_anchor(crate::AnchorId::INVALID);
            }
        }
    }

    fn retain_detached_subtree_in_host(
        &self,
        slots_host: &Rc<SlotsHost>,
        parent_scope: Option<ScopeId>,
        subtree: crate::slot::DetachedSubtree,
    ) {
        self.debug_assert_detached_subtree_metadata(&subtree);
        self.deactivate_scope_ids(subtree.scope_ids_iter());
        for root in subtree.root_nodes_iter() {
            let parent_id = {
                let mut applier = self.borrow_applier();
                applier.get_mut(root).ok().and_then(|node| node.parent())
            };
            if let Some(parent_id) = parent_id {
                self.commands_mut().push(Command::DetachChild {
                    parent_id,
                    child_id: root,
                });
            }
        }
        let evicted = self.core.shared_state.insert_retained(
            slots_host,
            RetainKey {
                parent_scope,
                key: subtree.root_key(),
            },
            subtree,
        );
        for subtree in evicted {
            self.dispose_detached_subtree_in_host(slots_host, subtree);
        }
    }

    fn evict_retained_subtrees_for_host(&self, slots_host: &Rc<SlotsHost>) {
        let evicted = self.core.shared_state.advance_retention_pass(slots_host);
        for subtree in evicted {
            self.dispose_detached_subtree_in_host(slots_host, subtree);
        }
    }

    fn dispose_detached_subtree_in_host(
        &self,
        slots_host: &Rc<SlotsHost>,
        subtree: crate::slot::DetachedSubtree,
    ) {
        self.debug_assert_detached_subtree_metadata(&subtree);
        self.dispose_scope_ids(subtree.scope_ids_iter());
        self.dispose_detached_nodes(subtree.root_nodes().to_vec());
        slots_host.with_table_and_lifecycle_mut(|table, lifecycle| {
            table.invalidate_detached_subtree_anchors(&subtree);
            lifecycle.queue_subtree_disposal(subtree);
        });
    }

    fn handle_detached_children_in_host(
        &self,
        slots_host: &Rc<SlotsHost>,
        parent_scope: Option<ScopeId>,
        detached: Vec<crate::slot::DetachedSubtree>,
    ) {
        for subtree in detached {
            let retention_mode = subtree
                .root_scope_id()
                .and_then(|scope_id| self.scope_for_id(scope_id))
                .map(|scope| scope.retention_mode())
                .unwrap_or_default();
            match retention_mode {
                RetentionMode::DisposeWhenInactive => {
                    self.dispose_detached_subtree_in_host(slots_host, subtree)
                }
                RetentionMode::RetainWhenInactive => {
                    self.retain_detached_subtree_in_host(slots_host, parent_scope, subtree)
                }
            }
        }
    }

    pub(crate) fn handle_detached_children(
        &self,
        parent_scope: Option<ScopeId>,
        detached: Vec<crate::slot::DetachedSubtree>,
    ) {
        let host = self.active_slots_host();
        self.handle_detached_children_in_host(&host, parent_scope, detached);
    }

    pub fn remember<T: 'static>(&self, init: impl FnOnce() -> T) -> Owned<T> {
        self.remember_with_kind(PayloadKind::Remember, init)
    }

    pub(crate) fn remember_internal<T: 'static>(&self, init: impl FnOnce() -> T) -> Owned<T> {
        self.remember_with_kind(PayloadKind::Internal, init)
    }

    pub(crate) fn remember_effect<T: 'static>(&self, init: impl FnOnce() -> T) -> Owned<T> {
        self.remember_with_kind(PayloadKind::Effect, init)
    }

    fn remember_with_kind<T: 'static>(
        &self,
        kind: PayloadKind,
        init: impl FnOnce() -> T,
    ) -> Owned<T> {
        self.with_slot_session_mut(|slots| slots.remember_with_kind(kind, init))
    }

    pub fn use_value_slot<'pass, T: 'static>(
        &'pass self,
        init: impl FnOnce() -> T,
    ) -> ValueSlotHandle<'pass> {
        let slot = self
            .with_slot_session_mut(|slots| reserve_value_slot(slots, PayloadKind::Internal, init));
        ValueSlotHandle::new(slot)
    }

    #[doc(hidden)]
    pub fn __use_param_slot<'pass, T: 'static>(
        &'pass self,
        init: impl FnOnce() -> T,
    ) -> ValueSlotHandle<'pass> {
        let slot =
            self.with_slot_session_mut(|slots| reserve_value_slot(slots, PayloadKind::Param, init));
        ValueSlotHandle::new(slot)
    }

    #[doc(hidden)]
    pub fn __use_return_slot<'pass, T: 'static>(
        &'pass self,
        init: impl FnOnce() -> T,
    ) -> ValueSlotHandle<'pass> {
        let slot = self
            .with_slot_session_mut(|slots| reserve_value_slot(slots, PayloadKind::Return, init));
        ValueSlotHandle::new(slot)
    }

    pub fn with_slot_value<'pass, T: 'static, R>(
        &'pass self,
        handle: ValueSlotHandle<'pass>,
        f: impl FnOnce(&T) -> R,
    ) -> R {
        self.with_slots(|slots| f(slots.read_value(handle.slot())))
    }

    pub fn with_slot_value_mut<'pass, T: 'static, R>(
        &'pass self,
        handle: ValueSlotHandle<'pass>,
        f: impl FnOnce(&mut T) -> R,
    ) -> R {
        self.with_slots_mut(|slots| f(slots.read_value_mut(handle.slot())))
    }

    pub fn write_slot_value<'pass, T: 'static>(
        &'pass self,
        handle: ValueSlotHandle<'pass>,
        value: T,
    ) {
        self.with_slots_mut(|slots| slots.write_value(handle.slot(), value));
    }

    pub fn mutable_state_of<T: Clone + 'static>(&self, initial: T) -> MutableState<T> {
        MutableState::with_runtime(initial, self.runtime_handle())
    }

    pub fn mutable_state_list_of<T, I>(&self, values: I) -> SnapshotStateList<T>
    where
        T: Clone + 'static,
        I: IntoIterator<Item = T>,
    {
        SnapshotStateList::with_runtime(values, self.runtime_handle())
    }

    pub fn mutable_state_map_of<K, V, I>(&self, pairs: I) -> SnapshotStateMap<K, V>
    where
        K: Clone + Eq + Hash + 'static,
        V: Clone + 'static,
        I: IntoIterator<Item = (K, V)>,
    {
        SnapshotStateMap::with_runtime(pairs, self.runtime_handle())
    }

    pub fn read_composition_local<T: Clone + 'static>(&self, local: &CompositionLocal<T>) -> T {
        let stack = self.core.local_stack.borrow();
        for context in stack.iter().rev() {
            if let Some(entry) = context.values.get(&local.key) {
                let typed = entry
                    .clone()
                    .downcast::<LocalStateEntry<T>>()
                    .expect("composition local type mismatch");
                return typed.value();
            }
        }
        local.default_value()
    }

    pub fn read_static_composition_local<T: Clone + 'static>(
        &self,
        local: &StaticCompositionLocal<T>,
    ) -> T {
        let stack = self.core.local_stack.borrow();
        for context in stack.iter().rev() {
            if let Some(entry) = context.values.get(&local.key) {
                let typed = entry
                    .clone()
                    .downcast::<StaticLocalEntry<T>>()
                    .expect("static composition local type mismatch");
                return typed.value();
            }
        }
        local.default_value()
    }

    pub fn current_recranpose_scope(&self) -> Option<RecomposeScope> {
        self.core.scope_stack.borrow().last().cloned()
    }

    pub fn phase(&self) -> crate::Phase {
        self.core.phase.get()
    }

    pub(crate) fn set_phase(&self, phase: crate::Phase) {
        self.core.phase.set(phase);
    }

    pub fn enter_phase(&self, phase: crate::Phase) {
        self.set_phase(phase);
    }

    pub(crate) fn subcompose<R>(
        &self,
        state: &mut SubcomposeState,
        slot_id: SlotId,
        content: impl FnOnce(&Composer) -> R,
    ) -> (R, Vec<NodeId>) {
        match self.phase() {
            crate::Phase::Measure | crate::Phase::Layout => {}
            current => panic!(
                "subcompose() may only be called during measure or layout; current phase: {:?}",
                current
            ),
        }

        self.subcompose_stack().push(SubcomposeFrame::default());
        struct StackGuard {
            core: Rc<ComposerCore>,
            leaked: bool,
        }
        impl Drop for StackGuard {
            fn drop(&mut self) {
                if !self.leaked {
                    self.core.subcompose_stack.borrow_mut().pop();
                }
            }
        }
        let mut guard = StackGuard {
            core: self.clone_core(),
            leaked: false,
        };

        let slot_host = state.get_or_create_slots(slot_id);
        let (result, _) = self.with_slot_override(slot_host.clone(), |composer| {
            composer.with_group(slot_id.raw(), |composer| content(composer))
        });

        let frame = {
            let mut stack = guard.core.subcompose_stack.borrow_mut();
            let frame = stack.pop().expect("subcompose stack underflow");
            guard.leaked = true;
            frame
        };
        let nodes = frame.nodes;
        let scopes = frame.scopes;
        state.register_active(slot_id, &nodes, &scopes);
        (result, nodes)
    }

    pub fn subcompose_measurement<R>(
        &self,
        state: &mut SubcomposeState,
        slot_id: SlotId,
        content: impl FnOnce(&Composer) -> R,
    ) -> (R, Vec<NodeId>) {
        let (result, nodes) = self.subcompose(state, slot_id, content);
        let roots = nodes
            .into_iter()
            .filter(|&id| self.node_has_no_parent(id))
            .collect();

        (result, roots)
    }

    pub fn subcompose_in<R>(
        &self,
        slots: &Rc<SlotsHost>,
        root: Option<NodeId>,
        f: impl FnOnce(&Composer) -> R,
    ) -> Result<R, NodeError> {
        let runtime_handle = self.runtime_handle();
        let phase = self.phase();
        let locals = self.current_local_stack();
        let shared_state = slots
            .runtime_state()
            .unwrap_or_else(|| Rc::clone(&self.core.shared_state));
        let core = Rc::new(ComposerCore::new(
            shared_state,
            Rc::clone(slots),
            Rc::clone(&self.core.applier),
            runtime_handle.clone(),
            self.observer(),
            root,
        ));
        core.phase.set(phase);
        *core.local_stack.borrow_mut() = locals;
        let composer = Composer::from_core(core);
        let (result, commands, side_effects, compact_applier) = composer.install(|composer| {
            let (output, outcome) = composer.with_slot_host_pass(
                Rc::clone(slots),
                crate::slot::SlotPassMode::Compose,
                |composer| f(composer),
            );
            let commands = composer.take_commands();
            let side_effects = composer.take_side_effects();
            (output, commands, side_effects, outcome.compacted)
        });
        {
            let mut applier = self.borrow_applier();
            commands.apply(&mut *applier)?;
            for update in runtime_handle.take_updates() {
                update.apply(&mut *applier)?;
            }
        }
        if compact_applier {
            self.core.applier.compact();
            self.core.applier.borrow_dyn().clear_recycled_nodes();
        }
        runtime_handle.drain_ui();
        for effect in side_effects {
            effect();
        }
        runtime_handle.drain_ui();
        Ok(result)
    }

    /// Subcomposes content using an isolated SlotsHost without resetting it.
    /// Unlike `subcompose_in`, this preserves existing slot state across calls,
    /// allowing efficient reuse during measurement passes. This is critical for
    /// lazy lists where items need stable slot positions.
    pub fn subcompose_slot<R>(
        &self,
        slots: &Rc<SlotsHost>,
        root: Option<NodeId>,
        f: impl FnOnce(&Composer) -> R,
    ) -> Result<(R, Vec<RecomposeScope>), NodeError> {
        let runtime_handle = self.runtime_handle();
        let phase = self.phase();
        let locals = self.current_local_stack();
        let shared_state = slots
            .runtime_state()
            .unwrap_or_else(|| Rc::clone(&self.core.shared_state));
        let core = Rc::new(ComposerCore::new(
            shared_state,
            Rc::clone(slots),
            Rc::clone(&self.core.applier),
            runtime_handle.clone(),
            self.observer(),
            root,
        ));
        core.phase.set(phase);
        *core.local_stack.borrow_mut() = locals;
        let composer = Composer::from_core(core);
        composer.subcompose_stack().push(SubcomposeFrame::default());
        struct StackGuard {
            core: Rc<ComposerCore>,
            leaked: bool,
        }
        impl Drop for StackGuard {
            fn drop(&mut self) {
                if !self.leaked {
                    self.core.subcompose_stack.borrow_mut().pop();
                }
            }
        }
        let mut guard = StackGuard {
            core: composer.clone_core(),
            leaked: false,
        };
        let root_group_key = crate::location_key(file!(), line!(), column!());
        let (result, commands, side_effects, compact_applier) = composer.install(|composer| {
            let (output, outcome) = composer.with_slot_host_pass(
                Rc::clone(slots),
                crate::slot::SlotPassMode::Compose,
                |composer| {
                    let output = composer.with_group(root_group_key, |composer| f(composer));
                    if root.is_some() {
                        composer.pop_parent();
                    }
                    output
                },
            );
            let commands = composer.take_commands();
            let side_effects = composer.take_side_effects();
            (output, commands, side_effects, outcome.compacted)
        });
        let frame = {
            let mut stack = guard.core.subcompose_stack.borrow_mut();
            let frame = stack.pop().expect("subcompose stack underflow");
            guard.leaked = true;
            frame
        };

        {
            let mut applier = self.borrow_applier();
            commands.apply(&mut *applier)?;
            for update in runtime_handle.take_updates() {
                update.apply(&mut *applier)?;
            }
        }
        if compact_applier {
            self.core.applier.compact();
            self.core.applier.borrow_dyn().clear_recycled_nodes();
        }
        runtime_handle.drain_ui();
        for effect in side_effects {
            effect();
        }
        runtime_handle.drain_ui();
        Ok((result, frame.scopes))
    }

    pub(crate) fn attach_root_nodes(&self, root_nodes: Vec<NodeId>) {
        for id in root_nodes {
            self.attach_to_parent_with_mode(id, true);
        }
    }

    pub fn skip_current_group(&self) {
        self.with_slot_session_mut(|slots| skip_slot_group(slots));
    }

    pub fn runtime_handle(&self) -> RuntimeHandle {
        self.core.runtime.clone()
    }

    pub fn set_recranpose_callback<F>(&self, callback: F)
    where
        F: FnMut(&Composer) + 'static,
    {
        if let Some(scope) = self.current_recranpose_scope() {
            let observer = self.observer();
            let scope_weak = scope.downgrade();
            let mut callback = callback;
            scope.set_recompose(Box::new(move |composer: &Composer| {
                if let Some(inner) = scope_weak.upgrade() {
                    let scope_instance = RecomposeScope { inner };
                    observer.observe_reads(
                        scope_instance.clone(),
                        move |scope_ref| scope_ref.invalidate(),
                        || {
                            callback(composer);
                        },
                    );
                }
            }));
        }
    }

    pub fn set_recranpose_fn(&self, callback: fn(&Composer)) {
        if let Some(scope) = self.current_recranpose_scope() {
            scope.set_recompose_fn(callback);
        }
    }

    pub fn with_composition_locals<R>(
        &self,
        provided: Vec<ProvidedValue>,
        f: impl FnOnce(&Composer) -> R,
    ) -> R {
        if provided.is_empty() {
            return f(self);
        }
        let mut context = LocalContext::default();
        for value in provided {
            let (key, entry) = value.into_entry(self);
            context.values.insert(key, entry);
        }
        {
            let mut stack = self.local_stack();
            Rc::make_mut(&mut *stack).push(context);
        }
        let result = f(self);
        {
            let mut stack = self.local_stack();
            Rc::make_mut(&mut *stack).pop();
        }
        result
    }
}
