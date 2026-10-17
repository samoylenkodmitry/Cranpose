use std::{
    any::Any,
    cell::{Cell, RefCell, RefMut},
    hash::Hash,
    marker::PhantomData,
    rc::{Rc, Weak},
};

use smallvec::SmallVec;

use crate::{
    AnchorId, Applier, ApplierGuard, ApplierHost, COMMAND_FLUSH_THRESHOLD, ChildList, Command,
    CommandQueue, CompositionLocal, ComputedLocalEntry, DirtyBubble, Key, LocalKey,
    LocalStackSnapshot, LocalStateEntry, MutableState, Node, NodeError, NodeId, Owned,
    ProvidedValue, RecomposeOptions, RecomposeScope, RecomposeScopeInner, RecycledNode,
    RetentionMode, RetentionPolicy, RuntimeHandle, ScopeId, SlotId, SlotPassOutcome, SlotTable,
    SlotsHost, SnapshotStateList, SnapshotStateMap, StaticCompositionLocal, SubcomposeState,
    collections::map::{HashMap, HashSet},
    composer_context, explicit_group_key_seed,
    retention::{RetainKey, RetentionManager},
    runtime,
    slot::{FinishGroupResult, GroupStart, GroupStartKind, PayloadKind, RootNodeIds, ValueSlotId},
};

/// A typed reference to a value stored in a slot, made by `Composer::use_value_slot`. It is valid
/// for the composition pass that made it, and `Composer::with_slot_value` reads through it.
pub struct ValueSlotHandle<'pass, T: 'static> {
    slot: ValueSlotId,
    record: Option<usize>,
    _pass: PhantomData<&'pass Composer>,
    _value: PhantomData<fn() -> T>,
}

impl<T: 'static> Copy for ValueSlotHandle<'_, T> {}

impl<T: 'static> Clone for ValueSlotHandle<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> ValueSlotHandle<'_, T> {
    pub(crate) fn new(slot: ValueSlotId, record: Option<usize>) -> Self {
        Self {
            slot,
            record,
            _pass: PhantomData,
            _value: PhantomData,
        }
    }
}

fn slots_storage_key(host: &Rc<SlotsHost>) -> usize {
    host.storage_key()
}

fn bind_slots_host_to_runtime_state(
    state: &Rc<ComposerRuntimeState>,
    host: &Rc<SlotsHost>,
) -> Rc<SlotsHost> {
    // A host bound to `state` is in its registry: whatever drops the entry
    // also unbinds the host, so binding it again would change nothing.
    if host.runtime_state_ptr() == Some(Rc::as_ptr(state)) {
        debug_assert!(
            state
                .host_for_storage_key(host.storage_key())
                .is_some_and(|registered| Rc::ptr_eq(&registered, host)),
            "a slot host bound to a runtime state must be in its registry"
        );
        return Rc::clone(host);
    }
    if host.runtime_state().is_some() {
        if host.rebind_orphaned_runtime_state(state) {
            state.bind_slots_host(host);
            return Rc::clone(host);
        }
        log::error!(
            "slot host already belongs to a different composer runtime state; using a fresh slot host"
        );
        let replacement = Rc::new(SlotsHost::new(SlotTable::new()));
        state.bind_slots_host(&replacement);
        return replacement;
    }
    state.bind_slots_host(host);
    Rc::clone(host)
}

struct GroupEntry {
    key: crate::slot::GroupKey,
    /// Boxed: a restore is rare, and the entry moves on every call.
    restored: Option<Box<crate::slot::DetachedSubtree>>,
    placeholder_for: Option<crate::slot::GroupKey>,
}

struct GroupScopeEntry<'a> {
    options: RecomposeOptions,
    start_kind: GroupStartKind,
    host: &'a SlotsHost,
    group: crate::slot::ActiveGroupId,
    parent_hint: Option<NodeId>,
}

struct SlotHostPassGuard {
    core: Rc<ComposerCore>,
    host: Rc<SlotsHost>,
    /// The branch fold depth the pass began at.
    fold_base: usize,
    active: bool,
}

impl SlotHostPassGuard {
    fn close(&mut self) {
        if !self.active {
            return;
        }
        if self.host.has_active_pass() {
            self.host.abandon_active_pass();
        }
        match self.core.slot_hosts.borrow_mut().pop() {
            Some(host) if Rc::ptr_eq(&host, &self.host) => {}
            Some(_) => {
                log::error!("slot host stack mismatch while closing slot host pass");
            }
            None => {
                log::error!("slot host stack underflow while closing slot host pass");
            }
        }
        self.core.branch_folds.end_pass(self.fold_base);
        self.active = false;
    }
}

impl Drop for SlotHostPassGuard {
    fn drop(&mut self) {
        self.close();
    }
}

pub(crate) struct PendingMovable {
    pub(crate) key: crate::slot::GroupKey,
    pub(crate) placeholder: AnchorId,
    pub(crate) parent_scope: Option<Weak<RecomposeScopeInner>>,
}

fn reparent_restored_scopes(
    host: &SlotsHost,
    group: crate::slot::ActiveGroupId,
    old_hint: Option<NodeId>,
    parent_hint: Option<NodeId>,
) {
    host.with_write_session(|slots| {
        slots.for_each_subtree_scope(group, |scope| {
            if scope.parent_hint() == old_hint {
                scope.set_parent_hint(parent_hint);
            }
            scope.reactivate();
        });
    });
}

fn movable_retain_key(id: Key) -> RetainKey {
    RetainKey::for_group(
        None,
        crate::slot::GroupKey::new(crate::slot::MOVABLE_STATIC_KEY, Some(id), 0),
    )
}

pub(crate) struct ComposerRuntimeState {
    retention_by_host: RefCell<HashMap<usize, RetentionManager>>,
    pending_movables_by_host: RefCell<HashMap<usize, Vec<PendingMovable>>>,
    retention_policy: Cell<RetentionPolicy>,
    live_hosts: RefCell<HashMap<usize, std::rc::Weak<SlotsHost>>>,
    applier_host: RefCell<Option<std::rc::Weak<dyn ApplierHost>>>,
    branch_folds: Rc<crate::slot::BranchFolds>,
}

/// How many applied command queues a thread keeps for its passes.
const SPARE_COMMAND_QUEUES: usize = 8;

/// The most commands a kept queue has room for. A larger one grew in a
/// spike such as the first composition, and keeping it would hold its memory.
const SPARE_COMMAND_CAPACITY: usize = 1024;

fn spare_commands() -> CommandQueue {
    crate::thread_state::try_runtimes(|state| state.spare_commands.borrow_mut().pop())
        .flatten()
        .unwrap_or_default()
}

/// The stacks a composer grows while it composes, kept empty between passes
/// so the next composer on this thread starts with their storage.
#[derive(Default)]
pub(crate) struct ComposerStacks {
    slot_hosts: Vec<Rc<SlotsHost>>,
    parent_stack: Vec<ParentFrame>,
    scope_stack: Vec<RecomposeScope>,
}

/// The most entries a kept stack has room for; a larger one grew in a spike
/// such as a deep first composition.
const SPARE_STACK_CAPACITY: usize = 256;

fn spare_composer_stacks() -> ComposerStacks {
    crate::thread_state::try_runtimes(|state| state.spare_composer_stacks.take())
        .flatten()
        .unwrap_or_default()
}

impl ComposerStacks {
    fn holds_storage(&self) -> bool {
        self.slot_hosts.capacity() > 0
            || self.parent_stack.capacity() > 0
            || self.scope_stack.capacity() > 0
    }
}

/// Keeps a dropped composer's emptied stacks for the next composer.
fn recycle_composer_stacks(mut stacks: ComposerStacks) {
    let ComposerStacks {
        slot_hosts,
        parent_stack,
        scope_stack,
    } = &mut stacks;
    if slot_hosts.capacity() > SPARE_STACK_CAPACITY
        || parent_stack.capacity() > SPARE_STACK_CAPACITY
        || scope_stack.capacity() > SPARE_STACK_CAPACITY
    {
        return;
    }
    slot_hosts.clear();
    parent_stack.clear();
    scope_stack.clear();
    crate::thread_state::try_runtimes(|state| state.spare_composer_stacks.set(Some(stacks)));
}

/// Keeps an applied, empty command queue for the next pass on this thread.
pub(crate) fn recycle_commands(commands: CommandQueue) {
    if commands.capacity() > SPARE_COMMAND_CAPACITY {
        return;
    }
    crate::thread_state::try_runtimes(|state| {
        let mut spare = state.spare_commands.borrow_mut();
        if spare.len() < SPARE_COMMAND_QUEUES {
            spare.push(commands);
        }
    });
}

impl Default for ComposerRuntimeState {
    fn default() -> Self {
        Self {
            retention_by_host: RefCell::new(HashMap::default()),
            pending_movables_by_host: RefCell::new(HashMap::default()),
            retention_policy: Cell::new(RetentionPolicy::default()),
            live_hosts: RefCell::new(HashMap::default()),
            applier_host: RefCell::new(None),
            branch_folds: Rc::default(),
        }
    }
}

impl ComposerRuntimeState {
    pub(crate) fn clear_host_storage_key(&self, host_key: usize, table: &mut SlotTable) {
        self.retention_by_host.borrow_mut().remove(&host_key);
        self.pending_movables_by_host.borrow_mut().remove(&host_key);
        self.live_hosts.borrow_mut().remove(&host_key);
        table.release_scopes();
    }

    pub(crate) fn force_recompose_retained_scopes(&self, host_key: usize) {
        if let Some(retention) = self.retention_by_host.borrow().get(&host_key) {
            for scope in retention
                .subtrees()
                .flat_map(crate::slot::DetachedSubtree::scopes)
            {
                scope.force_recompose();
            }
        }
    }

    pub(crate) fn bind_applier_host(&self, applier: &Rc<dyn ApplierHost>) {
        *self.applier_host.borrow_mut() = Some(Rc::downgrade(applier));
    }

    pub(crate) fn has_live_applier_host(&self) -> bool {
        self.applier_host
            .borrow()
            .as_ref()
            .and_then(std::rc::Weak::upgrade)
            .is_some()
    }

    pub(crate) fn bind_slots_host(self: &Rc<Self>, host: &Rc<SlotsHost>) {
        host.bind_runtime_state(self);
        self.live_hosts
            .borrow_mut()
            .insert(host.storage_key(), Rc::downgrade(host));
    }

