#![deny(missing_docs)]
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]

pub extern crate self as cranpose_core;

mod applier;
mod applier_host;
pub use applier::Applier;
mod blocking;
mod bubble_passes;
mod callbacks;
mod child_sync;
pub(crate) use child_sync::{
    apply_remove_child, attach_child_at, cleanup_detached_child, detach_child_from_parent,
    insert_child_with_reparenting, remove_child_and_cleanup_now, sync_or_settle_children,
};
mod command;
pub(crate) use command::{
    Command, CommandCallback, DeferredChildCleanup, DeferredChildCleanupQueue, DirtyBubble,
};
mod command_queue;
pub(crate) use command_queue::{
    COMMAND_FLUSH_THRESHOLD, ChildList, CommandQueue, SMALL_CHILD_SYNC_LINEAR_THRESHOLD,
};
#[cfg(test)]
pub(crate) use command_queue::{CommandTag, SyncChildrenCommand};
mod composer;
/// The composer that is current on this thread, which composable code reaches without passing it as
/// a parameter.
pub mod composer_context;
mod composition;
mod composition_locals;
mod computed_state;
pub mod concurrency;
mod debug_trace;
mod disposable_effect;
#[cfg(test)]
pub(crate) use disposable_effect::DisposableEffectState;
pub use disposable_effect::{
    __disposable_effect_impl, DisposableEffect, DisposableEffectResult, DisposableEffectScope,
    SideEffect,
};
mod effect_key;
mod emit;
pub mod env_flags;
#[cfg(any(feature = "internal", test))]
mod frame_clock;
mod hooks;
mod key_group;
pub use key_group::{key, with_key};
mod launched_effect;
mod layout_dirt_roots;
mod location_key;
mod memory_applier;
pub use memory_applier::{MemoryApplier, MemoryApplierDebugStats, SceneNodeAttachmentScratch};
mod movable_content;
mod node;
#[cfg(any(test, debug_assertions))]
pub(crate) use location_key::LocationKeyDebugInfo;
pub(crate) use location_key::{HotOrigin, avalanche_location_key};
#[cfg(test)]
pub(crate) use location_key::{
    hot_call_site_key, location_key_debug_collision_count_for_test,
    location_key_debug_info_for_test, register_location_key_debug_info_for_test,
};
pub use movable_content::{
    MovableContent, forget_movable, movable, movableContentOf, rememberMovableContentOf,
};
pub use node::{
    Node, bubble_layout_dirty, bubble_layout_dirty_in_composer, bubble_measure_dirty,
    bubble_measure_dirty_in_composer, bubble_semantics_dirty, bubble_semantics_dirty_in_composer,
    pop_parent, push_parent, queue_semantics_invalidation, with_node_mut,
};
mod node_error;
mod node_marks;
pub use node_error::NodeError;
/// `Owned`, the handle to a value that a composition remembers.
pub mod owned;
pub mod platform;
mod recompose;
mod recompose_scope;
pub(crate) use recompose_scope::{BodyRun, RecomposeScopeInner, ScopeRead};
pub use recompose_scope::{RecomposeOptions, RecomposeScope, RecomposeScopeRegistryDebugStats};
mod recycle_pool;
mod recycled_node;
use recycle_pool::RecyclePool;
pub use recycled_node::{RecycleOrigin, RecycledNode, RecycledNodeInsertion};
mod retention;
/// The runtime in which a composition runs: state cells, UI tasks, frame callbacks and the queue of
/// invalidated scopes.
pub mod runtime;
mod runtime_cell;
mod slot;
/// A min-heap of snapshot ids that supports removal by handle.
pub mod snapshot_double_index_heap;
/// A compact set of snapshot ids.
pub mod snapshot_id_set;
/// Tracks the pinned snapshots, which bound the state records that may be reclaimed.
pub mod snapshot_pinning;
/// Records which state objects a scope reads and calls back when one of them changes.
pub mod snapshot_state_observer;
pub mod snapshot_v2;
mod snapshot_weak_set;
pub mod source_trace;
mod stable_index;
mod state;
#[doc(hidden)]
pub use source_trace::__source_scope;
#[doc(hidden)]
pub use thread_state::with_or_fresh;
mod stage_times;
pub mod subcompose;
mod thread_state;
#[doc(hidden)]
pub use stage_times::{StageTimes, stage_ms};

