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
mod child_positions;
mod child_sync;
pub(crate) use child_positions::{
    build_child_positions, insert_child_into_diff_state, move_child_in_diff_state,
};
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
mod slots_host;
pub(crate) use slots_host::SlotPassOutcome;
pub use slots_host::SlotsHost;
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
    hash::{Hash, Hasher},
    rc::Rc,
};

#[cfg(test)]
pub(crate) use runtime::{TestRuntime, TestScheduler};

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