    pub(crate) fn retains_any(&self) -> bool {
        !self.retention_by_host.borrow().is_empty()
    }

    pub(crate) fn set_retention_policy(&self, policy: RetentionPolicy) {
        self.retention_policy.set(policy);
    }

    pub(crate) fn retention_policy(&self) -> RetentionPolicy {
        self.retention_policy.get()
    }

    pub(crate) fn take_retained(
        &self,
        host: &Rc<SlotsHost>,
        key: RetainKey,
        preflight: impl FnOnce(&mut crate::slot::DetachedSubtree) -> bool,
    ) -> Option<crate::slot::DetachedSubtree> {
        let host_key = slots_storage_key(host);
        let mut retention = self.retention_by_host.borrow_mut();
        let subtree = retention
            .get_mut(&host_key)?
            .take_after_restore_preflight(key, preflight);
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
        pinned: bool,
    ) -> Vec<crate::slot::DetachedSubtree> {
        let policy = self.retention_policy();
        let mut retention_by_host = self.retention_by_host.borrow_mut();
        let manager = retention_by_host
            .entry(slots_storage_key(host))
            .or_insert_with(|| RetentionManager::new(policy));
        manager.set_policy(policy);
        if pinned {
            manager.insert_pinned(key, subtree)
        } else {
            manager.insert(key, subtree)
        }
    }

    pub(crate) fn take_retained_movable(
        &self,
        id: Key,
    ) -> Option<(Rc<SlotsHost>, crate::slot::DetachedSubtree)> {
        self.take_retained_movable_by_key(movable_retain_key(id))
    }

    fn take_retained_movable_by_key(
        &self,
        key: RetainKey,
    ) -> Option<(Rc<SlotsHost>, crate::slot::DetachedSubtree)> {
        let mut retention_by_host = self.retention_by_host.borrow_mut();
        let (host_key, manager) = retention_by_host
            .iter_mut()
            .find(|(_, manager)| manager.contains(key))?;
        let host = self.host_for_storage_key(*host_key)?;
        let subtree = manager.take(key)?;
        Some((host, subtree))
    }

    /// Takes a movable's retained content out of a slot table other than the
    /// one composing, which is how it crosses a subcomposition.
    pub(crate) fn take_retained_movable_elsewhere(
        &self,
        besides: &Rc<SlotsHost>,
        key: RetainKey,
    ) -> Option<(Rc<SlotsHost>, crate::slot::DetachedSubtree)> {
        let besides_key = slots_storage_key(besides);
        let mut retention_by_host = self.retention_by_host.borrow_mut();
        let (host_key, manager) = retention_by_host
            .iter_mut()
            .find(|(host_key, manager)| **host_key != besides_key && manager.contains(key))?;
        let host = self.host_for_storage_key(*host_key)?;
        let subtree = manager.take(key)?;
        Some((host, subtree))
    }

    /// Whether a movable's content is retained in any slot table, which is
    /// what a site waiting for it has to know: the content may have been let
    /// go by a parent composed in another host, such as a subcomposition.
    pub(crate) fn movable_retained_anywhere(&self, key: RetainKey) -> bool {
        self.retention_by_host
            .borrow()
            .values()
            .any(|manager| manager.contains(key))
    }

    /// The slot table a movable's content is attached to right now, if any.
    pub(crate) fn host_holding_movable(&self, id: Key) -> Option<Rc<SlotsHost>> {
        self.live_hosts
            .borrow()
            .values()
            .filter_map(std::rc::Weak::upgrade)
            .find(|host| host.borrow().movable_is_attached(id))
    }

    pub(crate) fn record_pending_movable(&self, host: &Rc<SlotsHost>, pending: PendingMovable) {
        let mut by_host = self.pending_movables_by_host.borrow_mut();
        let sites = by_host.entry(slots_storage_key(host)).or_default();
        if sites
            .iter()
            .all(|site| site.placeholder != pending.placeholder)
        {
            sites.push(pending);
        }
    }

    pub(crate) fn take_pending_movables(&self, host: &Rc<SlotsHost>) -> Vec<PendingMovable> {
        self.pending_movables_by_host
            .borrow_mut()
            .remove(&slots_storage_key(host))
            .unwrap_or_default()
    }

    /// Every slot table with a site still waiting for movable content. The
    /// content may be let go by a parent composed in another table, so a pass
    /// that ends has to look wider than itself.
    pub(crate) fn hosts_awaiting_movables(&self) -> Vec<Rc<SlotsHost>> {
        let by_host = self.pending_movables_by_host.borrow();
        if by_host.is_empty() {
            return Vec::new();
        }
        let waiting = by_host
            .iter()
            .filter(|(_, sites)| !sites.is_empty())
            .map(|(host_key, _)| *host_key)
            .collect::<Vec<_>>();
        waiting
            .into_iter()
            .filter_map(|host_key| self.host_for_storage_key(host_key))
            .collect()
    }

    pub(crate) fn keep_pending_movables(&self, host: &Rc<SlotsHost>, sites: Vec<PendingMovable>) {
        if sites.is_empty() {
            return;
        }
        self.pending_movables_by_host
            .borrow_mut()
            .insert(slots_storage_key(host), sites);
    }

    pub(crate) fn advance_retention_pass(
        &self,
        host: &Rc<SlotsHost>,
    ) -> Vec<crate::slot::DetachedSubtree> {
        let mut retention_by_host = self.retention_by_host.borrow_mut();
        if retention_by_host.is_empty() {
            return Vec::new();
        }
        let host_key = slots_storage_key(host);
        let policy = self.retention_policy();
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
        snapshot.retained_subtree_count = retention.subtree_count;
        snapshot.retained_group_count = retention.group_count;
        snapshot.retained_payload_count = retention.payload_count;
        snapshot.retained_node_count = retention.node_count;
        snapshot.retained_scope_count = retention.scope_count;
    }

    pub(crate) fn slot_retention_debug_stats(
        &self,
        host: &SlotsHost,
    ) -> crate::slot::SlotRetentionDebugStats {
        let retention = self.retention_debug_stats(host.storage_key());
        crate::slot::SlotRetentionDebugStats {
            retained_subtree_count: retention.subtree_count,
            retained_group_count: retention.group_count,
            retained_payload_count: retention.payload_count,
            retained_node_count: retention.node_count,
            retained_scope_count: retention.scope_count,
            retained_anchor_count: retention.anchor_count,
            retained_heap_bytes: retention.heap_bytes,
            retained_evictions_total: retention.evictions_total,
        }
    }

    pub(crate) fn compact_table_identity_storage_for_host(
        &self,
        host: &SlotsHost,
        table: &mut SlotTable,
        compact_anchors: bool,
        compact_payloads: bool,
    ) {
        if !compact_anchors && !compact_payloads {
            return;
        }

        let host_key = host.storage_key();
        let mut retention = self.retention_by_host.borrow_mut();
        if let Some(retained) = retention.get_mut(&host_key) {
            if compact_anchors {
                table.compact_anchor_registry_storage(Some(&mut *retained));
            }
            if compact_payloads {
                table.compact_payload_anchor_registry_storage(Some(&mut *retained));
            }
        } else {
            if compact_anchors {
                table.compact_anchor_registry_storage(None);
            }
            if compact_payloads {
                table.compact_payload_anchor_registry_storage(None);
            }
        }
    }

    pub(crate) fn clear_host(&self, host: &SlotsHost, table: &mut SlotTable) {
        let host_key = host.storage_key();
        debug_assert!(
            self.host_retention_is_empty(host),
            "host retention must be drained before clearing host ownership"
        );
        self.clear_host_storage_key(host_key, table);
    }

    fn deactivate_and_queue_subtrees(
        &self,
        retention: RetentionManager,
        table: &mut SlotTable,
        lifecycle: &mut crate::slot::SlotLifecycleCoordinator,
    ) {
        for subtree in retention.into_subtrees() {
            for scope in subtree.scopes() {
                scope.deactivate();
            }
            table.invalidate_detached_subtree_anchors(&subtree);
            lifecycle.queue_subtree_disposal(subtree);
        }
    }

    pub(crate) fn dispose_retained_subtrees_for_host(
        &self,
        host_key: usize,
        table: &mut SlotTable,
        lifecycle: &mut crate::slot::SlotLifecycleCoordinator,
    ) -> Result<(), NodeError> {
        let retention = self.retention_by_host.borrow_mut().remove(&host_key);
        let applier_host = self
            .applier_host
            .borrow()
            .as_ref()
            .and_then(std::rc::Weak::upgrade);
        if let Some(applier_host) = applier_host {
            let mut disposal = crate::NodeDisposal::default();
            if let Some(retention) = retention {
                for subtree in retention.into_subtrees() {
                    subtree.scopes().for_each(RecomposeScope::deactivate);
                    table.invalidate_detached_subtree_anchors(&subtree);
                    disposal.retain_subtree(subtree);
                }
            }
            applier_host.dispose_nodes(disposal)?;
        } else if let Some(retention) = retention {
            self.deactivate_and_queue_subtrees(retention, table, lifecycle);
        }
        Ok(())
    }

    pub(crate) fn abandon_retained_subtrees_for_host(
        &self,
        host_key: usize,
        table: &mut SlotTable,
        lifecycle: &mut crate::slot::SlotLifecycleCoordinator,
    ) {
        let Some(retention) = self.retention_by_host.borrow_mut().remove(&host_key) else {
            self.clear_host_storage_key(host_key, table);
            return;
        };
        self.deactivate_and_queue_subtrees(retention, table, lifecycle);
        self.clear_host_storage_key(host_key, table);
    }