#[cfg(feature = "internal")]
#[doc(hidden)]
pub mod internal {
    pub use crate::frame_clock::{FrameCallbackRegistration, FrameClock};
}
pub use applier_host::{ApplierGuard, ApplierHost, ConcreteApplierHost, NodeDisposal};
pub use blocking::{
    BlockingError, BlockingExecutor, BlockingExecutorConfig, BlockingTask, launchBlocking,
    withBlocking,
};
pub use callbacks::{
    CallbackHolder, CallbackHolder1, ParamReturnState, ParamSlot, ParamState, ReturnSlot,
    SharedParam, refresh_param, refresh_shared_param, store_param,
};
pub use composer::{
    __branch_group_scope_advance, __branch_group_scope_deferred, BranchGroupGuard,
    CapturedCompositionContext, ComposableGroup, Composer, ScopedBranchGroupGuard, ValueSlotHandle,
};
pub(crate) use composer::{ComposerCore, EmittedNode, ParentAttachMode, ParentFrame};
pub use composition::{Composition, ROOT_RENDER_REPLAY_LIMIT};
pub use composition_locals::{
    CompositionLocal, CompositionLocalAccessorScope, CompositionLocalProvider, ProvidedValue,
    ReadableCompositionLocal, StaticCompositionLocal, compositionLocalOf,
    compositionLocalOfWithPolicy, compositionLocalWithComputedDefaultOf, staticCompositionLocalOf,
    staticCompositionLocalOfWithPolicy, staticCompositionLocalWithComputedDefaultOf,
};
pub(crate) use composition_locals::{ComputedLocalEntry, LocalStateEntry};
#[doc(hidden)]
pub use computed_state::{ComputedState, computedStateOf};
pub use concurrency::{
    CollectEvents, CoroutineScope, Delay, EventChannel, EventSender, EventStream, EventStreamNext,
    ProduceScope, collectAsState, delay, interval, produceState, rememberCoroutineScope,
    rememberEventStream, spawn_ui_task,
};
#[doc(hidden)]
pub use debug_trace::{
    debug_label_current_scope, debug_live_recompose_scope_count,
    debug_recompose_scope_registry_stats, debug_scope_invalidation_sources, debug_scope_label,
};
pub use hooks::{
    derivedStateOf, mutableStateList, mutableStateListOf, mutableStateMap, mutableStateMapOf,
    mutableStateOf, mutableStateOfNeverEqual, ownedMutableStateOf, ownedMutableStateOfNeverEqual,
    remember, rememberKeyed, rememberMutableStateOf, rememberMutableStateOfNeverEqual,
    rememberUpdatedState, try_mutableStateOf,
};
#[cfg(feature = "internal")]
#[doc(hidden)]
pub use hooks::{withFrameMillis, withFrameNanos};
pub use launched_effect::{
    __launched_effect_async_impl, __launched_effect_impl, CancelToken, LaunchedEffect,
    LaunchedEffectAsync, LaunchedEffectScope, TaskSite,
};
pub use location_key::{
    __constant, HotOriginGuard, branch_location_key, cached_composable_definition_key,
    caller_location_key, composable_definition_key, composable_identity_key, const_location_key,
    constant_value_key, hot_branch_key, hot_definition_key, hot_origin, location_key,
    noted_location_key,
};
pub use node_marks::NodeMarks;
pub use owned::Owned;
pub use platform::{Clock, RuntimeScheduler, SchedulerRef, scheduler_ref};
pub use retention::{RetentionBudget, RetentionEvictionPolicy, RetentionMode, RetentionPolicy};
#[doc(hidden)]
pub use runtime::{
    DefaultScheduler, Runtime, RuntimeHandle, StateId, TaskHandle, UiDispatcher,
    current_runtime_handle, label_next_ui_task, schedule_frame, schedule_node_update,
};
pub use runtime_cell::RuntimeCell;
pub use slot::{
    SlotDebugAnchor, SlotDebugEntry, SlotDebugEntryKind, SlotDebugGroup, SlotDebugScope,
    SlotDebugSnapshot, SlotRetentionDebugStats, SlotTable, SlotTableDebugStats,
    SlotTableLocalDebugStats, SlotTableMutationDebugStats,
};
pub use snapshot_state_observer::SnapshotStateObserver;

