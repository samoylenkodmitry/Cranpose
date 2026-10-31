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
pub(crate) use composer_context::with_current_composer_opt;
pub use composer_context::{
    current_recompose_scope_invalidated_only_by, recompose_current_scope_again, withCurrentComposer,
};
mod composition;
mod composition_locals;
pub(crate) use composition_locals::{LocalKey, LocalStackSnapshot};
mod computed_state;
pub mod concurrency;
mod debug_trace;
mod disposable_effect;
pub use diagnostics::{DIAGNOSTICS, Diagnostic, Diagnostics};
#[cfg(test)]
pub(crate) use disposable_effect::DisposableEffectState;
pub use disposable_effect::{
    __disposable_effect_impl, DisposableEffect, DisposableEffectResult, DisposableEffectScope,
    SideEffect,
};
mod effect_key;
mod emit;
mod event_dispatch;
pub use event_dispatch::{
    EventHandlerScopeGuard, dispatch_ui_event, enter_event_handler_scope, in_applied_snapshot,
    in_event_handler, run_in_mutable_snapshot,
};
pub mod debug_toggles;
pub mod diagnostics;
#[cfg(any(feature = "internal", test))]
mod frame_clock;
mod hooks;
pub(crate) use key_group::{explicit_group_key_seed, hash_key};
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
mod phase;
pub use phase::Phase;
mod anchor_id;
pub use anchor_id::AnchorId;
pub use node_error::NodeError;
/// `Owned`, the handle to a value that a composition remembers.
pub mod owned;
mod pausable;
#[doc(hidden)]
pub use pausable::{PausableContent, with_pausable_content};
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
#[cfg(test)]
pub(crate) use runtime::{TestRuntime, TestScheduler};
#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
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
    CallbackForwarder, CallbackHolder, ParamReturnState, ParamSlot, ParamState, ReturnSlot,
    SharedParam, refresh_param, refresh_shared_param, store_param,
};
pub use composer::{
    __branch_group_scope_advance, __branch_group_scope_deferred, BranchGroupGuard,
    CapturedCompositionContext, ComposableGroup, Composer, ParameterChange, ScopedBranchGroupGuard,
    ValueSlotHandle,
};
pub(crate) use composer::{ComposerCore, EmittedNode, ParentAttachMode, ParentFrame};
pub use composition::{
    Composition, CompositionPassDebugStats, LayoutBudget, ROOT_RENDER_REPLAY_LIMIT,
};
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

/// Identifies a composition group: the hash of its call site, or of an explicit key together with
/// its call site.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key(pub u64);

impl Key {
    /// This key scoped to `site`, for a group that both identify: the bitwise xor of the two.
    pub const fn scoped(self, site: Self) -> Self {
        Self(self.0 ^ site.0)
    }

    // One FNV-1a step folding `other` into this key.
    pub(crate) const fn mixed(self, other: Self) -> Self {
        Self((self.0 ^ other.0).wrapping_mul(0x0000_0100_0000_01b3))
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
/// Identifies a node in an applier.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub usize);

impl NodeId {
    // The id an applier hands out after this one.
    pub(crate) const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl std::str::FromStr for NodeId {
    type Err = std::num::ParseIntError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        text.parse().map(Self)
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

pub(crate) type ScopeId = usize;
pub(crate) type FrameCallbackId = u64;
pub use composer_context::{
    __current_composer, note_nested_slots_host, with_composer as with_current_composer,
};
pub(crate) use state::MutableStateInner;
pub use state::{
    MutableState, OwnedMutableState, SnapshotStateList, SnapshotStateMap, State,
    StateSubscriptionHold,
};
pub use subcompose::{
    ContentTypeReusePolicy, DefaultSlotReusePolicy, SlotId, SlotReusePolicy, SubcomposeState,
    TypedSlot,
};

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