    pub(crate) fn host_retention_is_empty(&self, host: &SlotsHost) -> bool {
        self.retention_by_host
            .borrow()
            .get(&host.storage_key())
            .is_none_or(RetentionManager::is_empty)
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn debug_verify_host(&self, host: &SlotsHost, table: &SlotTable) {
        if let Some(retention) = self.retention_by_host.borrow().get(&host.storage_key()) {
            retention.debug_verify(table);
        }
    }

    #[cfg(test)]
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

    #[cold]
    #[inline(never)]
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
    pub(crate) synthetic_root: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum InitialParentFrame {
    SyntheticRoot,
    RealParent,
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

/// The values one provider supplies, over the frames of the providers around
/// it.
pub(crate) struct LocalFrame {
    values: SmallVec<[(LocalKey, Rc<dyn Any>); 2]>,
    parent: LocalStackSnapshot,
}

/// The entry the innermost provider in `stack` supplies for `key`.
pub(crate) fn provided_entry(stack: &LocalStackSnapshot, key: &LocalKey) -> Option<Rc<dyn Any>> {
    let mut frame = stack.as_deref();
    while let Some(current) = frame {
        if let Some((_, entry)) = current.values.iter().find(|(provided, _)| provided == key) {
            return Some(Rc::clone(entry));
        }
        frame = current.parent.as_deref();
    }
    None
}

pub(crate) struct ComposerCore {
    pub(crate) shared_state: Rc<ComposerRuntimeState>,
    /// The branch fold stack of the runtime state, which its nested
    /// composers share.
    pub(crate) branch_folds: Rc<crate::slot::BranchFolds>,
    pub(crate) slots: Rc<SlotsHost>,
    slot_hosts: RefCell<Vec<Rc<SlotsHost>>>,
    pub(crate) applier: Rc<dyn ApplierHost>,
    pub(crate) runtime: RuntimeHandle,
    pub(crate) parent_stack: RefCell<Vec<ParentFrame>>,
    pub(crate) subcompose_stack: RefCell<Vec<SubcomposeFrame>>,
    pub(crate) root: Cell<Option<NodeId>>,
    pub(crate) commands: RefCell<CommandQueue>,
    pub(crate) scope_stack: RefCell<Vec<RecomposeScope>>,
    subcomposition_owner_scope: RefCell<Option<RecomposeScope>>,
    pub(crate) local_stack: RefCell<LocalStackSnapshot>,
    pub(crate) side_effects: RefCell<Vec<Box<dyn FnOnce()>>>,
    pub(crate) pending_scope_options: Cell<Option<RecomposeOptions>>,
    pub(crate) phase: Cell<crate::Phase>,
    pub(crate) recompose_parent_hint: Cell<Option<NodeId>>,
    pub(crate) recompose_child_cursor: Cell<crate::recompose::RecomposeChildCursor>,
    pub(crate) root_render_requested: Cell<bool>,
    /// Whether a static composition local provided around what composes
    /// now changed: every group opened meanwhile runs its body.
    locals_changed: Cell<bool>,
    pub(crate) _not_send: PhantomData<*const ()>,
}

/// The composition context inherited by work that is composed in another slot
/// host. Besides composition locals, this carries the source owner scope so a
/// secondary tree cannot outlive the composition that supplied its callbacks.
#[derive(Clone)]
pub struct CapturedCompositionContext {
    locals: LocalStackSnapshot,
    owner_scope: Option<Weak<RecomposeScopeInner>>,
    locals_changed: bool,
}

impl CapturedCompositionContext {
    /// Whether a static composition local provided around the capture site
    /// changed in the composition that captured this context: what composes
    /// under it, subcompositions included, must compose again without
    /// skipping.
    pub fn locals_changed(&self) -> bool {
        self.locals_changed
    }

    /// Total deactivations along the capturing scope's owner chain right now;
    /// see [`crate::RecomposeScope::owner_chain_deactivation_epoch`]. Zero
    /// when the context has no owner scope or it is gone.
    pub fn owner_chain_deactivation_epoch(&self) -> u64 {
        self.owner_scope
            .as_ref()
            .and_then(Weak::upgrade)
            .map_or(0, |inner| {
                crate::RecomposeScope { inner }.owner_chain_deactivation_epoch()
            })
    }
}

fn take_subcompose_frame(core: &ComposerCore, operation: &str) -> SubcomposeFrame {
    match core.subcompose_stack.borrow_mut().pop() {
        Some(frame) => frame,
        None => {
            log::error!("subcompose stack underflow while finishing {operation}");
            SubcomposeFrame::default()
        }
    }
}

struct SubcomposeStackGuard {
    core: Rc<ComposerCore>,
    leaked: bool,
}

impl Drop for SubcomposeStackGuard {
    fn drop(&mut self) {
        if !self.leaked {
            self.core.subcompose_stack.borrow_mut().pop();
        }
    }
}

impl Drop for ComposerCore {
    /// A pass that queued nothing never takes its queue: hand its storage
    /// back for the next pass.
    fn drop(&mut self) {
        let commands = std::mem::take(self.commands.get_mut());
        if commands.len() == 0 && commands.capacity() > 0 {
            recycle_commands(commands);
        }
        let stacks = self.take_stacks();
        if stacks.holds_storage() {
            recycle_composer_stacks(stacks);
        }
    }
}

impl ComposerCore {
    fn new(parts: ComposerParts) -> Self {
        let ComposerParts {
            shared_state,
            slots,
            applier,
            runtime,
            root,
            initial_parent_frame,
        } = parts;
        let ComposerStacks {
            slot_hosts,
            mut parent_stack,
            scope_stack,
        } = spare_composer_stacks();
        push_root_frame(&mut parent_stack, root, initial_parent_frame);
        Self {
            branch_folds: Rc::clone(&shared_state.branch_folds),
            shared_state,
            slots,
            slot_hosts: RefCell::new(slot_hosts),
            applier,
            runtime,
            parent_stack: RefCell::new(parent_stack),
            subcompose_stack: RefCell::new(Vec::new()),
            root: Cell::new(root),
            commands: RefCell::new(spare_commands()),
            scope_stack: RefCell::new(scope_stack),
            subcomposition_owner_scope: RefCell::new(None),
            local_stack: RefCell::new(None),
            side_effects: RefCell::new(Vec::new()),
            pending_scope_options: Cell::new(None),
            phase: Cell::new(crate::Phase::Compose),
            recompose_parent_hint: Cell::new(None),
            recompose_child_cursor: Cell::new(crate::recompose::RecomposeChildCursor::Unknown),
            root_render_requested: Cell::new(false),
            locals_changed: Cell::new(false),
            _not_send: PhantomData,
        }
    }

    /// The stacks, emptied, with their storage.
    fn take_stacks(&mut self) -> ComposerStacks {
        let mut stacks = ComposerStacks {
            slot_hosts: std::mem::take(self.slot_hosts.get_mut()),
            parent_stack: std::mem::take(self.parent_stack.get_mut()),
            scope_stack: std::mem::take(self.scope_stack.get_mut()),
        };
        stacks.slot_hosts.clear();
        stacks.parent_stack.clear();
        stacks.scope_stack.clear();
        stacks
    }

    /// Becomes the core of a new pass, in place, as [`Self::new`] builds
    /// one: the allocation and the stacks' storage stay. Every field is
    /// named, so a new field cannot be left out.
    fn restart(&mut self, parts: ComposerParts) {
        let Self {
            shared_state,
            branch_folds,
            slots,
            slot_hosts,
            applier,
            runtime,
            parent_stack,
            subcompose_stack,
            root,
            commands,
            scope_stack,
            subcomposition_owner_scope,
            local_stack,
            side_effects,
            pending_scope_options,
            phase,
            recompose_parent_hint,
            recompose_child_cursor,
            root_render_requested,
            locals_changed,
            _not_send: _,
        } = self;
        if !Rc::ptr_eq(shared_state, &parts.shared_state) {
            *branch_folds = Rc::clone(&parts.shared_state.branch_folds);
            *shared_state = parts.shared_state;
        }
        *slots = parts.slots;
        *applier = parts.applier;
        *runtime = parts.runtime;
        slot_hosts.get_mut().clear();
        let parent_stack = parent_stack.get_mut();
        parent_stack.clear();
        push_root_frame(parent_stack, parts.root, parts.initial_parent_frame);
        subcompose_stack.get_mut().clear();
        root.set(parts.root);
        let commands = commands.get_mut();
        if commands.len() > 0 || commands.capacity() == 0 {
            *commands = spare_commands();
        }
        scope_stack.get_mut().clear();
        *subcomposition_owner_scope.get_mut() = None;
        *local_stack.get_mut() = None;
        side_effects.get_mut().clear();
        pending_scope_options.set(None);
        phase.set(crate::Phase::Compose);
        recompose_parent_hint.set(None);
        recompose_child_cursor.set(crate::recompose::RecomposeChildCursor::Unknown);
        root_render_requested.set(false);
        locals_changed.set(false);
    }

    /// Lets go of what the finished pass composed under: its stacks, locals,
    /// owner scope and side effects.
    fn park(&mut self) {
        self.slot_hosts.get_mut().clear();
        self.parent_stack.get_mut().clear();
        self.scope_stack.get_mut().clear();
        self.subcompose_stack.get_mut().clear();
        *self.subcomposition_owner_scope.get_mut() = None;
        *self.local_stack.get_mut() = None;
        self.side_effects.get_mut().clear();
    }
}

/// Opens the parent frame of a pass's top-level nodes under `root`.
fn push_root_frame(
    parent_stack: &mut Vec<ParentFrame>,
    root: Option<NodeId>,
    initial_parent_frame: InitialParentFrame,
) {
    if let Some(root_id) = root {
        parent_stack.push(ParentFrame {
            id: root_id,
            previous: ChildList::new(),
            new_children: ChildList::new(),
            new_children_membership: None,
            attach_mode: ParentAttachMode::DeferredSync,
            synthetic_root: matches!(initial_parent_frame, InitialParentFrame::SyntheticRoot),
        });
    }
}

/// What a composer core is built for: the tree it composes and the
/// parent its top-level nodes attach under.
struct ComposerParts {
    shared_state: Rc<ComposerRuntimeState>,
    slots: Rc<SlotsHost>,
    applier: Rc<dyn ApplierHost>,
    runtime: RuntimeHandle,
    root: Option<NodeId>,
    initial_parent_frame: InitialParentFrame,
}

/// The state of one composition pass. It writes the slot table and queues node changes for the
/// applier. Clones share the same pass. Composable functions reach the current composer through
/// `with_current_composer`.
#[derive(Clone)]
pub struct Composer {
    pub(crate) core: Rc<ComposerCore>,
}

/// Keeps a branch's key in the fold of the groups composed inside it, for
/// code `#[composable]` expands to.
#[doc(hidden)]
pub struct BranchGroupGuard<'a> {
    folds: &'a crate::slot::BranchFolds,
    token: usize,
}

impl Drop for BranchGroupGuard<'_> {
    #[inline]
    fn drop(&mut self) {
        self.folds.close(self.token);
    }
}

/// A [`BranchGroupGuard`] for code that reaches the composer through the
/// thread's current one, as a content closure does.
#[doc(hidden)]
pub struct ScopedBranchGroupGuard {
    folds: Rc<crate::slot::BranchFolds>,
    token: usize,
}

impl ScopedBranchGroupGuard {
    #[inline]
    pub(crate) fn open(folds: &Rc<crate::slot::BranchFolds>, key: Key) -> Self {
        Self {
            token: folds.push(key),
            folds: Rc::clone(folds),
        }
    }
}

impl Drop for ScopedBranchGroupGuard {
    #[inline]
    fn drop(&mut self) {
        self.folds.close(self.token);
    }
}

/// A group a composable call opened; [`ComposableGroup::close`] ends it
/// after the call's body.
#[doc(hidden)]
pub struct ComposableGroup<'a> {
    composer: &'a Composer,
    host: Rc<SlotsHost>,
    scope: RecomposeScope,
    pass: Option<SlotHostPassGuard>,
    runs_body: bool,
    completed: bool,
    skipped: Cell<bool>,
}