/// Runs the provided closure inside a mutable snapshot and applies the result.
///
/// Use this function when you need to update `MutableState` from outside the
/// composition or layout phase, typically in event handlers or async tasks.
///
/// # Why is this needed?
/// Cranpose uses a snapshot system (MVCC) to isolate state changes. Modifications
/// made to `MutableState` are only visible to the current thread's active snapshot.
/// To make changes visible to the rest of the system (and trigger recomposition),
/// they must be "applied" by committing the snapshot. This helper handles that
/// lifecycle for you.
///
/// # Example
///
/// ```ignore
/// // Inside a button click handler
/// run_in_mutable_snapshot(|| {
///     count.set(count.value() + 1);
/// });
/// ```
///
/// # Important
/// ALL UI event handlers (keyboard, mouse, touch, animations) that modify state
/// MUST use this function or [`dispatch_ui_event`].
pub fn run_in_mutable_snapshot<T>(block: impl FnOnce() -> T) -> Result<T, &'static str> {
    let snapshot = snapshot_v2::take_mutable_snapshot(None, None);

    let _applied_guard = AppliedSnapshotFlagGuard::enter();
    let value = snapshot.enter(block);

    match snapshot.apply() {
        snapshot_v2::SnapshotApplyResult::Success => Ok(value),
        snapshot_v2::SnapshotApplyResult::Failure => Err("Snapshot apply failed"),
    }
}

struct AppliedSnapshotFlagGuard {
    previous: bool,
}

impl AppliedSnapshotFlagGuard {
    fn enter() -> Self {
        let previous = thread_state::cells(|cells| cells.in_applied_snapshot.replace(true));
        Self { previous }
    }
}

impl Drop for AppliedSnapshotFlagGuard {
    fn drop(&mut self) {
        thread_state::cells(|cells| cells.in_applied_snapshot.set(self.previous));
    }
}

/// Dispatches a UI event in a proper snapshot context.
///
/// This is a convenience wrapper around [`run_in_mutable_snapshot`] that returns
/// `Option<T>` instead of `Result<T, &str>`.
///
/// # Example
/// ```ignore
/// // In a keyboard event handler:
/// dispatch_ui_event(|| {
///     text_field_state.edit(|buffer| {
///         buffer.insert("a");
///     });
/// });
/// ```
pub fn dispatch_ui_event<T>(block: impl FnOnce() -> T) -> Option<T> {
    run_in_mutable_snapshot(block).ok()
}

/// The largest sizes that the command queues and the side effect list reached during one
/// composition pass, for debugging memory use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompositionPassDebugStats {
    /// Most node commands queued at once.
    pub commands_len: usize,
    /// Largest capacity reserved for node commands.
    pub commands_cap: usize,
    /// Most bytes of command arguments queued at once.
    pub command_payload_len_bytes: usize,
    /// Largest capacity reserved for command arguments, in bytes.
    pub command_payload_cap_bytes: usize,
    /// Most child list updates queued at once.
    pub sync_children_len: usize,
    /// Largest capacity reserved for child list updates.
    pub sync_children_cap: usize,
    /// Most child ids stored for child list updates.
    pub sync_child_ids_len: usize,
    /// Largest capacity reserved for those child ids.
    pub sync_child_ids_cap: usize,
    /// Most side effects queued at once.
    pub side_effects_len: usize,
    /// Largest capacity reserved for the side effects.
    pub side_effects_cap: usize,
}

/// Ends the event handler scope when dropped, restoring the setting that held before it.
#[must_use]
pub struct EventHandlerScopeGuard {
    previous: bool,
}

impl Drop for EventHandlerScopeGuard {
    fn drop(&mut self) {
        thread_state::cells(|cells| cells.in_event_handler.set(self.previous));
    }
}

/// Marks the thread as inside an event handler until the returned guard is dropped. In debug
/// builds, a state write there that is not inside a snapshot logs a warning.
pub fn enter_event_handler_scope() -> EventHandlerScopeGuard {
    let previous = thread_state::cells(|cells| cells.in_event_handler.replace(true));
    EventHandlerScopeGuard { previous }
}

/// Returns true if currently in an event handler context.
pub fn in_event_handler() -> bool {
    thread_state::cells(|cells| cells.in_event_handler.get())
}

/// Returns true if currently in an applied snapshot context.
pub fn in_applied_snapshot() -> bool {
    thread_state::cells(|cells| cells.in_applied_snapshot.get())
}

use std::{
    cell::{Cell, Ref, RefCell, RefMut},
    hash::{Hash, Hasher},
    rc::{Rc, Weak},
};

#[cfg(test)]
pub(crate) use runtime::{TestRuntime, TestScheduler};

use crate::collections::map::HashMap;

/// Identifies a composition group: the hash of its call site, or of an explicit key together with
/// its call site.
pub type Key = u64;
/// Identifies a node in an applier.
pub type NodeId = usize;

#[cfg(test)]
pub(crate) fn slot_validation_diagnostics_enabled() -> bool {
    true
}

#[cfg(all(debug_assertions, not(test)))]
pub(crate) fn slot_validation_diagnostics_enabled() -> bool {
    crate::env_flag!("CRANPOSE_VALIDATE_SLOTS")
}

/// Stable identifier for a slot in the slot table.
///
/// Anchors provide positional stability: they maintain their identity even when
/// the slot table is reorganized (e.g., during conditional rendering or group moves).
/// This prevents effect states from being prematurely removed during recomposition.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Default)]
pub struct AnchorId {
    id: u32,
    generation: u32,
}

impl AnchorId {
    pub(crate) const INVALID: Self = Self {
        id: 0,
        generation: 0,
    };

    pub(crate) fn new(id: usize) -> Self {
        Self {
            id: slot::checked_usize_to_u32(id, "anchor id"),
            generation: 1,
        }
    }

    /// Check if this anchor is valid (non-zero).
    pub fn is_valid(&self) -> bool {
        self.id != 0
    }
}

pub(crate) type ScopeId = usize;
pub(crate) type FrameCallbackId = u64;
/// The composition locals in scope: the innermost provider's frame, whose
/// parents lead out to the outermost, or `None` outside every provider. A
/// provider pushes one frame over the chain, so a snapshot of the locals is
/// a shared pointer and never a copy of them.
type LocalStackSnapshot = Option<Rc<composer::LocalFrame>>;

#[derive(Clone)]
pub(crate) struct LocalKey(Rc<()>);

impl LocalKey {
    fn new() -> Self {
        Self(Rc::new(()))
    }

    pub(crate) fn entry_source(&self) -> Key {
        avalanche_location_key(Rc::as_ptr(&self.0) as usize as u64)
    }
}

impl std::fmt::Debug for LocalKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("LocalKey")
            .field(&(Rc::as_ptr(&self.0) as usize))
            .finish()
    }
}

impl PartialEq for LocalKey {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for LocalKey {}

impl Hash for LocalKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}

pub use subcompose::{
    ContentTypeReusePolicy, DefaultSlotReusePolicy, SlotId, SlotReusePolicy, SubcomposeState,
    TypedSlot,
};

/// The stage of a frame in which a composer is used.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Composable functions run.
    Compose,
    /// Nodes are measured. Subcomposition is allowed.
    Measure,
    /// Nodes are placed. Subcomposition is allowed.
    Layout,
}

pub use composer_context::{
    __current_composer, note_nested_slots_host, with_composer as with_current_composer,
};

/// Runs `f` with the composer that is current on this thread. Panics if there is none. Same as
/// `with_current_composer`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
pub fn withCurrentComposer<R>(f: impl FnOnce(&Composer) -> R) -> R {
    composer_context::with_composer(f)
}

fn with_current_composer_opt<R>(f: impl FnOnce(&Composer) -> R) -> Option<R> {
    composer_context::try_with_composer(f)
}

#[doc(hidden)]
pub fn current_recompose_scope_invalidated_only_by(
    allowed_sources: impl IntoIterator<Item = StateId>,
) -> Option<bool> {
    with_current_composer_opt(|composer| {
        let allowed_sources = allowed_sources.into_iter().collect();
        let mut scope = composer.current_recompose_scope();
        let mut saw_unknown_source = false;
        while let Some(current) = scope {
            if current.has_unknown_invalidation_source() {
                saw_unknown_source = true;
                scope = current.parent_scope();
                continue;
            }
            if let Some(matches) = current.invalidated_only_by(&allowed_sources) {
                return Some(matches);
            }
            scope = current.parent_scope();
        }
        saw_unknown_source.then_some(false)
    })
    .flatten()
}

/// Holds a slot table into which a composition writes: the main composition has one, and each
/// subcomposition has its own.
pub struct SlotsHost {
    storage_key: Cell<usize>,
    inner: RefCell<SlotsHostInner>,
}

#[derive(Debug, Default)]
pub(crate) struct SlotPassOutcome {
    pub(crate) compacted: bool,
    pub(crate) compact_anchor_registry_storage: bool,
    pub(crate) compact_payload_storage: bool,
    pub(crate) trim_growth_slack: bool,
}

#[derive(Default)]
pub(crate) struct FinishedSlotPass {
    pub(crate) outcome: SlotPassOutcome,
    pub(crate) detached_root_children: Vec<slot::DetachedSubtree>,
}