impl ComposableGroup<'_> {
    /// The recompose scope of the group.
    pub fn scope(&self) -> &RecomposeScope {
        &self.scope
    }

    /// Registers `callback` as the code that recomposes the group's scope,
    /// as [`Composer::set_recompose_callback`] does for the current scope.
    pub fn set_recompose_callback<F>(&self, callback: F)
    where
        F: FnMut(&Composer) + 'static,
    {
        self.scope.set_recompose_callback(callback);
    }

    /// Skips the group's body: what it composed last stays. The call
    /// composes nothing more in the group before closing it.
    pub fn skip(&self) {
        self.skipped.set(true);
    }

    /// Ends the group after its body composed.
    #[inline]
    pub fn close(mut self) {
        self.completed = true;
    }
}

impl Drop for ComposableGroup<'_> {
    fn drop(&mut self) {
        if self.completed {
            self.scope.mark_composed_once();
        }
        self.scope
            .end_reads(self.completed & self.runs_body & !self.skipped.get());
        if self.skipped.get() {
            self.composer
                .close_skipped_group_in_active_pass(&self.host, &self.scope);
        } else {
            self.composer
                .close_group_in_active_pass(&self.host, &self.scope);
        }
        if let Some(mut pass) = self.pass.take() {
            if self.completed
                && let Err(err) = self.composer.finish_slot_host_pass(&pass.host)
            {
                log::error!("slot host pass finalization failed: {err}");
            }
            pass.close();
        }
    }
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
        root: Option<NodeId>,
    ) -> Self {
        Self::new_with_shared_state_with_parent_frame(
            shared_state,
            slots,
            applier,
            runtime,
            root,
            InitialParentFrame::SyntheticRoot,
        )
    }

    fn new_with_shared_state_with_parent_frame(
        shared_state: Rc<ComposerRuntimeState>,
        slots: Rc<SlotsHost>,
        applier: Rc<dyn ApplierHost>,
        runtime: RuntimeHandle,
        root: Option<NodeId>,
        initial_parent_frame: InitialParentFrame,
    ) -> Self {
        shared_state.bind_applier_host(&applier);
        let slots = bind_slots_host_to_runtime_state(&shared_state, &slots);
        let core = Rc::new(ComposerCore::new(ComposerParts {
            shared_state,
            slots,
            applier,
            runtime,
            root,
            initial_parent_frame,
        }));
        Self { core }
    }

    /// A composer for a pass, as [`Self::new_with_shared_state`] makes,
    /// built in place of `parked`'s core when nothing else holds it: the
    /// pass then allocates no core and reuses the stacks' storage.
    pub(crate) fn for_pass(
        parked: Option<Composer>,
        shared_state: Rc<ComposerRuntimeState>,
        slots: Rc<SlotsHost>,
        applier: Rc<dyn ApplierHost>,
        runtime: RuntimeHandle,
        root: Option<NodeId>,
    ) -> Self {
        let Some(mut composer) = parked.filter(|parked| Rc::strong_count(&parked.core) == 1) else {
            return Self::new_with_shared_state(shared_state, slots, applier, runtime, root);
        };
        let Some(core) = Rc::get_mut(&mut composer.core) else {
            return Self::new_with_shared_state(shared_state, slots, applier, runtime, root);
        };
        shared_state.bind_applier_host(&applier);
        let slots = bind_slots_host_to_runtime_state(&shared_state, &slots);
        core.restart(ComposerParts {
            shared_state,
            slots,
            applier,
            runtime,
            root,
            initial_parent_frame: InitialParentFrame::SyntheticRoot,
        });
        composer
    }

    /// The composer of a finished pass, kept for [`Self::for_pass`] when
    /// nothing else holds its core. It lets go of what the pass composed
    /// under.
    pub(crate) fn park(mut self) -> Option<Composer> {
        Rc::get_mut(&mut self.core)?.park();
        Some(self)
    }

    /// Creates a composer that writes into `slots` and queues node changes for `applier`. Top-level
    /// nodes attach under `root`, or have no parent when `root` is `None`.
    pub fn new(
        slots: Rc<SlotsHost>,
        applier: Rc<dyn ApplierHost>,
        runtime: RuntimeHandle,
        root: Option<NodeId>,
    ) -> Self {
        Self::new_with_shared_state_with_parent_frame(
            slots
                .runtime_state()
                .unwrap_or_else(|| Rc::new(ComposerRuntimeState::default())),
            slots,
            applier,
            runtime,
            root,
            InitialParentFrame::RealParent,
        )
    }

    pub(crate) fn from_core(core: Rc<ComposerCore>) -> Self {
        Self { core }
    }

    pub(crate) fn clone_core(&self) -> Rc<ComposerCore> {
        Rc::clone(&self.core)
    }

    pub(crate) fn request_root_render(&self) {
        self.core.root_render_requested.set(true);
    }

    pub(crate) fn take_root_render_request(&self) -> bool {
        self.core.root_render_requested.replace(false)
    }

    /// Returns the slot table host that receives this composer's writes: the innermost host set up
    /// for a subcomposition, otherwise the composition's own.
    pub fn active_slots_host(&self) -> Rc<SlotsHost> {
        self.core
            .slot_hosts
            .borrow()
            .last()
            .cloned()
            .unwrap_or_else(|| Rc::clone(&self.core.slots))
    }

    fn with_active_slots_host<R>(&self, f: impl FnOnce(&SlotsHost) -> R) -> R {
        let hosts = self.core.slot_hosts.borrow();
        f(hosts.last().unwrap_or(&self.core.slots))
    }

    pub(crate) fn with_slots<R>(&self, f: impl FnOnce(&SlotTable) -> R) -> R {
        self.with_active_slots_host(|host| f(&host.borrow()))
    }

    pub(crate) fn with_slots_mut<R>(&self, f: impl FnOnce(&mut SlotTable) -> R) -> R {
        self.with_active_slots_host(|host| f(&mut host.borrow_mut()))
    }

    pub(crate) fn with_slot_session_mut<R>(
        &self,
        f: impl FnOnce(&mut crate::slot::SlotWriteSession<'_>) -> R,
    ) -> R {
        self.with_active_slots_host(|host| host.with_write_session(f))
    }

    pub(crate) fn try_with_slot_host_pass<R>(
        &self,
        slots: Rc<SlotsHost>,
        mode: crate::slot::SlotPassMode,
        f: impl FnOnce(&Composer) -> R,
    ) -> Result<(R, SlotPassOutcome), NodeError> {
        let mut guard = self.begin_slot_host_pass(&slots, mode);
        let result = f(self);
        let outcome = self.finish_slot_host_pass(&guard.host)?;
        guard.close();
        Ok((result, outcome))
    }

    pub(crate) fn with_slot_host_pass<R>(
        &self,
        slots: Rc<SlotsHost>,
        mode: crate::slot::SlotPassMode,
        f: impl FnOnce(&Composer) -> R,
    ) -> (R, SlotPassOutcome) {
        let mut guard = self.begin_slot_host_pass(&slots, mode);
        let result = f(self);
        let outcome = match self.finish_slot_host_pass(&guard.host) {
            Ok(outcome) => outcome,
            Err(err) => {
                log::error!("slot host pass finalization failed: {err}");
                SlotPassOutcome::default()
            }
        };
        guard.close();
        (result, outcome)
    }

    pub(crate) fn with_slot_override<R>(
        &self,
        slots: Rc<SlotsHost>,
        f: impl FnOnce(&Composer) -> R,
    ) -> (R, SlotPassOutcome) {
        self.with_slot_host_pass(slots, crate::slot::SlotPassMode::Compose, f)
    }

    fn begin_slot_host_pass(
        &self,
        slots: &Rc<SlotsHost>,
        mode: crate::slot::SlotPassMode,
    ) -> SlotHostPassGuard {
        let slots = bind_slots_host_to_runtime_state(&self.core.shared_state, slots);
        let fold_base = self.core.branch_folds.len();
        slots.begin_pass(mode, &self.core.branch_folds);
        {
            let mut stack = self.core.slot_hosts.borrow_mut();
            if let Some(parent) = stack.last()
                && !Rc::ptr_eq(parent, &slots)
            {
                parent.note_nested_host(&slots);
            }
            stack.push(Rc::clone(&slots));
        }
        SlotHostPassGuard {
            core: self.clone_core(),
            host: slots,
            fold_base,
            active: true,
        }
    }

    fn finish_slot_host_pass(&self, slots: &Rc<SlotsHost>) -> Result<SlotPassOutcome, NodeError> {
        let finished = {
            let mut applier = self.core.applier.borrow_dyn();
            slots.finish_pass(&mut *applier)
        }?;
        self.handle_detached_children_in_host(slots, None, finished.detached_root_children)?;
        self.wake_sites_whose_movable_arrived();
        self.evict_retained_subtrees_for_host(slots)?;
        slots.complete_pass_cleanup(&finished.outcome);
        Ok(finished.outcome)
    }

    fn wake_sites_whose_movable_arrived(&self) {
        for host in self.core.shared_state.hosts_awaiting_movables() {
            self.wake_sites_in_host(&host);
        }
    }

    fn wake_sites_in_host(&self, slots: &Rc<SlotsHost>) {
        let pending = self.core.shared_state.take_pending_movables(slots);
        if pending.is_empty() {
            return;
        }
        let mut waiting = Vec::new();
        for site in pending {
            if !slots.borrow().group_is_active(site.placeholder) {
                continue;
            }
            let retain_key = RetainKey::for_group(None, site.key);
            if !self.core.shared_state.movable_retained_anywhere(retain_key) {
                waiting.push(site);
                continue;
            }
            match site.parent_scope.as_ref().and_then(RecomposeScope::upgrade) {
                Some(scope) => {
                    scope.force_recompose();
                    scope.invalidate();
                }
                None => log::error!(
                    "movable content {:?} arrived for a site whose scope is gone",
                    site.key
                ),
            }
        }
        self.core.shared_state.keep_pending_movables(slots, waiting);
    }

    pub(crate) fn forget_movables(&self, ids: &[Key]) -> Result<(), NodeError> {
        for id in ids {
            let Some((host, subtree)) = self.core.shared_state.take_retained_movable(*id) else {
                continue;
            };
            self.dispose_detached_subtree_in_host(&host, subtree)?;
            host.flush_pending_drops();
        }
        Ok(())
    }

    pub(crate) fn parent_stack(&self) -> RefMut<'_, Vec<ParentFrame>> {
        self.core.parent_stack.borrow_mut()
    }

    pub(crate) fn current_parent_hint(&self) -> Option<NodeId> {
        let stack = self.core.parent_stack.borrow();
        let stack_hint = stack
            .last()
            .and_then(|frame| (!frame.synthetic_root).then_some(frame.id));
        stack_hint.or_else(|| self.core.recompose_parent_hint.get())
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

    pub(crate) fn local_stack(&self) -> RefMut<'_, LocalStackSnapshot> {
        self.core.local_stack.borrow_mut()
    }

    pub(crate) fn current_local_stack(&self) -> LocalStackSnapshot {
        self.core.local_stack.borrow().clone()
    }

    pub(crate) fn side_effects_mut(&self) -> RefMut<'_, Vec<Box<dyn FnOnce()>>> {
        self.core.side_effects.borrow_mut()
    }

    pub(crate) fn borrow_applier(&self) -> ApplierGuard<'_, dyn Applier> {
        self.core.applier.borrow_dyn()
    }

    /// Records nodes whose retained subtrees a reused subcompose slot just
    /// rebound to new content.
    ///
    /// The rebinding recomposes inline during measure, so every repass it
    /// schedules is consumed by the layout pass already running — nothing
    /// else survives to tell the scoped scene update these subtrees changed,
    /// and a translate-only update would keep their stale layers (measured:
    /// a 60pt lazy scroll presented the old row's text at the new row's
    /// position). The structural-change set is the one channel drained
    /// after layout, so the rebound children ride it.
    pub fn record_rebound_slot_children(&self, children: &[NodeId]) {
        let mut applier = self.borrow_applier();
        for &child in children {
            applier.record_structural_change(child);
        }
    }

    /// Registers a virtual node in the Applier.
    ///
    /// Assigns the node's ID so its layout registry and invalidation metadata
    /// belong to the node stored in the applier.
    pub fn register_virtual_node(
        &self,
        node_id: NodeId,
        node: Box<dyn Node>,
    ) -> Result<(), NodeError> {
        let mut applier = self.borrow_applier();
        applier.insert_with_id(node_id, node)?;
        applier.get_mut(node_id)?.set_node_id(node_id);
        Ok(())
    }

    /// Detaches and disposes subcomposition roots evicted from their reuse pool.
    /// Applies pending composition commands before releasing the roots and their descendants.
    ///
    /// ```no_run
    /// # use cranpose_core::{Composer, NodeError, SubcomposeState};
    /// # fn finish(composer: &Composer, state: &mut SubcomposeState) -> Result<(), NodeError> {
    /// let evicted = state.finish_pass();
    /// composer.dispose_subcomposed_nodes(evicted)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn dispose_subcomposed_nodes(
        &self,
        disposal: crate::subcompose::SubcomposeDisposal,
    ) -> Result<(), NodeError> {
        if disposal.nodes.is_empty() && disposal.slot_hosts.is_empty() {
            return Ok(());
        }
        self.apply_pending_commands()?;
        let disposal = crate::NodeDisposal::from_subcomposed(disposal, &*self.borrow_applier());
        self.core.applier.dispose_nodes(disposal)
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

    /// Returns a node's parent, or `None` for a root node.
    ///
    /// Returns an error when the node is no longer present in the applier.
    pub fn node_parent(&self, node_id: NodeId) -> Result<Option<NodeId>, NodeError> {
        self.borrow_applier()
            .get_mut(node_id)
            .map(|node| node.parent())
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

    /// Whether any node needs measurement or placement before its cached layout can be reused.
    ///
    /// ```no_run
    /// # use cranpose_core::{Composer, NodeId};
    /// # fn cache_is_current(composer: &Composer, children: &[NodeId]) -> bool {
    /// !composer.nodes_need_relayout(children.iter().copied())
    /// # }
    /// ```
    pub fn nodes_need_relayout(&self, node_ids: impl IntoIterator<Item = NodeId>) -> bool {
        let mut applier = self.borrow_applier();
        node_ids.into_iter().any(|node_id| {
            applier
                .get_mut(node_id)
                .is_ok_and(|node| node.layout_dirty())
        })
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
        if let Some(frame) = parent_stack.last_mut()
            && matches!(frame.attach_mode, ParentAttachMode::DeferredSync)
        {
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

    /// Runs `f` with this composer as the current one and its runtime as the active runtime. Both
    /// are restored when `f` returns.
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

    pub(crate) fn flush_pending_commands_if_large(&self) -> Result<(), NodeError> {
        let queued = self.core.commands.borrow().len();
        if queued < COMMAND_FLUSH_THRESHOLD {
            return Ok(());
        }
        self.apply_pending_commands()
    }

    fn resolve_group_entry(
        &self,
        host: &Rc<SlotsHost>,
        seed: crate::slot::GroupKeySeed,
        parent_scope_id: Option<ScopeId>,
    ) -> GroupEntry {
        let key = host.with_write_session(|slots| slots.reserve_group_key(seed));
        let retain_key = RetainKey::for_group(parent_scope_id, key);
        let restored = self
            .core
            .shared_state
            .take_retained(host, retain_key, |subtree| {
                host.with_write_session(|slots| slots.retained_restore_ready(key, subtree))
            })
            .or_else(|| self.take_movable_from_another_table(host, retain_key, key))
            .map(Box::new);
        if restored.is_some() || !key.is_movable() {
            return GroupEntry {
                key,
                restored,
                placeholder_for: None,
            };
        }
        let attached_elsewhere = self.movable_attached_elsewhere(host, key);
        if !attached_elsewhere {
            return GroupEntry {
                key,
                restored: None,
                placeholder_for: None,
            };
        }
        let id = key.explicit_key.unwrap_or_default();
        let placeholder = host.with_write_session(|slots| {
            slots.reserve_group_key(crate::slot::GroupKeySeed::movable_placeholder(id))
        });
        GroupEntry {
            key: placeholder,
            restored: None,
            placeholder_for: Some(key),
        }
    }

    /// Movable content that was let go by a parent composed in another slot
    /// table — a `SubcomposeLayout`'s, or the one a subcomposition owns — is
    /// retained there, not here. Bring it over: the table that held it lets
    /// go of its anchors, and this one issues its own.
    fn take_movable_from_another_table(
        &self,
        host: &Rc<SlotsHost>,
        retain_key: RetainKey,
        key: crate::slot::GroupKey,
    ) -> Option<crate::slot::DetachedSubtree> {
        if !key.is_movable() {
            return None;
        }
        let (source, mut subtree) = self
            .core
            .shared_state
            .take_retained_movable_elsewhere(host, retain_key)?;
        source
            .borrow_mut()
            .invalidate_detached_subtree_anchors(&subtree);
        if host.with_write_session(|slots| slots.retained_restore_ready(key, &mut subtree)) {
            return Some(subtree);
        }
        log::error!(
            "movable content {key:?} could not be taken over by the slot table that asked for it"
        );
        if let Err(error) = self.dispose_detached_subtree_in_host(host, subtree) {
            log::error!("disposing movable content that could not move failed: {error}");
        }
        None
    }

    /// Whether the movable's content is attached to some parent that is not
    /// the one composing, in this slot table or in another.
    fn movable_attached_elsewhere(&self, host: &Rc<SlotsHost>, key: crate::slot::GroupKey) -> bool {
        if host.with_write_session(|slots| slots.movable_attached_elsewhere(key)) {
            return true;
        }
        let Some(id) = key.movable_id() else {
            return false;
        };
        self.core
            .shared_state
            .host_holding_movable(id)
            .is_some_and(|holder| !Rc::ptr_eq(&holder, host))
    }

    fn scope_for_started_group(
        &self,
        host: &SlotsHost,
        group: crate::slot::ActiveGroupId,
        scope: Option<RecomposeScope>,
    ) -> RecomposeScope {
        if let Some(scope) = scope {
            return scope;
        }
        let scope = RecomposeScope::new(self.runtime_handle());
        host.with_write_session(|slots| slots.set_group_scope(group, scope.clone()));
        scope
    }

    fn enter_group_scope(&self, scope_ref: &RecomposeScope, entry: GroupScopeEntry<'_>) {
        let GroupScopeEntry {
            options,
            start_kind,
            host,
            group,
            parent_hint,
        } = entry;
        // A group entered while its scope is inactive is reused content, as
        // when a lazy list gives a recycled row to another item: like Compose,
        // it composes again rather than skipping, which reactivates the
        // scopes beneath it.
        let reused = !scope_ref.is_active() && scope_ref.has_composed_once();
        scope_ref.reactivate();
        {
            let mut stack = self.scope_stack();
            match stack.last() {
                Some(parent_scope) => scope_ref.set_parent_scope(Some(parent_scope)),
                None => scope_ref.set_lifetime_owner_scope(
                    self.core.subcomposition_owner_scope.borrow().as_ref(),
                ),
            }
            stack.push(scope_ref.clone());
        }
        scope_ref.set_retention_mode(options.retention);

        // Content under a changed static composition local runs every body,
        // as restored content does.
        let restored = matches!(start_kind, GroupStartKind::Restored);
        let recompose = options.force_recompose | self.core.locals_changed.get() | restored;
        if recompose | (reused & !options.force_reuse) {
            scope_ref.force_recompose();
        } else if options.force_reuse {
            scope_ref.force_reuse();
        }

        scope_ref.set_slots_host(host);

        {
            let mut stack = self.subcompose_stack();
            if let Some(frame) = stack.last_mut() {
                frame.scopes.push(scope_ref.clone());
            }
        }

        scope_ref.snapshot_locals(&self.core.local_stack.borrow());
        if restored {
            reparent_restored_scopes(host, group, scope_ref.parent_hint(), parent_hint);
        }
        scope_ref.set_parent_hint(parent_hint);
    }

    fn start_group(
        &self,
        host: &Rc<SlotsHost>,
        seed: crate::slot::GroupKeySeed,
        parent_node: Option<NodeId>,
    ) -> (
        GroupStart<crate::slot::ActiveGroupId>,
        Option<crate::slot::GroupKey>,
    ) {
        if !self.core.shared_state.retains_any() {
            let started = host.with_write_session(|slots| {
                let key = slots.reserve_group_key(seed);
                (!key.is_movable()).then(|| slots.begin_group(key, None, parent_node))
            });
            if let Some(started) = started {
                return (started, None);
            }
        }
        let parent_scope_id = self
            .core
            .scope_stack
            .borrow()
            .last()
            .map(RecomposeScope::id);
        let GroupEntry {
            key,
            restored,
            placeholder_for,
        } = self.resolve_group_entry(host, seed, parent_scope_id);
        let started =
            host.with_write_session(|slots| slots.begin_group(key, restored, parent_node));
        (started, placeholder_for)
    }

    #[inline(never)]
    fn open_group_in_active_pass(
        &self,
        host: &Rc<SlotsHost>,
        key: crate::slot::GroupKeySeed,
    ) -> (RecomposeScope, bool) {
        let options = self.core.pending_scope_options.take().unwrap_or_default();
        let parent_node = self.current_parent_hint();
        let (
            GroupStart {
                group,
                anchor,
                scope,
                kind,
            },
            placeholder_for,
        ) = self.start_group(host, key, parent_node);
        let scope_ref = self.scope_for_started_group(host, group, scope);
        if let Some(movable_key) = placeholder_for {
            self.core.shared_state.record_pending_movable(
                host,
                PendingMovable {
                    key: movable_key,
                    placeholder: anchor,
                    parent_scope: self
                        .core
                        .scope_stack
                        .borrow()
                        .last()
                        .map(RecomposeScope::downgrade),
                },
            );
        }
        self.enter_group_scope(
            &scope_ref,
            GroupScopeEntry {
                options,
                start_kind: kind,
                host,
                group,
                parent_hint: parent_node,
            },
        );
        (scope_ref, placeholder_for.is_none())
    }

    #[inline(never)]
    fn close_group_in_active_pass(&self, host: &Rc<SlotsHost>, scope: &RecomposeScope) {
        let result = host.with_write_session(|slots| {
            let result = slots.finish_group_body_changes();
            slots.end_group();
            result
        });
        self.close_finished_group(host, scope, result);
        self.settle_closed_group(scope);
    }

    /// Closes a group whose body was skipped, in one slot session: its root
    /// nodes stay attached where they were.
    #[inline(never)]
    fn close_skipped_group_in_active_pass(&self, host: &Rc<SlotsHost>, scope: &RecomposeScope) {
        // Attaching a node reaches the parent stack and the applier, never
        // the slot table the session holds.
        host.with_write_session(|slots| slots.skip_and_end_group(|id| self.attach_to_parent(id)));
        self.pop_closed_scope(scope);
        self.settle_closed_group(scope);
    }

    fn settle_closed_group(&self, scope: &RecomposeScope) {
        scope.mark_recomposed();
        if let Err(err) = self.flush_pending_commands_if_large() {
            log::error!("mid-composition command flush failed: {err}");
        }
    }

    #[inline(never)]
    fn open_group(&self, key: crate::slot::GroupKeySeed) -> ComposableGroup<'_> {
        let host = self.active_slots_host();
        let (host, pass) = if host.has_active_pass() {
            (host, None)
        } else {
            let pass = self.begin_slot_host_pass(&host, crate::slot::SlotPassMode::Compose);
            (self.active_slots_host(), Some(pass))
        };
        let (scope, runs_body) = self.open_group_in_active_pass(&host, key);
        scope.begin_reads();
        ComposableGroup {
            composer: self,
            host,
            scope,
            pass,
            runs_body,
            completed: false,
            skipped: Cell::new(false),
        }
    }

    #[doc(hidden)]
    pub fn __open_composable_group(&self, key: Key) -> ComposableGroup<'_> {
        self.open_group(crate::slot::GroupKeySeed::unkeyed(key))
    }

    pub(crate) fn with_group_seed<R>(
        &self,
        key: crate::slot::GroupKeySeed,
        f: impl FnOnce(&Composer) -> R,
    ) -> R {
        let group = self.open_group(key);
        debug_assert!(group.runs_body, "only movable content waits for a body");
        let result = f(self);
        group.close();
        result
    }

    pub(crate) fn with_movable_group(&self, id: Key, f: impl FnOnce(&Composer)) {
        let group = self.open_group(crate::slot::GroupKeySeed::movable(id));
        if group.runs_body {
            f(self);
        }
        group.close();
    }

    /// Runs `f` in a group identified by `key`. Slots and nodes that `f` creates belong to the
    /// group.
    pub fn with_group<R>(&self, key: Key, f: impl FnOnce(&Composer) -> R) -> R {
        self.with_group_seed(crate::slot::GroupKeySeed::unkeyed(key), f)
    }

    /// Runs `f` in a group identified by `key` that is retained when it leaves the composition,
    /// with the recompose `options` applied to its scope. A retained group returns with its state
    /// and nodes when `key` is composed again.
    pub fn cranpose_with_reuse<R>(
        &self,
        key: Key,
        mut options: RecomposeOptions,
        f: impl FnOnce(&Composer) -> R,
    ) -> R {
        options.retention = RetentionMode::RetainWhenInactive;
        self.core.pending_scope_options.set(Some(options));
        self.with_group(key, f)
    }

    /// Runs `f` in a group identified by the hash of `key`, so state and nodes follow the key when
    /// siblings move. Compose's `key`.
    #[track_caller]
    pub fn with_key<K: Hash, R>(&self, key: &K, f: impl FnOnce(&Composer) -> R) -> R {
        let seed = explicit_group_key_seed(key, std::panic::Location::caller());
        self.with_group_seed(seed, f)
    }

    #[doc(hidden)]
    pub fn __branch_group_deferred(&self, key: Key) -> BranchGroupGuard<'_> {
        let folds = &*self.core.branch_folds;
        BranchGroupGuard {
            token: folds.push(key),
            folds,
        }
    }

    fn dispose_detached_nodes(&self, nodes: impl IntoIterator<Item = NodeId>) {
        for node_id in nodes {
            self.commands_mut().push(Command::callback(move |applier| {
                crate::slot::dispose_detached_node_now(applier, node_id)
            }));
        }
    }

    fn detached_root_parent_commands(
        &self,
        subtree: &crate::slot::DetachedSubtree,
        context: &'static str,
    ) -> Result<Vec<(NodeId, Option<NodeId>)>, NodeError> {
        let mut root_nodes = Vec::new();
        subtree.collect_root_nodes_checked_into(&mut root_nodes, context);
        let mut roots = Vec::with_capacity(root_nodes.len());
        for root in root_nodes {
            let parent_id = {
                let mut applier = self.borrow_applier();
                applier.get_mut(root)?.parent()
            };
            roots.push((root, parent_id));
        }
        Ok(roots)
    }

    fn retain_detached_subtree_in_host(
        &self,
        slots_host: &Rc<SlotsHost>,
        parent_scope: Option<ScopeId>,
        subtree: crate::slot::DetachedSubtree,
    ) -> Result<(), NodeError> {
        let Some(root_key) = subtree.root_key_checked() else {
            log::error!("retention rejected detached subtree without a root group");
            self.dispose_detached_subtree_in_host(slots_host, subtree)?;
            return Ok(());
        };
        let root_detaches = self.detached_root_parent_commands(&subtree, "retention")?;
        subtree.scopes().for_each(RecomposeScope::deactivate);
        for (root, parent_id) in root_detaches {
            if let Some(parent_id) = parent_id {
                self.commands_mut().push(Command::DetachChild {
                    parent_id,
                    child_id: root,
                });
            }
        }
        let evicted = self.core.shared_state.insert_retained(
            slots_host,
            RetainKey::for_group(parent_scope, root_key),
            subtree,
            root_key.is_movable(),
        );
        for subtree in evicted {
            self.dispose_detached_subtree_in_host(slots_host, subtree)?;
        }
        Ok(())
    }

    fn evict_retained_subtrees_for_host(
        &self,
        slots_host: &Rc<SlotsHost>,
    ) -> Result<(), NodeError> {
        let evicted = self.core.shared_state.advance_retention_pass(slots_host);
        for subtree in evicted {
            self.dispose_detached_subtree_in_host(slots_host, subtree)?;
        }
        Ok(())
    }

    fn dispose_detached_subtree_in_host(
        &self,
        slots_host: &Rc<SlotsHost>,
        subtree: crate::slot::DetachedSubtree,
    ) -> Result<(), NodeError> {
        let root_nodes = self
            .detached_root_parent_commands(&subtree, "disposal")?
            .into_iter()
            .map(|(root, _)| root);
        subtree.scopes().for_each(RecomposeScope::deactivate);
        self.dispose_detached_nodes(root_nodes);
        slots_host.with_table_and_lifecycle_mut(|table, lifecycle| {
            table.invalidate_detached_subtree_anchors(&subtree);
            lifecycle.queue_subtree_disposal(subtree);
        });
        Ok(())
    }

    fn handle_detached_children_in_host(
        &self,
        slots_host: &Rc<SlotsHost>,
        parent_scope: Option<ScopeId>,
        detached: Vec<crate::slot::DetachedSubtree>,
    ) -> Result<(), NodeError> {
        for mut subtree in detached {
            for movable in subtree.split_off_nested_movables() {
                self.retain_detached_subtree_in_host(slots_host, None, movable)?;
            }
            if subtree
                .root_key_checked()
                .is_some_and(crate::slot::GroupKey::is_movable)
            {
                self.retain_detached_subtree_in_host(slots_host, None, subtree)?;
                continue;
            }
            let retention_mode = subtree
                .root_scope()
                .map(RecomposeScope::retention_mode)
                .unwrap_or_default();
            match retention_mode {
                RetentionMode::DisposeWhenInactive => {
                    self.dispose_detached_subtree_in_host(slots_host, subtree)?;
                }
                RetentionMode::RetainWhenInactive => {
                    self.retain_detached_subtree_in_host(slots_host, parent_scope, subtree)?;
                }
            }
        }
        Ok(())
    }

    fn handle_finished_group_result(
        &self,
        host: &Rc<SlotsHost>,
        parent_scope: Option<ScopeId>,
        result: FinishGroupResult,
    ) {
        let FinishGroupResult {
            detached_children,
            direct_nodes,
            root_nodes,
            was_skipped,
        } = result;
        if was_skipped {
            self.attach_root_nodes(root_nodes);
        }
        self.dispose_detached_nodes(direct_nodes);
        if detached_children.is_empty() {
            return;
        }
        if let Err(err) =
            self.handle_detached_children_in_host(host, parent_scope, detached_children)
        {
            log::error!("detached subtree handling failed while closing a group: {err}");
        }
    }

    pub(crate) fn close_current_group_body_for_scope(&self, scope: &RecomposeScope) {
        self.close_group_body(&self.active_slots_host(), scope);
    }

    fn close_group_body(&self, host: &Rc<SlotsHost>, scope: &RecomposeScope) {
        #[expect(
            clippy::redundant_closure_for_method_calls,
            reason = "the method path is not general over the session lifetime"
        )]
        let result = host.with_write_session(|slots| slots.finish_group_body_changes());
        self.close_finished_group(host, scope, result);
    }

    /// Handles what a group's body left, when it left anything, and pops
    /// the group's scope.
    fn close_finished_group(
        &self,
        host: &Rc<SlotsHost>,
        scope: &RecomposeScope,
        result: Option<FinishGroupResult>,
    ) {
        if let Some(result) = result {
            self.handle_finished_group_result(host, Some(scope.id()), result);
        }
        self.pop_closed_scope(scope);
    }

    fn pop_closed_scope(&self, scope: &RecomposeScope) {
        if let Some(popped) = self.scope_stack().pop() {
            debug_assert_eq!(
                popped.id(),
                scope.id(),
                "closed scope must match the active scope stack"
            );
        } else {
            log::error!("scope stack underflow while closing scope {}", scope.id());
        }
    }

    /// Returns the value `init` made on the first composition at this call site. Later compositions
    /// return the same value until the call site leaves the composition.
    #[track_caller]
    pub fn remember<T: 'static>(&self, init: impl FnOnce() -> T) -> Owned<T> {
        self.remember_at(crate::caller_location_key(), init)
    }

    #[doc(hidden)]
    pub fn remember_at<T: 'static>(
        &self,
        source: crate::Key,
        init: impl FnOnce() -> T,
    ) -> Owned<T> {
        self.with_slot_session_mut(|slots| {
            slots.remember_with_kind(PayloadKind::Remember, source, init)
        })
    }

    #[track_caller]
    pub(crate) fn remember_internal<T: 'static>(
        &self,
        source_salt: crate::Key,
        init: impl FnOnce() -> T,
    ) -> Owned<T> {
        let source = crate::caller_location_key() ^ source_salt;
        self.with_slot_session_mut(|slots| {
            slots.remember_with_kind(PayloadKind::Internal, source, init)
        })
    }

    #[track_caller]
    /// The state of the effect written at `site`, in a slot of the current
    /// group: it leaves with the slot, and its drop ends the effect.
    pub(crate) fn remember_effect<T: Default + 'static>(&self, site: Key) -> Owned<T> {
        self.with_slot_session_mut(|slots| slots.remember_effect::<T>(site))
    }

    /// Reserves the next slot at the call site for a `T` that `init` makes on the first
    /// composition. Returns a handle for reading the value through `with_slot_value`.
    #[track_caller]
    pub fn use_value_slot<T: 'static>(&self, init: impl FnOnce() -> T) -> ValueSlotHandle<'_, T> {
        let source = crate::caller_location_key();
        let (slot, record) = self.with_slot_session_mut(|slots| {
            slots.located_value_slot(PayloadKind::Internal, source, init)
        });
        ValueSlotHandle::new(slot, record)
    }

    #[doc(hidden)]
    pub fn __use_param_slot<T: 'static>(
        &self,
        source: Key,
        init: impl FnOnce() -> T,
    ) -> ValueSlotHandle<'_, T> {
        let (slot, record) = self.with_slot_session_mut(|slots| {
            slots.located_value_slot(PayloadKind::Param, source, init)
        });
        ValueSlotHandle::new(slot, record)
    }

    /// Finds the call's next parameter slot and runs `update` on its value,
    /// with one slot lookup for both.
    #[doc(hidden)]
    pub fn __update_param_slot<T: 'static, R>(
        &self,
        source: Key,
        init: impl FnOnce() -> T,
        update: impl FnOnce(&mut T) -> R,
    ) -> (ValueSlotHandle<'_, T>, R) {
        self.update_value_slot(PayloadKind::Param, source, init, update)
    }

    /// Finds the call's return slot and runs `read` on its value, with one
    /// slot lookup for both.
    #[doc(hidden)]
    pub fn __update_return_slot<T: 'static, R>(
        &self,
        source: Key,
        init: impl FnOnce() -> T,
        read: impl FnOnce(&mut T) -> R,
    ) -> (ValueSlotHandle<'_, T>, R) {
        self.update_value_slot(PayloadKind::Return, source, init, read)
    }

    fn update_value_slot<T: 'static, R>(
        &self,
        kind: PayloadKind,
        source: Key,
        init: impl FnOnce() -> T,
        update: impl FnOnce(&mut T) -> R,
    ) -> (ValueSlotHandle<'_, T>, R) {
        self.with_slot_session_mut(|slots| {
            let (slot, record, value) = slots.value_slot_and_value(kind, source, init);
            (ValueSlotHandle::new(slot, record), update(value))
        })
    }

    #[doc(hidden)]
    #[track_caller]
    pub fn __use_return_slot<T: 'static>(
        &self,
        init: impl FnOnce() -> T,
    ) -> ValueSlotHandle<'_, T> {
        let source = crate::caller_location_key();
        let (slot, record) = self.with_slot_session_mut(|slots| {
            slots.located_value_slot(PayloadKind::Return, source, init)
        });
        ValueSlotHandle::new(slot, record)
    }

    #[doc(hidden)]
    pub fn __invalidate_return_consumer_scope(&self) {
        let Some(scope) = self.current_recompose_scope() else {
            self.request_root_render();
            return;
        };

        if let Some(target) = scope.callback_promotion_target() {
            target.invalidate();
        } else {
            self.request_root_render();
        }
    }

    /// Runs `f` with a shared reference to the value in the slot that `handle` names.
    pub fn with_slot_value<'pass, T: 'static, R>(
        &'pass self,
        handle: ValueSlotHandle<'pass, T>,
        f: impl FnOnce(&T) -> R,
    ) -> R {
        self.with_slots(|slots| f(slots.value_at(handle.slot, handle.record)))
    }

    /// Runs `f` with a mutable reference to the value in the slot that `handle` names.
    pub fn with_slot_value_mut<'pass, T: 'static, R>(
        &'pass self,
        handle: ValueSlotHandle<'pass, T>,
        f: impl FnOnce(&mut T) -> R,
    ) -> R {
        self.with_slots_mut(|slots| f(slots.value_at_mut(handle.slot, handle.record)))
    }

    /// Creates a [`MutableState`] holding `initial`, tied to this composer's runtime. It is not
    /// remembered, so each call makes a new state.
    pub fn mutable_state_of<T: Clone + 'static>(&self, initial: T) -> MutableState<T> {
        MutableState::with_runtime(initial, self.runtime_handle())
    }

    /// Creates a snapshot-aware list holding `values`, tied to this composer's runtime. It is not
    /// remembered, so each call makes a new list.
    pub fn mutable_state_list_of<T, I>(&self, values: I) -> SnapshotStateList<T>
    where
        T: Clone + 'static,
        I: IntoIterator<Item = T>,
    {
        SnapshotStateList::with_runtime(values, self.runtime_handle())
    }

    /// Creates a snapshot-aware map holding `pairs`, tied to this composer's runtime. It is not
    /// remembered, so each call makes a new map.
    pub fn mutable_state_map_of<K, V, I>(&self, pairs: I) -> SnapshotStateMap<K, V>
    where
        K: Clone + Eq + Hash + 'static,
        V: Clone + 'static,
        I: IntoIterator<Item = (K, V)>,
    {
        SnapshotStateMap::with_runtime(pairs, self.runtime_handle())
    }

    /// Returns the value of `local` from the innermost enclosing provider, or the local's default
    /// value when no provider is in scope.
    pub fn read_composition_local<T: Clone + 'static>(&self, local: &CompositionLocal<T>) -> T {
        self.read_provided(
            &local.key,
            |entry: &LocalStateEntry<T>| entry.value(),
            |entry: &ComputedLocalEntry<T>| entry.value(),
        )
        .unwrap_or_else(|| local.default_value())
    }

    /// Returns the value of `local` from the innermost enclosing provider, or the local's default
    /// value when no provider is in scope.
    pub fn read_static_composition_local<T: Clone + 'static>(
        &self,
        local: &StaticCompositionLocal<T>,
    ) -> T {
        let provided = local.provided_under(&self.core.local_stack.borrow());
        provided.unwrap_or_else(|deferred| deferred.value(local))
    }

    /// The value of the innermost provided entry for `key`, a plain value
    /// `V` or a computed one `C`; `None` without a provider. A computed value
    /// runs after the local stack is released, as it reads other locals.
    fn read_provided<V: 'static, C: 'static, T>(
        &self,
        key: &LocalKey,
        value: impl FnOnce(&V) -> T,
        computed: impl FnOnce(&C) -> T,
    ) -> Option<T> {
        let entry = provided_entry(&self.core.local_stack.borrow(), key)?;
        match entry.downcast::<V>() {
            Ok(entry) => Some(value(&entry)),
            Err(entry) => entry
                .downcast::<C>()
                .inspect_err(|_| {
                    log::error!("composition local entry type mismatch for key {key:?}");
                })
                .ok()
                .map(|entry| computed(&entry)),
        }
    }

    /// Returns the recompose scope being composed, or `None` outside any scope.
    pub fn current_recompose_scope(&self) -> Option<RecomposeScope> {
        self.core.scope_stack.borrow().last().cloned()
    }

    /// Notes a read of `state_id` in the scope that the read recomposes: the
    /// nearest one with a recompose callback. Returns that scope unless the
    /// read repeats the last body's, which the state already watches.
    pub(crate) fn note_state_read(
        &self,
        state_id: Option<crate::StateId>,
    ) -> Option<(RecomposeScope, crate::ScopeRead)> {
        let stack = self.core.scope_stack.borrow();
        let scope = stack
            .iter()
            .rev()
            .find(|scope| scope.has_recompose_callback())
            .or_else(|| stack.last())?;
        let read = state_id.map_or(crate::ScopeRead::NotReading, |id| scope.note_read(id));
        (!matches!(read, crate::ScopeRead::Watched)).then(|| (scope.clone(), read))
    }

    /// Returns the phase of the frame in which this composer is used.
    pub fn phase(&self) -> crate::Phase {
        self.core.phase.get()
    }

    pub(crate) fn set_phase(&self, phase: crate::Phase) {
        self.core.phase.set(phase);
    }

    /// Switches the composer to `phase`.
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
                "subcompose() may only be called during measure or layout; current phase: {current:?}"
            ),
        }

        self.subcompose_stack().push(SubcomposeFrame::default());
        let mut guard = SubcomposeStackGuard {
            core: self.clone_core(),
            leaked: false,
        };

        let slot_host = state.get_or_create_slots(slot_id);
        let (result, _) = self.with_slot_override(slot_host, |composer| {
            composer.with_group(slot_id.raw(), |composer| content(composer))
        });

        let frame = {
            let frame = take_subcompose_frame(&guard.core, "subcompose");
            guard.leaked = true;
            frame
        };
        let nodes = frame.nodes;
        let scopes = frame.scopes;
        state.register_active(slot_id, &nodes, &scopes);
        (result, nodes)
    }

    /// Composes `content` in the slot `slot_id` of `state` and returns its result with the ids of
    /// the nodes it produced that have no parent. Panics outside the measure and layout phases.
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

    fn spin_up_subcompose_core(
        &self,
        slots: &Rc<SlotsHost>,
        root: Option<NodeId>,
        runtime_handle: &RuntimeHandle,
        locals: LocalStackSnapshot,
    ) -> Rc<ComposerCore> {
        let phase = self.phase();
        let shared_state = slots
            .runtime_state()
            .unwrap_or_else(|| Rc::clone(&self.core.shared_state));
        let core = Rc::new(ComposerCore::new(ComposerParts {
            shared_state,
            slots: Rc::clone(slots),
            applier: Rc::clone(&self.core.applier),
            runtime: runtime_handle.clone(),
            root,
            initial_parent_frame: InitialParentFrame::RealParent,
        }));
        core.phase.set(phase);
        core.locals_changed.set(self.core.locals_changed.get());
        *core.local_stack.borrow_mut() = locals;
        core
    }

    /// Applies a subcomposition's commands. The pass holds a state teardown
    /// scope from its start until this returns, as a root pass does: the
    /// states a removed group remembered stay readable until the nodes it
    /// emitted are detached, whose modifier nodes may read them.
    fn flush_subcompose_pass(
        &self,
        mut commands: CommandQueue,
        runtime_handle: &RuntimeHandle,
        compact_applier: bool,
        side_effects: Vec<Box<dyn FnOnce()>>,
    ) -> Result<(), NodeError> {
        {
            let mut applier = self.borrow_applier();
            commands.apply(&mut *applier)?;
            for update in runtime_handle.take_updates() {
                update.apply(&mut *applier)?;
            }
        }
        recycle_commands(commands);
        if compact_applier {
            self.core.applier.compact();
            self.core.applier.borrow_dyn().clear_recycled_nodes();
        }
        composer_context::without_composer(|| {
            for effect in side_effects {
                effect();
            }
        });
        Ok(())
    }

    /// Composes `f` as an independent pass over the separate slot table `slots` and applies its
    /// node changes before returning. Top-level nodes from `f` attach under `root`, if any. The
    /// pass sees the composition locals in scope at the call.
    pub fn subcompose_in<R>(
        &self,
        slots: &Rc<SlotsHost>,
        root: Option<NodeId>,
        f: impl FnOnce(&Composer) -> R,
    ) -> Result<R, NodeError> {
        let _teardown = crate::runtime::enter_state_teardown_scope();
        let runtime_handle = self.runtime_handle();
        let locals = self.current_local_stack();
        let core = self.spin_up_subcompose_core(slots, root, &runtime_handle, locals);
        let composer = Composer::from_core(core);
        let (result, commands, side_effects, compact_applier) = composer.install(|composer| {
            let (output, outcome) = composer.try_with_slot_host_pass(
                Rc::clone(slots),
                crate::slot::SlotPassMode::Compose,
                |composer| f(composer),
            )?;
            let commands = composer.take_commands();
            let side_effects = composer.take_side_effects();
            Ok((output, commands, side_effects, outcome.compacted))
        })?;
        self.flush_subcompose_pass(commands, &runtime_handle, compact_applier, side_effects)?;
        Ok(result)
    }

    /// Captures the composition context at the current point so work composed
    /// in another slot host inherits both locals and source ownership.
    ///
    /// A `SubcomposeLayout` captures this while it is being composed and replays
    /// it while subcomposing off the measure pass, so content that is
    /// subcomposed during layout observes the same composition locals as the
    /// `SubcomposeLayout` call site — matching Jetpack Compose, where a
    /// subcomposition inherits the composition locals of the layout that
    /// created it rather than whatever happens to be in scope during measure
    /// (which, after composition unwinds, no longer carries ancestor providers).
    pub fn capture_composition_context(&self) -> CapturedCompositionContext {
        CapturedCompositionContext {
            locals: self.current_local_stack(),
            owner_scope: self
                .current_recompose_scope()
                .map(|scope| scope.downgrade()),
            locals_changed: self.core.locals_changed.get(),
        }
    }

    /// Runs `f` as content under a static composition local that changed
    /// when `changed`: every group it opens runs its body, as under the
    /// provider of a changed static local.
    pub fn with_locals_changed<R>(&self, changed: bool, f: impl FnOnce(&Composer) -> R) -> R {
        let outer = self.core.locals_changed.get();
        self.core.locals_changed.set(outer || changed);
        let result = f(self);
        self.core.locals_changed.set(outer);
        result
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
        let context = self.capture_composition_context();
        self.subcompose_slot_with_context(slots, root, &context, f)
    }

    /// Like [`Composer::subcompose_slot`], but uses a context captured at the
    /// source composition site. This is required for measure-time composition,
    /// where the source scope is no longer on the active stack.
    pub fn subcompose_slot_with_context<R>(
        &self,
        slots: &Rc<SlotsHost>,
        root: Option<NodeId>,
        context: &CapturedCompositionContext,
        f: impl FnOnce(&Composer) -> R,
    ) -> Result<(R, Vec<RecomposeScope>), NodeError> {
        let _teardown = crate::runtime::enter_state_teardown_scope();
        let runtime_handle = self.runtime_handle();
        let locals = context.locals.clone();
        let core = self.spin_up_subcompose_core(slots, root, &runtime_handle, locals);
        *core.subcomposition_owner_scope.borrow_mut() = context
            .owner_scope
            .as_ref()
            .and_then(RecomposeScope::upgrade);
        let composer = Composer::from_core(core);
        composer.subcompose_stack().push(SubcomposeFrame::default());
        let mut guard = SubcomposeStackGuard {
            core: composer.clone_core(),
            leaked: false,
        };
        let root_group_key = crate::location_key!();
        let (result, commands, side_effects, compact_applier) = composer.install(|composer| {
            let (output, outcome) = composer.try_with_slot_host_pass(
                Rc::clone(slots),
                crate::slot::SlotPassMode::Compose,
                |composer| {
                    let output = composer.with_group(root_group_key, |composer| f(composer));
                    if root.is_some() {
                        composer.pop_parent();
                    }
                    output
                },
            )?;
            let commands = composer.take_commands();
            let side_effects = composer.take_side_effects();
            Ok((output, commands, side_effects, outcome.compacted))
        })?;
        let frame = {
            let frame = take_subcompose_frame(&guard.core, "subcompose_slot");
            guard.leaked = true;
            frame
        };

        self.flush_subcompose_pass(commands, &runtime_handle, compact_applier, side_effects)?;
        Ok((result, frame.scopes))
    }

    fn attach_root_nodes(&self, root_nodes: RootNodeIds) {
        for id in root_nodes {
            self.attach_to_parent(id);
        }
    }

    /// Skips the currently open group without running it again. Its nodes and remembered values
    /// stay as the last composition left them.
    pub fn skip_current_group(&self) {
        #[expect(
            clippy::redundant_closure_for_method_calls,
            reason = "the method path is not general over the session lifetime"
        )]
        self.with_slot_session_mut(|slots| slots.skip_group());
    }

    /// Returns a handle to the runtime of this composer.
    pub fn runtime_handle(&self) -> RuntimeHandle {
        self.core.runtime.clone()
    }

    /// Registers `callback` as the code that recomposes the current scope when state it read
    /// changes. Does nothing outside a recompose scope.
    pub fn set_recompose_callback<F>(&self, callback: F)
    where
        F: FnMut(&Composer) + 'static,
    {
        if let Some(scope) = self.current_recompose_scope() {
            scope.set_recompose_callback(callback);
        }
    }

    /// Like `set_recompose_callback` for a plain function pointer.
    pub fn set_recompose_fn(&self, callback: fn(&Composer)) {
        if let Some(scope) = self.current_recompose_scope() {
            scope.set_boxed_recompose(Box::new(callback), None);
        }
    }

    /// Runs `f` with `provided` in scope over the locals around it; of values
    /// for one local, the last wins.
    pub fn with_composition_locals<R>(
        &self,
        provided: impl IntoIterator<Item = ProvidedValue>,
        site: crate::Key,
        f: impl FnOnce(&Composer) -> R,
    ) -> R {
        let provided: SmallVec<[ProvidedValue; 2]> = provided.into_iter().collect();
        if provided.is_empty() {
            return f(self);
        }
        let mut values = SmallVec::<[(LocalKey, Rc<dyn Any>); 2]>::new();
        let mut static_changed = false;
        for value in provided.into_iter().rev() {
            if values.iter().any(|(key, _)| key == value.key()) {
                continue;
            }
            let (key, entry, changed) = value.into_entry(self, site);
            static_changed |= changed;
            values.push((key, entry));
        }
        let parent = self.current_local_stack();
        *self.local_stack() = Some(Rc::new(LocalFrame {
            values,
            parent: parent.clone(),
        }));
        let result = self.with_locals_changed(static_changed, f);
        *self.local_stack() = parent;
        result
    }
}