struct SlotsHostInner {
    table: SlotTable,
    nested_hosts: Vec<Weak<SlotsHost>>,
    lifecycle: slot::SlotLifecycleCoordinator,
    runtime_state: Option<Rc<composer::ComposerRuntimeState>>,
    /// The table's storage capacity when the active pass began; `None`
    /// while no pass is active.
    active_pass: Option<usize>,
    /// The passes' writer state, `None` before the first pass. It stays in
    /// place between passes, each reusing its storage: moved, its hundreds
    /// of bytes cost a copy at each pass's start and end.
    pass_state: Option<slot::SlotWriteSessionState>,
}

impl Drop for SlotsHost {
    fn drop(&mut self) {
        let storage_key = self.storage_key.get();
        let inner = self.inner.get_mut();
        if let Some(state) = inner.runtime_state.clone() {
            if let Err(err) = state.dispose_retained_subtrees_for_host(
                storage_key,
                &mut inner.table,
                &mut inner.lifecycle,
            ) {
                log::error!(
                    "retained subtree disposal failed while dropping SlotsHost {storage_key}: {err}"
                );
                state.abandon_retained_subtrees_for_host(
                    storage_key,
                    &mut inner.table,
                    &mut inner.lifecycle,
                );
            } else {
                state.clear_host_storage_key(storage_key, &mut inner.table);
            }
        }
        inner.lifecycle.dispose_slot_table(&mut inner.table);
    }
}

impl SlotsHost {
    /// Returns a key that identifies the slot table the host holds. It changes when the table is
    /// replaced or taken out.
    pub fn storage_key(&self) -> usize {
        self.storage_key.get()
    }

    /// Creates a host around the slot table `storage`.
    pub fn new(storage: SlotTable) -> Self {
        let storage_key = storage.storage_id();
        Self {
            storage_key: Cell::new(storage_key),
            inner: RefCell::new(SlotsHostInner {
                table: storage,
                nested_hosts: Vec::new(),
                lifecycle: slot::SlotLifecycleCoordinator::default(),
                runtime_state: None,
                active_pass: None,
                pass_state: None,
            }),
        }
    }

    /// Records `nested` as a host composed inside this one, so that forgetting effects here reaches
    /// it too. Does nothing while this host is in use.
    pub fn note_nested_host(&self, nested: &Rc<Self>) {
        let Ok(mut inner) = self.inner.try_borrow_mut() else {
            return;
        };
        inner.nested_hosts.retain(|held| held.upgrade().is_some());
        if inner
            .nested_hosts
            .iter()
            .any(|held| held.upgrade().is_some_and(|host| Rc::ptr_eq(&host, nested)))
        {
            return;
        }
        inner.nested_hosts.push(Rc::downgrade(nested));
    }

    pub(crate) fn forget_effects(&self) -> bool {
        let (forgotten, nested, runtime_state) = {
            let Ok(mut inner) = self.inner.try_borrow_mut() else {
                return false;
            };
            if inner.active_pass.is_some() {
                return false;
            }
            let drops = inner.table.take_effect_drops();
            inner.nested_hosts.retain(|held| held.upgrade().is_some());
            let nested: Vec<Rc<Self>> = inner
                .nested_hosts
                .iter()
                .filter_map(Weak::upgrade)
                .collect();
            (drops, nested, inner.runtime_state.clone())
        };
        let mut any = !forgotten.is_empty();
        drop(forgotten);
        for host in nested {
            any |= host.forget_effects();
        }
        if any {
            self.borrow()
                .scopes()
                .for_each(RecomposeScope::force_recompose);
            if let Some(runtime_state) = runtime_state {
                runtime_state.force_recompose_retained_scopes(self.storage_key());
            }
        }
        any
    }

    pub(crate) fn bind_runtime_state(&self, state: &Rc<composer::ComposerRuntimeState>) {
        let mut inner = self.inner.borrow_mut();
        inner.runtime_state = Some(Rc::clone(state));
    }

    pub(crate) fn rebind_orphaned_runtime_state(
        &self,
        state: &Rc<composer::ComposerRuntimeState>,
    ) -> bool {
        let inner = self.inner.borrow();
        if inner.active_pass.is_some() {
            log::error!("cannot rebind SlotsHost during an active pass");
            return false;
        }
        let Some(bound_state) = inner.runtime_state.as_ref() else {
            drop(inner);
            self.bind_runtime_state(state);
            return true;
        };
        if Rc::ptr_eq(bound_state, state) {
            return true;
        }
        if bound_state.has_live_applier_host() {
            return false;
        }
        drop(inner);

        let mut inner = self.inner.borrow_mut();
        let Some(bound_state) = inner.runtime_state.as_ref() else {
            inner.runtime_state = Some(Rc::clone(state));
            return true;
        };
        if Rc::ptr_eq(bound_state, state) {
            return true;
        }
        if bound_state.has_live_applier_host() {
            return false;
        }

        let previous_state = Rc::clone(bound_state);
        let mut lifecycle = std::mem::take(&mut inner.lifecycle);
        lifecycle.flush_pending_drops();
        let host_key = self.storage_key();
        if previous_state
            .dispose_retained_subtrees_for_host(host_key, &mut inner.table, &mut lifecycle)
            .is_err()
        {
            inner.lifecycle = lifecycle;
            return false;
        }
        previous_state.clear_host(self, &mut inner.table);
        lifecycle.flush_pending_drops();
        inner.runtime_state = Some(Rc::clone(state));
        inner.lifecycle = lifecycle;
        true
    }

    pub(crate) fn runtime_state(&self) -> Option<Rc<composer::ComposerRuntimeState>> {
        self.inner.borrow().runtime_state.clone()
    }

    fn runtime_state_ptr(&self) -> Option<*const composer::ComposerRuntimeState> {
        self.inner.borrow().runtime_state.as_ref().map(Rc::as_ptr)
    }

    pub(crate) fn borrow(&self) -> Ref<'_, SlotTable> {
        Ref::map(self.inner.borrow(), |inner| &inner.table)
    }

    pub(crate) fn borrow_mut(&self) -> RefMut<'_, SlotTable> {
        RefMut::map(self.inner.borrow_mut(), |inner| &mut inner.table)
    }

    /// Takes the slot table out of the host after disposing what it retained. Fails if other
    /// references to the host exist or a slot pass is active.
    pub fn into_table(self: Rc<Self>) -> Result<SlotTable, NodeError> {
        if Rc::strong_count(&self) != 1 {
            return Err(NodeError::SlotHostUnavailable {
                operation: "SlotsHost::into_table",
                reason: "other host references are alive",
            });
        }
        self.take_table_for_transfer()
    }

    fn take_table_for_transfer(&self) -> Result<SlotTable, NodeError> {
        let inner = self.inner.borrow();
        if inner.active_pass.is_some() {
            return Err(NodeError::SlotHostUnavailable {
                operation: "SlotsHost::into_table",
                reason: "slot pass is active",
            });
        }
        drop(inner);
        let mut inner = self.inner.borrow_mut();
        let mut lifecycle = std::mem::take(&mut inner.lifecycle);
        lifecycle.flush_pending_drops();
        if let Some(state) = inner.runtime_state.clone() {
            let host_key = self.storage_key();
            if let Err(error) =
                state.dispose_retained_subtrees_for_host(host_key, &mut inner.table, &mut lifecycle)
            {
                inner.lifecycle = lifecycle;
                return Err(error);
            }
            state.clear_host(self, &mut inner.table);
            lifecycle.flush_pending_drops();
        }
        let taken = std::mem::take(&mut inner.table);
        self.storage_key.set(inner.table.storage_id());
        inner.runtime_state = None;
        inner.lifecycle = lifecycle;
        Ok(taken)
    }

    /// Disposes the content of the slot table and replaces it with an empty one. Fails while a slot
    /// pass is active.
    pub fn reset(&self) -> Result<(), NodeError> {
        let inner = self.inner.borrow();
        if inner.active_pass.is_some() {
            return Err(NodeError::SlotHostUnavailable {
                operation: "SlotsHost::reset",
                reason: "slot pass is active",
            });
        }
        let runtime_state = inner.runtime_state.clone();
        drop(inner);
        let mut inner = self.inner.borrow_mut();
        let mut lifecycle = std::mem::take(&mut inner.lifecycle);
        if let Some(state) = runtime_state {
            let host_key = self.storage_key();
            if let Err(error) =
                state.dispose_retained_subtrees_for_host(host_key, &mut inner.table, &mut lifecycle)
            {
                inner.lifecycle = lifecycle;
                return Err(error);
            }
            state.clear_host(self, &mut inner.table);
        }
        lifecycle.dispose_slot_table(&mut inner.table);
        inner.table = SlotTable::default();
        self.storage_key.set(inner.table.storage_id());
        inner.runtime_state = None;
        inner.lifecycle = slot::SlotLifecycleCoordinator::default();
        Ok(())
    }

    pub(crate) fn abandon_after_apply_failure(&self) {
        let inner = self.inner.borrow();
        if inner.active_pass.is_some() {
            log::error!("cannot abandon SlotsHost during an active pass");
            return;
        }
        let runtime_state = inner.runtime_state.clone();
        drop(inner);
        let mut inner = self.inner.borrow_mut();
        let mut lifecycle = std::mem::take(&mut inner.lifecycle);
        if let Some(state) = runtime_state {
            let host_key = self.storage_key();
            state.abandon_retained_subtrees_for_host(host_key, &mut inner.table, &mut lifecycle);
        }
        lifecycle.dispose_slot_table(&mut inner.table);
        inner.table = SlotTable::default();
        self.storage_key.set(inner.table.storage_id());
        inner.runtime_state = None;
        inner.lifecycle = slot::SlotLifecycleCoordinator::default();
    }

    pub(crate) fn debug_stats(&self) -> SlotTableDebugStats {
        let inner = self.inner.borrow();
        let local = inner.table.debug_stats();
        let lifecycle = inner.lifecycle.debug_stats();
        let retention = inner
            .runtime_state
            .clone()
            .map(|state| state.slot_retention_debug_stats(self))
            .unwrap_or_default();
        SlotTableDebugStats::from_parts(local, lifecycle, retention)
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn debug_snapshot(&self) -> SlotDebugSnapshot {
        let inner = self.inner.borrow();
        let mut snapshot = inner.table.debug_snapshot();
        if let Some(state) = inner.runtime_state.clone() {
            state.fill_slot_debug_snapshot(self, &mut snapshot);
        }
        snapshot
    }

    pub(crate) fn begin_pass(&self, mode: slot::SlotPassMode, folds: &Rc<slot::BranchFolds>) {
        let mut inner = self.inner.borrow_mut();
        let SlotsHostInner {
            table,
            active_pass,
            pass_state,
            ..
        } = &mut *inner;
        if active_pass.is_some() {
            log::error!("slot pass already active for host");
            return;
        }
        let state = match pass_state {
            Some(state) => {
                state.set_folds(Rc::clone(folds));
                state
            }
            None => pass_state.insert(slot::SlotWriteSessionState::new(Rc::clone(folds))),
        };
        state.reset_for_pass(mode);
        *active_pass = Some(table.storage_capacity());
    }

    pub(crate) fn has_active_pass(&self) -> bool {
        self.inner.borrow().active_pass.is_some()
    }

    pub(crate) fn abandon_active_pass(&self) {
        self.inner.borrow_mut().active_pass = None;
    }

    pub(crate) fn with_write_session<R>(
        &self,
        f: impl FnOnce(&mut slot::SlotWriteSession<'_>) -> R,
    ) -> R {
        let mut inner = self.inner.borrow_mut();
        let SlotsHostInner {
            table,
            lifecycle,
            active_pass,
            pass_state,
            ..
        } = &mut *inner;
        let state = pass_state
            .as_mut()
            .filter(|_| active_pass.is_some())
            .expect("slot write session requires an active pass");
        let mut session = table.write_session(lifecycle, state);
        f(&mut session)
    }

    pub(crate) fn with_table_and_lifecycle_mut<R>(
        &self,
        f: impl FnOnce(&mut SlotTable, &mut slot::SlotLifecycleCoordinator) -> R,
    ) -> R {
        let mut inner = self.inner.borrow_mut();
        let SlotsHostInner {
            table, lifecycle, ..
        } = &mut *inner;
        f(table, lifecycle)
    }

    pub(crate) fn finish_pass(
        &self,
        applier: &mut dyn Applier,
    ) -> Result<FinishedSlotPass, NodeError> {
        let mut inner = self.inner.borrow_mut();
        let SlotsHostInner {
            table,
            lifecycle,
            active_pass,
            pass_state,
            ..
        } = &mut *inner;
        let (Some(storage_capacity), Some(state)) = (active_pass.take(), pass_state.as_mut())
        else {
            return Ok(FinishedSlotPass::default());
        };

        state.flush_payload_location_refreshes(table);

        #[cfg(any(test, debug_assertions))]
        if let Err(err) = state.validate(table) {
            log::error!("slot writer invariant violation before finalize_pass: {err:?}");
            return Err(NodeError::SlotHostUnavailable {
                operation: "SlotsHost::finish_pass",
                reason: "slot writer invariant violation",
            });
        }

        let detached_root_children = {
            let mut session = table.write_session(lifecycle, state);
            session.finalize_pass(applier)?
        };

        let outcome = SlotPassOutcome {
            compacted: state.request_compaction,
            compact_anchor_registry_storage: state.request_anchor_storage_compaction,
            compact_payload_storage: state.request_payload_storage_compaction,
            trim_growth_slack: state.removed_nothing()
                && table.storage_capacity() > storage_capacity,
        };
        Ok(FinishedSlotPass {
            outcome,
            detached_root_children,
        })
    }

    pub(crate) fn flush_pending_drops(&self) {
        self.inner.borrow_mut().lifecycle.flush_pending_drops();
    }

    pub(crate) fn complete_pass_cleanup(&self, outcome: &SlotPassOutcome) {
        let mut inner = self.inner.borrow_mut();
        let SlotsHostInner {
            table,
            lifecycle,
            runtime_state,
            ..
        } = &mut *inner;
        lifecycle.flush_pending_drops();
        if outcome.compacted {
            table.compact_storage();
            lifecycle.compact_storage();
        } else if outcome.trim_growth_slack {
            table.trim_growth_slack();
        }
        if let Some(state) = runtime_state.clone() {
            state.compact_table_identity_storage_for_host(
                self,
                table,
                outcome.compact_anchor_registry_storage,
                outcome.compact_payload_storage,
            );
        } else {
            if outcome.compact_anchor_registry_storage {
                table.compact_anchor_registry_storage(None);
            }
            if outcome.compact_payload_storage {
                table.compact_payload_anchor_registry_storage(None);
            }
        }
        table.assert_fast_integrity("slot pass cleanup");
        #[cfg(any(test, debug_assertions))]
        {
            table.debug_verify();
            if let Some(state) = runtime_state.clone() {
                state.debug_verify_host(self, table);
            }
        }
    }
}

fn build_child_positions(children: &[NodeId]) -> HashMap<NodeId, usize> {
    let mut positions = HashMap::default();
    positions.reserve(children.len());
    for (index, &child) in children.iter().enumerate() {
        positions.insert(child, index);
    }
    positions
}

fn refresh_child_positions(
    current: &[NodeId],
    positions: &mut HashMap<NodeId, usize>,
    start: usize,
    end: usize,
) {
    if current.is_empty() || start >= current.len() {
        return;
    }
    let end = end.min(current.len() - 1);
    for (offset, &child) in current[start..=end].iter().enumerate() {
        positions.insert(child, start + offset);
    }
}

fn insert_child_into_diff_state(
    current: &mut ChildList,
    positions: &mut HashMap<NodeId, usize>,
    index: usize,
    child: NodeId,
) {
    let index = index.min(current.len());
    current.insert(index, child);
    refresh_child_positions(current, positions, index, current.len() - 1);
}

fn move_child_in_diff_state(
    current: &mut ChildList,
    positions: &mut HashMap<NodeId, usize>,
    from_index: usize,
    target_index: usize,
) -> usize {
    let child = current.remove(from_index);
    let to_index = target_index.min(current.len());
    current.insert(to_index, child);
    refresh_child_positions(
        current,
        positions,
        from_index.min(to_index),
        from_index.max(to_index),
    );
    to_index
}

pub(crate) use state::MutableStateInner;
pub use state::{
    MutableState, OwnedMutableState, SnapshotStateList, SnapshotStateMap, State,
    StateSubscriptionHold,
};

fn hash_key<K: Hash>(key: &K) -> Key {
    let mut hasher = hash::default::new();
    key.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn explicit_group_key_seed<K: Hash>(
    key: &K,
    caller: &'static std::panic::Location<'static>,
) -> slot::GroupKeySeed {
    let source_key = location_key(caller.file(), caller.line(), caller.column());
    let explicit_key = hash_key(key);
    slot::GroupKeySeed::keyed(source_key, explicit_key)
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/recursive_decrease_increase_test.rs"]
mod recursive_decrease_increase_test;

pub mod collections;
/// The hasher type and constructor that the framework uses by default.
pub mod hash;

/// Where a test writes real files. Behind `test-helpers` so only a test build
/// of the workspace carries it.
#[cfg(any(test, feature = "test-helpers"))]
pub mod test_scratch;
#[cfg(any(test, feature = "test-helpers"))]
pub use test_scratch::test_scratch_dir;

#[expect(clippy::print_stderr, reason = "CRANPOSE_STRUCTURAL_DIAG asks for it")]
pub(crate) fn note_structural(reason: &str, parent_id: NodeId, child_id: NodeId) {
    if env_flag!("CRANPOSE_STRUCTURAL_DIAG") {
        eprintln!("[structural] {reason} parent={parent_id} child={child_id}");
    }
}

#[expect(clippy::print_stderr, reason = "CRANPOSE_STRUCTURAL_DIAG asks for it")]
pub(crate) fn note_structural_move(parent_id: NodeId, from_index: usize, to_index: usize) {
    if env_flag!("CRANPOSE_STRUCTURAL_DIAG") {
        eprintln!("[structural] move parent={parent_id} from={from_index} to={to_index}");
    }
}
