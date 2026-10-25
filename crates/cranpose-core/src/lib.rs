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
pub use recompose_scope::{RecomposeOptions, RecomposeScope, RecomposeScopeRegistryDebugStats};
pub(crate) use recompose_scope::{RecomposeScopeInner, ScopeRead};
mod recycle_pool;
mod recycled_node;
use recycle_pool::RecyclePool;
pub use recycled_node::{RecycledNode, RecycledNodeInsertion};
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
    any::TypeId,
    cell::{Cell, Ref, RefCell, RefMut},
    cmp::Reverse,
    collections::BinaryHeap,
    hash::{Hash, Hasher},
    rc::{Rc, Weak},
};

#[cfg(test)]
pub(crate) use runtime::{TestRuntime, TestScheduler};
use smallvec::SmallVec;

use crate::collections::map::{HashMap, HashSet};

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

type CommandCallback = Box<dyn FnOnce(&mut dyn Applier) -> Result<(), NodeError> + 'static>;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirtyBubble {
    layout: bool,
    measure: bool,
    semantics: bool,
}

impl DirtyBubble {
    const LAYOUT: Self = Self {
        layout: true,
        measure: false,
        semantics: false,
    };

    const MEASURE: Self = Self {
        layout: false,
        measure: true,
        semantics: false,
    };

    pub(crate) const LAYOUT_AND_MEASURE: Self = Self {
        layout: true,
        measure: true,
        semantics: false,
    };

    pub(crate) const SEMANTICS: Self = Self {
        layout: false,
        measure: false,
        semantics: true,
    };

    /// Marks `node_id` with the bubble's flags and every ancestor up to the
    /// root as having a dirty descendant.
    fn apply(self, applier: &mut dyn Applier, node_id: NodeId) {
        self.walk(applier, Some(node_id), true);
    }

    /// Marks every ancestor of `node_id` up to the root as having a dirty
    /// descendant, leaving `node_id`'s own flags as they are.
    fn apply_to_ancestors(self, applier: &mut dyn Applier, node_id: NodeId) {
        if self.layout || self.measure {
            applier.note_layout_dirt(node_id);
        }
        let parent = applier.get_mut(node_id).ok().and_then(|node| node.parent());
        self.walk(applier, parent, false);
    }

    /// Visits each node once for all the flags. The walk goes on past an
    /// ancestor that is already dirty: a scoped layout repass can leave a
    /// dirty node under a clean ancestor. It stops at one an earlier walk of
    /// the same batch of commands passed with its flags
    /// (`bubble_passes.rs`). A virtual start node passes its own flags to
    /// its parent. A walk that carries measure marks all that one carrying
    /// layout marks, so it passes with the layout flag too, and a layout
    /// walk after it stops where it passed.
    fn walk(self, applier: &mut dyn Applier, first: Option<NodeId>, mut start: bool) {
        let flags = u8::from(self.layout || self.measure)
            | u8::from(self.measure) << 1
            | u8::from(self.semantics) << 2;
        let mut next = first;
        while let Some(id) = next {
            let Some(node) = applier.bubble_node(id, flags, !start) else {
                break;
            };
            self.mark(node, start);
            next = node.parent();
            let started = start;
            start = start && node.is_virtual();
            if started && !start && (self.layout || self.measure) {
                applier.note_layout_dirt(id);
            }
        }
    }

    /// Marks the bubble's start node with its flags, and an ancestor as
    /// having a dirty descendant where it is not marked yet.
    fn mark(self, node: &mut dyn Node, start: bool) {
        if start {
            if self.layout {
                node.mark_needs_layout();
            }
            if self.measure {
                node.mark_needs_measure();
            }
            if self.semantics {
                node.mark_needs_semantics();
            }
            return;
        }
        let marked = if self.measure {
            node.descendant_needs_measure()
        } else {
            node.descendant_needs_layout()
        };
        if (self.layout || self.measure) && !marked {
            node.mark_descendant_needs_layout(self.measure);
        }
        if self.semantics && !node.needs_semantics() {
            node.mark_descendant_needs_semantics();
        }
    }
}

pub(crate) enum Command {
    BubbleDirty {
        node_id: NodeId,
        bubble: DirtyBubble,
    },
    RemoveNode {
        id: NodeId,
    },
    MountNode {
        id: NodeId,
    },
    AttachChild {
        parent_id: NodeId,
        child_id: NodeId,
        insert_index: Option<usize>,
        bubble: DirtyBubble,
    },
    InsertChild {
        parent_id: NodeId,
        child_id: NodeId,
        appended_index: usize,
        insert_index: usize,
        bubble: DirtyBubble,
    },
    MoveChild {
        parent_id: NodeId,
        from_index: usize,
        to_index: usize,
        bubble: DirtyBubble,
    },
    RemoveChild {
        parent_id: NodeId,
        child_id: NodeId,
    },
    DetachChild {
        parent_id: NodeId,
        child_id: NodeId,
    },
    /// Makes `parent_id`'s children `expected_children`; `None` when the pass
    /// composed the children the parent held before it, in the same order.
    SyncChildren {
        parent_id: NodeId,
        expected_children: Option<ChildList>,
    },
    Callback(CommandCallback),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct DeferredChildCleanup {
    child_id: NodeId,
    generation: u32,
    removed_from_parent: bool,
}

#[derive(Default)]
struct DeferredChildCleanupQueue {
    pending: Vec<DeferredChildCleanup>,
    preserved: Vec<(NodeId, u32)>,
}

impl DeferredChildCleanupQueue {
    fn push(&mut self, child_id: NodeId, generation: u32, removed_from_parent: bool) {
        if self
            .preserved
            .iter()
            .any(|&(preserved_id, preserved_generation)| {
                preserved_id == child_id && preserved_generation == generation
            })
        {
            return;
        }
        self.pending.push(DeferredChildCleanup {
            child_id,
            generation,
            removed_from_parent,
        });
    }

    fn preserve(&mut self, child_id: NodeId, generation: u32) {
        if !self
            .preserved
            .iter()
            .any(|&(preserved_id, preserved_generation)| {
                preserved_id == child_id && preserved_generation == generation
            })
        {
            self.preserved.push((child_id, generation));
        }
        self.pending
            .retain(|cleanup| cleanup.child_id != child_id || cleanup.generation != generation);
    }

    fn flush(&mut self, applier: &mut dyn Applier) -> Result<(), NodeError> {
        for cleanup in self.pending.drain(..) {
            cleanup_detached_child(applier, cleanup)?;
        }
        Ok(())
    }

    fn clear(&mut self) {
        self.pending.clear();
        self.preserved.clear();
    }
}

impl Command {
    pub(crate) fn callback(
        callback: impl FnOnce(&mut dyn Applier) -> Result<(), NodeError> + 'static,
    ) -> Self {
        Self::Callback(Box::new(callback))
    }

    pub(crate) fn apply(self, applier: &mut dyn Applier) -> Result<(), NodeError> {
        let mut deferred_cleanup = DeferredChildCleanupQueue::default();
        self.apply_with_cleanup(applier, &mut deferred_cleanup)?;
        deferred_cleanup.flush(applier)
    }

    fn apply_with_cleanup(
        self,
        applier: &mut dyn Applier,
        deferred_cleanup: &mut DeferredChildCleanupQueue,
    ) -> Result<(), NodeError> {
        match self {
            Self::BubbleDirty { node_id, bubble } => {
                bubble.apply(applier, node_id);
                Ok(())
            }
            Self::RemoveNode { id } => {
                if let Ok(node) = applier.get_mut(id) {
                    node.unmount();
                }
                match applier.remove(id) {
                    Ok(()) | Err(NodeError::Missing { .. }) => Ok(()),
                    Err(err) => Err(err),
                }
            }
            Self::MountNode { id } => {
                let node = match applier.get_mut(id) {
                    Ok(node) => node,
                    Err(NodeError::Missing { .. }) => return Ok(()),
                    Err(err) => return Err(err),
                };
                node.set_node_id(id);
                node.mount();
                Ok(())
            }
            Self::AttachChild {
                parent_id,
                child_id,
                insert_index,
                bubble,
            } => {
                attach_child_at(applier, parent_id, child_id, insert_index, bubble);
                Ok(())
            }
            Self::InsertChild {
                parent_id,
                child_id,
                appended_index,
                insert_index,
                bubble,
            } => {
                insert_child_with_reparenting(applier, parent_id, child_id);
                bubble.apply(applier, parent_id);
                if insert_index != appended_index
                    && let Ok(parent_node) = applier.get_mut(parent_id)
                {
                    parent_node.move_child(appended_index, insert_index);
                }
                Ok(())
            }
            Self::MoveChild {
                parent_id,
                from_index,
                to_index,
                bubble,
            } => {
                if let Ok(parent_node) = applier.get_mut(parent_id) {
                    parent_node.move_child(from_index, to_index);
                }
                bubble.apply(applier, parent_id);
                note_structural_move(parent_id, from_index, to_index);
                applier.record_structural_change(parent_id);
                Ok(())
            }
            Self::RemoveChild {
                parent_id,
                child_id,
            } => apply_remove_child(applier, parent_id, child_id, deferred_cleanup),
            Self::DetachChild {
                parent_id,
                child_id,
            } => {
                let generation = applier.node_generation(child_id);
                detach_child_from_parent(applier, parent_id, child_id)?;
                deferred_cleanup.preserve(child_id, generation);
                Ok(())
            }
            Self::SyncChildren {
                parent_id,
                expected_children,
            } => sync_or_settle_children(
                applier,
                parent_id,
                expected_children.as_deref(),
                deferred_cleanup,
            ),
            Self::Callback(callback) => callback(applier),
        }
    }
}

const COMMAND_FLUSH_THRESHOLD: usize = 4096;
type ChildList = SmallVec<[NodeId; 4]>;
const SMALL_CHILD_SYNC_LINEAR_THRESHOLD: usize = 8;

#[derive(Copy, Clone)]
enum CommandTag {
    BubbleDirty,
    RemoveNode,
    MountNode,
    AttachChild,
    InsertChild,
    MoveChild,
    RemoveChild,
    DetachChild,
    SyncChildren,
    Callback,
}

impl CommandTag {
    fn label(self) -> &'static str {
        match self {
            Self::BubbleDirty => "BubbleDirty",
            Self::RemoveNode => "RemoveNode",
            Self::MountNode => "MountNode",
            Self::AttachChild => "AttachChild",
            Self::InsertChild => "InsertChild",
            Self::MoveChild => "MoveChild",
            Self::RemoveChild => "RemoveChild",
            Self::DetachChild => "DetachChild",
            Self::SyncChildren => "SyncChildren",
            Self::Callback => "Callback",
        }
    }
}

#[derive(Copy, Clone)]
struct BubbleDirtyCommand {
    node_id: NodeId,
    bubble: DirtyBubble,
}

#[derive(Copy, Clone)]
struct AttachChildCommand {
    parent_id: NodeId,
    child_id: NodeId,
    insert_index: Option<usize>,
    bubble: DirtyBubble,
}

#[derive(Copy, Clone)]
struct InsertChildCommand {
    parent_id: NodeId,
    child_id: NodeId,
    appended_index: usize,
    insert_index: usize,
    bubble: DirtyBubble,
}

#[derive(Copy, Clone)]
struct MoveChildCommand {
    parent_id: NodeId,
    from_index: usize,
    to_index: usize,
    bubble: DirtyBubble,
}

#[derive(Copy, Clone)]
struct RemoveChildCommand {
    parent_id: NodeId,
    child_id: NodeId,
}

#[derive(Copy, Clone)]
struct DetachChildCommand {
    parent_id: NodeId,
    child_id: NodeId,
}

struct SyncChildrenCommand {
    parent_id: NodeId,
    /// Where the expected children start in the queue's child ids, and how
    /// many there are; `None` for a parent that kept its children.
    children: Option<(usize, usize)>,
}

/// The commands a composition pass queued for the applier, in order. Kept
/// across passes once applied, so a frame's commands reuse the storage the
/// last frame's grew.
#[derive(Default)]
pub(crate) struct CommandQueue {
    tags: Vec<CommandTag>,
    bubble_dirty: Vec<BubbleDirtyCommand>,
    remove_nodes: Vec<NodeId>,
    mount_nodes: Vec<NodeId>,
    attach_children: Vec<AttachChildCommand>,
    insert_children: Vec<InsertChildCommand>,
    move_children: Vec<MoveChildCommand>,
    remove_children: Vec<RemoveChildCommand>,
    detach_children: Vec<DetachChildCommand>,
    sync_children: Vec<SyncChildrenCommand>,
    sync_child_ids: Vec<NodeId>,
    callbacks: Vec<CommandCallback>,
    cleanup: DeferredChildCleanupQueue,
}

impl CommandQueue {
    fn push_tag(&mut self, tag: CommandTag) {
        self.tags.push(tag);
    }

    fn push(&mut self, command: Command) {
        match command {
            Command::BubbleDirty { node_id, bubble } => {
                self.bubble_dirty
                    .push(BubbleDirtyCommand { node_id, bubble });
                self.push_tag(CommandTag::BubbleDirty);
            }
            Command::RemoveNode { id } => {
                self.remove_nodes.push(id);
                self.push_tag(CommandTag::RemoveNode);
            }
            Command::MountNode { id } => {
                self.mount_nodes.push(id);
                self.push_tag(CommandTag::MountNode);
            }
            Command::AttachChild {
                parent_id,
                child_id,
                insert_index,
                bubble,
            } => {
                self.attach_children.push(AttachChildCommand {
                    parent_id,
                    child_id,
                    insert_index,
                    bubble,
                });
                self.push_tag(CommandTag::AttachChild);
            }
            Command::InsertChild {
                parent_id,
                child_id,
                appended_index,
                insert_index,
                bubble,
            } => {
                self.insert_children.push(InsertChildCommand {
                    parent_id,
                    child_id,
                    appended_index,
                    insert_index,
                    bubble,
                });
                self.push_tag(CommandTag::InsertChild);
            }
            Command::MoveChild {
                parent_id,
                from_index,
                to_index,
                bubble,
            } => {
                self.move_children.push(MoveChildCommand {
                    parent_id,
                    from_index,
                    to_index,
                    bubble,
                });
                self.push_tag(CommandTag::MoveChild);
            }
            Command::RemoveChild {
                parent_id,
                child_id,
            } => {
                self.remove_children.push(RemoveChildCommand {
                    parent_id,
                    child_id,
                });
                self.push_tag(CommandTag::RemoveChild);
            }
            Command::DetachChild {
                parent_id,
                child_id,
            } => {
                self.detach_children.push(DetachChildCommand {
                    parent_id,
                    child_id,
                });
                self.push_tag(CommandTag::DetachChild);
            }
            Command::SyncChildren {
                parent_id,
                expected_children,
            } => {
                let children = expected_children.map(|expected| {
                    let child_start = self.sync_child_ids.len();
                    self.sync_child_ids.extend_from_slice(&expected);
                    (child_start, expected.len())
                });
                self.sync_children.push(SyncChildrenCommand {
                    parent_id,
                    children,
                });
                self.push_tag(CommandTag::SyncChildren);
            }
            Command::Callback(callback) => {
                self.callbacks.push(callback);
                self.push_tag(CommandTag::Callback);
            }
        }
    }

    fn len(&self) -> usize {
        self.tags.len()
    }

    fn capacity(&self) -> usize {
        self.tags.capacity()
    }

    fn payload_len_bytes(&self) -> usize {
        self.bubble_dirty
            .len()
            .saturating_mul(size_of::<BubbleDirtyCommand>())
            .saturating_add(self.remove_nodes.len().saturating_mul(size_of::<NodeId>()))
            .saturating_add(self.mount_nodes.len().saturating_mul(size_of::<NodeId>()))
            .saturating_add(
                self.attach_children
                    .len()
                    .saturating_mul(size_of::<AttachChildCommand>()),
            )
            .saturating_add(
                self.insert_children
                    .len()
                    .saturating_mul(size_of::<InsertChildCommand>()),
            )
            .saturating_add(
                self.move_children
                    .len()
                    .saturating_mul(size_of::<MoveChildCommand>()),
            )
            .saturating_add(
                self.remove_children
                    .len()
                    .saturating_mul(size_of::<RemoveChildCommand>()),
            )
            .saturating_add(
                self.detach_children
                    .len()
                    .saturating_mul(size_of::<DetachChildCommand>()),
            )
            .saturating_add(
                self.sync_children
                    .len()
                    .saturating_mul(size_of::<SyncChildrenCommand>()),
            )
            .saturating_add(
                self.sync_child_ids
                    .len()
                    .saturating_mul(size_of::<NodeId>()),
            )
            .saturating_add(
                self.callbacks
                    .len()
                    .saturating_mul(size_of::<CommandCallback>()),
            )
    }

    fn payload_capacity_bytes(&self) -> usize {
        self.bubble_dirty
            .capacity()
            .saturating_mul(size_of::<BubbleDirtyCommand>())
            .saturating_add(
                self.remove_nodes
                    .capacity()
                    .saturating_mul(size_of::<NodeId>()),
            )
            .saturating_add(
                self.mount_nodes
                    .capacity()
                    .saturating_mul(size_of::<NodeId>()),
            )
            .saturating_add(
                self.attach_children
                    .capacity()
                    .saturating_mul(size_of::<AttachChildCommand>()),
            )
            .saturating_add(
                self.insert_children
                    .capacity()
                    .saturating_mul(size_of::<InsertChildCommand>()),
            )
            .saturating_add(
                self.move_children
                    .capacity()
                    .saturating_mul(size_of::<MoveChildCommand>()),
            )
            .saturating_add(
                self.remove_children
                    .capacity()
                    .saturating_mul(size_of::<RemoveChildCommand>()),
            )
            .saturating_add(
                self.detach_children
                    .capacity()
                    .saturating_mul(size_of::<DetachChildCommand>()),
            )
            .saturating_add(
                self.sync_children
                    .capacity()
                    .saturating_mul(size_of::<SyncChildrenCommand>()),
            )
            .saturating_add(
                self.sync_child_ids
                    .capacity()
                    .saturating_mul(size_of::<NodeId>()),
            )
            .saturating_add(
                self.callbacks
                    .capacity()
                    .saturating_mul(size_of::<CommandCallback>()),
            )
    }

    /// Applies the queued commands in order and empties the queue, keeping
    /// its storage. A failed command leaves the queue empty too.
    pub(crate) fn apply(&mut self, applier: &mut dyn Applier) -> Result<(), NodeError> {
        applier.begin_bubble_batch();
        let result = self.apply_in_order(applier);
        applier.end_bubble_batch();
        self.clear();
        result
    }

    fn clear(&mut self) {
        let Self {
            tags,
            bubble_dirty,
            remove_nodes,
            mount_nodes,
            attach_children,
            insert_children,
            move_children,
            remove_children,
            detach_children,
            sync_children,
            sync_child_ids,
            callbacks,
            cleanup,
        } = self;
        tags.clear();
        bubble_dirty.clear();
        remove_nodes.clear();
        mount_nodes.clear();
        attach_children.clear();
        insert_children.clear();
        move_children.clear();
        remove_children.clear();
        detach_children.clear();
        sync_children.clear();
        sync_child_ids.clear();
        callbacks.clear();
        cleanup.clear();
    }

    fn apply_in_order(&mut self, applier: &mut dyn Applier) -> Result<(), NodeError> {
        let mut payloads = CommandPayloads {
            bubble_dirty: self.bubble_dirty.drain(..),
            remove_nodes: self.remove_nodes.drain(..),
            mount_nodes: self.mount_nodes.drain(..),
            attach_children: self.attach_children.drain(..),
            insert_children: self.insert_children.drain(..),
            move_children: self.move_children.drain(..),
            remove_children: self.remove_children.drain(..),
            detach_children: self.detach_children.drain(..),
            sync_children: self.sync_children.drain(..),
            sync_child_ids: &self.sync_child_ids,
            callbacks: self.callbacks.drain(..),
        };
        for &tag in &self.tags {
            payloads.apply(tag, applier, &mut self.cleanup)?;
        }
        payloads.assert_all_taken();
        self.cleanup.flush(applier)
    }
}

/// The payloads of a queue being applied, taken in the order its tags name
/// them.
struct CommandPayloads<'a> {
    bubble_dirty: std::vec::Drain<'a, BubbleDirtyCommand>,
    remove_nodes: std::vec::Drain<'a, NodeId>,
    mount_nodes: std::vec::Drain<'a, NodeId>,
    attach_children: std::vec::Drain<'a, AttachChildCommand>,
    insert_children: std::vec::Drain<'a, InsertChildCommand>,
    move_children: std::vec::Drain<'a, MoveChildCommand>,
    remove_children: std::vec::Drain<'a, RemoveChildCommand>,
    detach_children: std::vec::Drain<'a, DetachChildCommand>,
    sync_children: std::vec::Drain<'a, SyncChildrenCommand>,
    sync_child_ids: &'a [NodeId],
    callbacks: std::vec::Drain<'a, CommandCallback>,
}

impl CommandPayloads<'_> {
    fn apply(
        &mut self,
        tag: CommandTag,
        applier: &mut dyn Applier,
        cleanup: &mut DeferredChildCleanupQueue,
    ) -> Result<(), NodeError> {
        match tag {
            CommandTag::BubbleDirty
            | CommandTag::RemoveNode
            | CommandTag::MountNode
            | CommandTag::Callback => self.apply_node_command(tag, applier, cleanup),
            CommandTag::AttachChild
            | CommandTag::InsertChild
            | CommandTag::MoveChild
            | CommandTag::RemoveChild
            | CommandTag::DetachChild
            | CommandTag::SyncChildren => self.apply_child_command(tag, applier, cleanup),
        }
    }

    fn apply_node_command(
        &mut self,
        tag: CommandTag,
        applier: &mut dyn Applier,
        cleanup: &mut DeferredChildCleanupQueue,
    ) -> Result<(), NodeError> {
        match tag {
            CommandTag::BubbleDirty => {
                let BubbleDirtyCommand { node_id, bubble } =
                    next_command_payload(&mut self.bubble_dirty, tag)?;
                Command::BubbleDirty { node_id, bubble }.apply_with_cleanup(applier, cleanup)?;
            }
            CommandTag::RemoveNode => {
                let id = next_command_payload(&mut self.remove_nodes, tag)?;
                Command::RemoveNode { id }.apply_with_cleanup(applier, cleanup)?;
            }
            CommandTag::MountNode => {
                let id = next_command_payload(&mut self.mount_nodes, tag)?;
                Command::MountNode { id }.apply_with_cleanup(applier, cleanup)?;
            }
            CommandTag::Callback => {
                let callback = next_command_payload(&mut self.callbacks, tag)?;
                Command::Callback(callback).apply_with_cleanup(applier, cleanup)?;
                applier.forget_bubble_passes();
            }
            CommandTag::AttachChild
            | CommandTag::InsertChild
            | CommandTag::MoveChild
            | CommandTag::RemoveChild
            | CommandTag::DetachChild
            | CommandTag::SyncChildren => return Err(command_payload_error(tag)),
        }
        Ok(())
    }

    fn apply_child_command(
        &mut self,
        tag: CommandTag,
        applier: &mut dyn Applier,
        cleanup: &mut DeferredChildCleanupQueue,
    ) -> Result<(), NodeError> {
        match tag {
            CommandTag::AttachChild => {
                let AttachChildCommand {
                    parent_id,
                    child_id,
                    insert_index,
                    bubble,
                } = next_command_payload(&mut self.attach_children, tag)?;
                Command::AttachChild {
                    parent_id,
                    child_id,
                    insert_index,
                    bubble,
                }
                .apply_with_cleanup(applier, cleanup)?;
            }
            CommandTag::InsertChild => {
                let InsertChildCommand {
                    parent_id,
                    child_id,
                    appended_index,
                    insert_index,
                    bubble,
                } = next_command_payload(&mut self.insert_children, tag)?;
                Command::InsertChild {
                    parent_id,
                    child_id,
                    appended_index,
                    insert_index,
                    bubble,
                }
                .apply_with_cleanup(applier, cleanup)?;
            }
            CommandTag::MoveChild => {
                let MoveChildCommand {
                    parent_id,
                    from_index,
                    to_index,
                    bubble,
                } = next_command_payload(&mut self.move_children, tag)?;
                Command::MoveChild {
                    parent_id,
                    from_index,
                    to_index,
                    bubble,
                }
                .apply_with_cleanup(applier, cleanup)?;
            }
            CommandTag::RemoveChild => {
                let RemoveChildCommand {
                    parent_id,
                    child_id,
                } = next_command_payload(&mut self.remove_children, tag)?;
                Command::RemoveChild {
                    parent_id,
                    child_id,
                }
                .apply_with_cleanup(applier, cleanup)?;
            }
            CommandTag::DetachChild => {
                let DetachChildCommand {
                    parent_id,
                    child_id,
                } = next_command_payload(&mut self.detach_children, tag)?;
                Command::DetachChild {
                    parent_id,
                    child_id,
                }
                .apply_with_cleanup(applier, cleanup)?;
            }
            CommandTag::SyncChildren => self.sync_children(applier, cleanup)?,
            CommandTag::BubbleDirty
            | CommandTag::RemoveNode
            | CommandTag::MountNode
            | CommandTag::Callback => return Err(command_payload_error(tag)),
        }
        Ok(())
    }

    fn sync_children(
        &mut self,
        applier: &mut dyn Applier,
        cleanup: &mut DeferredChildCleanupQueue,
    ) -> Result<(), NodeError> {
        let tag = CommandTag::SyncChildren;
        let SyncChildrenCommand {
            parent_id,
            children,
        } = next_command_payload(&mut self.sync_children, tag)?;
        let expected_children = children
            .map(|(child_start, child_len)| {
                child_start
                    .checked_add(child_len)
                    .and_then(|child_end| self.sync_child_ids.get(child_start..child_end))
                    .ok_or_else(|| command_payload_error(tag))
            })
            .transpose()?;
        sync_or_settle_children(applier, parent_id, expected_children, cleanup)
    }

    fn assert_all_taken(&mut self) {
        debug_assert!(self.bubble_dirty.next().is_none());
        debug_assert!(self.remove_nodes.next().is_none());
        debug_assert!(self.mount_nodes.next().is_none());
        debug_assert!(self.attach_children.next().is_none());
        debug_assert!(self.insert_children.next().is_none());
        debug_assert!(self.move_children.next().is_none());
        debug_assert!(self.remove_children.next().is_none());
        debug_assert!(self.detach_children.next().is_none());
        debug_assert!(self.sync_children.next().is_none());
        debug_assert!(self.callbacks.next().is_none());
    }
}

fn command_payload_error(tag: CommandTag) -> NodeError {
    NodeError::MalformedCommandPayload { tag: tag.label() }
}

fn next_command_payload<T>(
    payloads: &mut impl Iterator<Item = T>,
    tag: CommandTag,
) -> Result<T, NodeError> {
    payloads.next().ok_or_else(|| command_payload_error(tag))
}

fn attach_child_at(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    child_id: NodeId,
    insert_index: Option<usize>,
    bubble: DirtyBubble,
) {
    if insert_child_with_reparenting(applier, parent_id, child_id) {
        if let Some(target) = insert_index {
            move_attached_child_left_to(applier, parent_id, child_id, target);
        }
        bubble.apply(applier, parent_id);
    } else if let Ok(child) = applier.get_mut(child_id) {
        let dirty_bubble = DirtyBubble {
            layout: child.layout_dirty(),
            measure: child.needs_measure() || child.descendant_needs_measure(),
            semantics: false,
        };
        dirty_bubble.apply_to_ancestors(applier, child_id);
    }
}

fn move_attached_child_left_to(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    child_id: NodeId,
    target: usize,
) {
    let Ok(parent_node) = applier.get_mut(parent_id) else {
        return;
    };
    let Some(current_index) = parent_node.owned_child_index(child_id) else {
        return;
    };
    if target < current_index {
        parent_node.move_child(current_index, target);
        note_structural_move(parent_id, current_index, target);
    }
}

fn insert_child_with_reparenting(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    child_id: NodeId,
) -> bool {
    if parent_id == child_id {
        debug_assert_ne!(
            parent_id, child_id,
            "a node cannot be attached as its own child"
        );
        return false;
    }

    let old_parent = applier
        .get_mut(child_id)
        .ok()
        .and_then(|node| node.parent());
    if old_parent != Some(parent_id) {
        applier.forget_bubble_passes();
    }
    if let Some(old_parent_id) = old_parent
        && old_parent_id != parent_id
    {
        let removed = applier
            .get_mut(old_parent_id)
            .is_ok_and(|old_parent_node| old_parent_node.remove_child(child_id));
        if let Ok(child_node) = applier.get_mut(child_id) {
            child_node.on_removed_from_parent();
        }
        if removed {
            DirtyBubble::LAYOUT_AND_MEASURE.apply(applier, old_parent_id);
            note_structural("reparent-detach", old_parent_id, child_id);
            applier.record_structural_change(old_parent_id);
        }
    }

    let inserted = applier
        .get_mut(parent_id)
        .is_ok_and(|parent_node| parent_node.insert_child(child_id));
    if inserted {
        note_structural("attach", parent_id, child_id);
        applier.record_structural_change(parent_id);
    }
    if let Ok(child_node) = applier.get_mut(child_id) {
        child_node.on_attached_to_parent(parent_id);
    }
    inserted
}

fn apply_remove_child(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    child_id: NodeId,
    deferred_cleanup: &mut DeferredChildCleanupQueue,
) -> Result<(), NodeError> {
    detach_child_from_parent(applier, parent_id, child_id)?;

    let generation = applier.node_generation(child_id);
    let removed_from_parent = if let Ok(node) = applier.get_mut(child_id) {
        node.parent().is_none()
    } else {
        return Ok(());
    };
    deferred_cleanup.push(child_id, generation, removed_from_parent);
    Ok(())
}

fn detach_child_from_parent(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    child_id: NodeId,
) -> Result<(), NodeError> {
    let removed = applier
        .get_mut(parent_id)
        .is_ok_and(|parent_node| parent_node.remove_child(child_id));
    if removed {
        DirtyBubble::LAYOUT_AND_MEASURE.apply(applier, parent_id);
        note_structural("detach", parent_id, child_id);
        applier.record_structural_change(parent_id);
    }

    if let Ok(node) = applier.get_mut(child_id) {
        match node.parent() {
            Some(existing_parent_id) if existing_parent_id == parent_id => {
                node.on_removed_from_parent();
                applier.forget_bubble_passes();
            }
            None => {}
            Some(_) => return Ok(()),
        }
    } else {
        return Ok(());
    }

    Ok(())
}

fn cleanup_detached_child(
    applier: &mut dyn Applier,
    cleanup: DeferredChildCleanup,
) -> Result<(), NodeError> {
    if applier.node_generation(cleanup.child_id) != cleanup.generation {
        return Ok(());
    }

    let parent_id = match applier.get_mut(cleanup.child_id) {
        Ok(node) => node.parent(),
        Err(NodeError::Missing { .. }) => return Ok(()),
        Err(err) => return Err(err),
    };
    if parent_id.is_some() {
        return Ok(());
    }

    if let Ok(node) = applier.get_mut(cleanup.child_id) {
        if !cleanup.removed_from_parent {
            node.on_removed_from_parent();
        }
        node.unmount();
    }
    match applier.remove(cleanup.child_id) {
        Ok(()) | Err(NodeError::Missing { .. }) => Ok(()),
        Err(err) => Err(err),
    }
}

fn remove_child_and_cleanup_now(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    child_id: NodeId,
) -> Result<(), NodeError> {
    let mut deferred_cleanup = DeferredChildCleanupQueue::default();
    apply_remove_child(applier, parent_id, child_id, &mut deferred_cleanup)?;
    deferred_cleanup.flush(applier)
}

fn collect_current_children(applier: &mut dyn Applier, parent_id: NodeId) -> ChildList {
    let mut scratch = SmallVec::<[NodeId; 8]>::new();
    if let Ok(node) = applier.get_mut(parent_id) {
        node.collect_children_into(&mut scratch);
    }
    let mut current = ChildList::new();
    current.extend(scratch);
    current
}

fn sync_children(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    expected_children: &[NodeId],
    deferred_cleanup: &mut DeferredChildCleanupQueue,
) -> Result<(), NodeError> {
    let mut current = collect_current_children(applier, parent_id);
    let children_changed = current.as_slice() != expected_children;

    if children_changed {
        if current.len().max(expected_children.len()) <= SMALL_CHILD_SYNC_LINEAR_THRESHOLD {
            sync_children_small(
                applier,
                parent_id,
                &mut current,
                expected_children,
                deferred_cleanup,
            )?;
        } else {
            let mut target_positions: HashMap<NodeId, usize> = HashMap::default();
            target_positions.reserve(expected_children.len());
            for (index, &child) in expected_children.iter().enumerate() {
                target_positions.insert(child, index);
            }

            for index in (0..current.len()).rev() {
                let child = current[index];
                if !target_positions.contains_key(&child) {
                    current.remove(index);
                    apply_remove_child(applier, parent_id, child, deferred_cleanup)?;
                }
            }

            let mut current_positions = build_child_positions(&current);
            for (target_index, &child) in expected_children.iter().enumerate() {
                if let Some(current_index) = current_positions.get(&child).copied() {
                    if current_index != target_index {
                        let from_index = current_index;
                        let to_index = move_child_in_diff_state(
                            &mut current,
                            &mut current_positions,
                            from_index,
                            target_index,
                        );
                        Command::MoveChild {
                            parent_id,
                            from_index,
                            to_index,
                            bubble: DirtyBubble::LAYOUT_AND_MEASURE,
                        }
                        .apply(applier)?;
                    }
                } else {
                    let insert_index = target_index.min(current.len());
                    let appended_index = current.len();
                    insert_child_into_diff_state(
                        &mut current,
                        &mut current_positions,
                        insert_index,
                        child,
                    );
                    Command::InsertChild {
                        parent_id,
                        child_id: child,
                        appended_index,
                        insert_index,
                        bubble: DirtyBubble::LAYOUT_AND_MEASURE,
                    }
                    .apply(applier)?;
                }
            }
        }
    }

    reconcile_children(applier, parent_id, expected_children, !children_changed)
}

fn sync_children_small(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    current: &mut ChildList,
    expected_children: &[NodeId],
    deferred_cleanup: &mut DeferredChildCleanupQueue,
) -> Result<(), NodeError> {
    for index in (0..current.len()).rev() {
        let child = current[index];
        if !expected_children.contains(&child) {
            current.remove(index);
            apply_remove_child(applier, parent_id, child, deferred_cleanup)?;
        }
    }

    for (target_index, &child) in expected_children.iter().enumerate() {
        if let Some(current_index) = current
            .iter()
            .position(|&current_child| current_child == child)
        {
            if current_index != target_index {
                let child = current.remove(current_index);
                let to_index = target_index.min(current.len());
                current.insert(to_index, child);
                Command::MoveChild {
                    parent_id,
                    from_index: current_index,
                    to_index,
                    bubble: DirtyBubble::LAYOUT_AND_MEASURE,
                }
                .apply(applier)?;
            }
        } else {
            let insert_index = target_index.min(current.len());
            let appended_index = current.len();
            current.insert(insert_index, child);
            Command::InsertChild {
                parent_id,
                child_id: child,
                appended_index,
                insert_index,
                bubble: DirtyBubble::LAYOUT_AND_MEASURE,
            }
            .apply(applier)?;
        }
    }

    Ok(())
}

/// Attaches the expected children that do not name the parent. An unchanged
/// child list is the parent's own: a child that tracks its parent already
/// names it, and one that does not track it cannot be repaired, so only a
/// changed list visits its children.
fn reconcile_children(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    expected_children: &[NodeId],
    children_unchanged: bool,
) -> Result<(), NodeError> {
    if children_unchanged {
        settle_kept_children(applier, parent_id);
        return Ok(());
    }
    let mut repaired = false;
    for &child_id in expected_children {
        let needs_attach = applier
            .get_mut(child_id)
            .is_ok_and(|node| node.parent() != Some(parent_id));
        if needs_attach {
            insert_child_with_reparenting(applier, parent_id, child_id);
            repaired = true;
        }
    }
    if repaired {
        DirtyBubble::LAYOUT_AND_MEASURE.apply(applier, parent_id);
    }
    Ok(())
}

/// Makes `parent_id`'s children `expected_children`, or, for `None`, finishes
/// a parent that kept the ones it held.
fn sync_or_settle_children(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    expected_children: Option<&[NodeId]>,
    deferred_cleanup: &mut DeferredChildCleanupQueue,
) -> Result<(), NodeError> {
    if applier
        .get_mut(parent_id)
        .is_ok_and(|parent| parent.keeps_own_children())
    {
        return Ok(());
    }
    match expected_children {
        Some(expected_children) => {
            sync_children(applier, parent_id, expected_children, deferred_cleanup)
        }
        None => {
            settle_kept_children(applier, parent_id);
            Ok(())
        }
    }
}

/// Finishes a parent whose children stayed as they were: its own layout dirt,
/// which it may have taken while it composed, reaches its ancestors.
fn settle_kept_children(applier: &mut dyn Applier, parent_id: NodeId) {
    debug_assert!(
        collect_current_children(applier, parent_id)
            .iter()
            .all(|&child_id| applier
                .get_mut(child_id)
                .map_or(true, |node| node.parent().is_none_or(|p| p == parent_id))),
        "the children a parent holds must name no other parent"
    );
    if applier
        .get_mut(parent_id)
        .is_ok_and(|node| node.layout_dirty())
    {
        DirtyBubble::LAYOUT.apply_to_ancestors(applier, parent_id);
    }
}

/// The stock [`Applier`]: it keeps nodes in memory under ids that stay stable for the life of the
/// node.
#[derive(Default)]
pub struct MemoryApplier {
    /// Behind one allocation, so lending the applier to a layout pass moves a
    /// pointer.
    state: Box<MemoryApplierState>,
}

#[derive(Default)]
struct MemoryApplierState {
    nodes: Vec<Option<Box<dyn Node>>>,
    physical_stable_ids: Vec<u32>,
    physical_warm_recycled_origins: Vec<bool>,
    stable_index: stable_index::StableIndex,
    free_ids: BinaryHeap<Reverse<usize>>,
    high_ids: HashMap<NodeId, HighIdSlot>,
    next_stable_id: NodeId,
    layout_runtime: Option<RuntimeHandle>,
    recycle_pools: HashMap<TypeId, RecyclePool>,
    structural_change_parents: Vec<NodeId>,
    virtual_node_ids: HashSet<NodeId>,
    bubble_passes: bubble_passes::BubblePasses,
    layout_dirt_roots: layout_dirt_roots::LayoutDirtRoots,
}

// A node stored under an id too high for the dense table: the node while it
// lives, whether it came from a warm shell, and the generation its id has
// reached, which stays after the node goes.
#[derive(Default)]
struct HighIdSlot {
    node: Option<Box<dyn Node>>,
    warm_origin: bool,
    generation: u32,
}

/// Reusable ancestry storage for [`MemoryApplier::scene_nodes_attached_to_into`].
///
/// Attachment results are kept only for one batch, so callers can reuse this
/// value without keeping stale attachment results after graph mutations.
#[derive(Default)]
pub struct SceneNodeAttachmentScratch {
    attached: NodeMarks,
    path: Vec<NodeId>,
}

const ATTACHED: u8 = 1;
const DETACHED: u8 = 2;

struct RemovalFrame {
    node_id: NodeId,
    children: SmallVec<[NodeId; 8]>,
    next_child: usize,
}

/// Sizes of a [`MemoryApplier`]'s tables, for leak and memory checks. A `_len` field counts entries
/// and a `_cap` field the capacity reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemoryApplierDebugStats {
    /// The id that the next created node gets.
    pub next_stable_id: NodeId,
    /// Live nodes.
    pub nodes_len: usize,
    /// Slots in the dense node table, both live and free.
    pub nodes_cap: usize,
    /// Entries in the table that gives each slot its stable id.
    pub physical_stable_ids_len: usize,
    /// Capacity reserved for the stable ids of table slots.
    pub physical_stable_ids_cap: usize,
    /// Live entries in the index from stable ids to table slots.
    pub stable_to_physical_len: usize,
    /// Capacity reserved for the index from stable ids to table slots.
    pub stable_to_physical_cap: usize,
    /// Stable ids that carry a generation counter.
    pub stable_generations_len: usize,
    /// Capacity reserved for the generation counters of stable ids.
    pub stable_generations_cap: usize,
    /// Dense table slots free for reuse.
    pub free_ids_len: usize,
    /// Capacity reserved for the free slot ids.
    pub free_ids_cap: usize,
    /// Nodes stored under ids too high for the dense table.
    pub high_id_nodes_len: usize,
    /// Capacity reserved for the high-id nodes.
    pub high_id_nodes_cap: usize,
    /// Generation counters kept for high-id nodes.
    pub high_id_generations_len: usize,
    /// Capacity reserved for the generations of high-id nodes.
    pub high_id_generations_cap: usize,
    /// Node types that have recycled nodes pooled.
    pub recycled_type_count: usize,
    /// Capacity reserved for the pools of recycled nodes.
    pub recycled_type_cap: usize,
    /// Pooled recycled nodes of all types.
    pub recycled_node_count: usize,
    /// Capacity reserved for the pooled recycled nodes.
    pub recycled_node_capacity: usize,
    /// Nodes recycled through the warm path, both live and pooled.
    pub warm_recycled_node_id_count: usize,
    /// Capacity reserved for the warm recycled nodes.
    pub warm_recycled_node_id_capacity: usize,
}

impl MemoryApplier {
    const EAGER_COMPACT_NODE_LEN: usize = 1_024;
    const HIGH_ID_THRESHOLD: NodeId = 1_000_000_000;
    const INVALID_STABLE_ID: u32 = u32::MAX;
    const INITIAL_DENSE_NODE_CAP: usize = 32;
    const LARGE_DENSE_NODE_GROWTH_THRESHOLD: usize = 32 * 1024;
    const LARGE_DENSE_NODE_GROWTH_DIVISOR: usize = 4;

    fn pack_stable_id(stable_id: NodeId) -> u32 {
        u32::try_from(stable_id).expect("stable id overflow")
    }

    fn unpack_stable_id(stable_id: u32) -> NodeId {
        stable_id as NodeId
    }

    fn next_dense_node_target_len(old_len: usize) -> usize {
        if old_len < Self::INITIAL_DENSE_NODE_CAP {
            return Self::INITIAL_DENSE_NODE_CAP;
        }
        if old_len < Self::LARGE_DENSE_NODE_GROWTH_THRESHOLD {
            return old_len.saturating_mul(2);
        }

        let incremental_growth =
            (old_len / Self::LARGE_DENSE_NODE_GROWTH_DIVISOR).max(Self::INITIAL_DENSE_NODE_CAP);
        old_len.saturating_add(incremental_growth)
    }

    fn ensure_dense_node_storage_capacity(&mut self) {
        let len = self
            .state
            .nodes
            .len()
            .max(self.state.physical_stable_ids.len())
            .max(self.state.physical_warm_recycled_origins.len());
        if len < self.state.nodes.capacity()
            && len < self.state.physical_stable_ids.capacity()
            && len < self.state.physical_warm_recycled_origins.capacity()
        {
            return;
        }

        let target = Self::next_dense_node_target_len(len);
        if self.state.nodes.capacity() < target {
            self.state
                .nodes
                .reserve_exact(target.saturating_sub(self.state.nodes.len()));
        }
        if self.state.physical_stable_ids.capacity() < target {
            self.state
                .physical_stable_ids
                .reserve_exact(target.saturating_sub(self.state.physical_stable_ids.len()));
        }
        if self.state.physical_warm_recycled_origins.capacity() < target {
            self.state.physical_warm_recycled_origins.reserve_exact(
                target.saturating_sub(self.state.physical_warm_recycled_origins.len()),
            );
        }
    }

    /// Creates an applier with no nodes.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves a scene-scope candidate the way structural records are
    /// resolved: to its nearest non-virtual ancestor, and only while still
    /// attached to `root`. A node detached after recording must not reach the
    /// scoped scene update — an id the graph cannot resolve forces it to give
    /// up and rebuild the whole scene.
    pub fn scene_node_attached_to(&mut self, node_id: NodeId, root: NodeId) -> Option<NodeId> {
        let resolved = self.first_non_virtual_ancestor(node_id)?;
        self.is_attached_to(resolved, root).then_some(resolved)
    }

    /// [`Self::scene_node_attached_to`] for each of `nodes`, in order, with
    /// every ancestor looked up at most once: a batch costs the distinct
    /// ancestors of its nodes, not each node's depth.
    pub fn scene_nodes_attached_to(
        &mut self,
        nodes: impl IntoIterator<Item = NodeId>,
        root: NodeId,
    ) -> Vec<Option<NodeId>> {
        let mut result = Vec::new();
        self.scene_nodes_attached_to_into(
            nodes,
            root,
            &mut result,
            &mut SceneNodeAttachmentScratch::default(),
        );
        result
    }

    /// Fills `output` with [`Self::scene_nodes_attached_to`] results, reusing
    /// its storage and the ancestry cache in `scratch` across batches.
    ///
    /// Each distinct ancestor is inspected at most once per call. The cache
    /// is cleared for each call, so graph mutations are observed by the next
    /// batch.
    ///
    /// ```
    /// use cranpose_core::{MemoryApplier, NodeId, SceneNodeAttachmentScratch};
    ///
    /// let mut applier = MemoryApplier::new();
    /// let mut scratch = SceneNodeAttachmentScratch::default();
    /// let mut attached: Vec<Option<NodeId>> = Vec::new();
    /// let root = NodeId::default();
    /// applier.scene_nodes_attached_to_into([], root, &mut attached, &mut scratch);
    /// assert!(attached.is_empty());
    /// ```
    pub fn scene_nodes_attached_to_into(
        &mut self,
        nodes: impl IntoIterator<Item = NodeId>,
        root: NodeId,
        output: &mut Vec<Option<NodeId>>,
        scratch: &mut SceneNodeAttachmentScratch,
    ) {
        let nodes = nodes.into_iter();
        output.clear();
        let (lower_bound, upper_bound) = nodes.size_hint();
        if lower_bound == 0 && upper_bound == Some(0) {
            scratch.path.clear();
            return;
        }
        scratch.attached.reset(self);
        scratch.attached.set(self, root, ATTACHED);
        output.reserve(lower_bound);
        scratch.path.clear();
        for node_id in nodes {
            let Some(resolved) = self.first_non_virtual_ancestor(node_id) else {
                output.push(None);
                continue;
            };
            let mut current = resolved;
            scratch.path.clear();
            let answer = loop {
                match scratch.attached.get(self, current) {
                    ATTACHED => break true,
                    DETACHED => break false,
                    _ => {}
                }
                scratch.path.push(current);
                match self.get_mut(current).ok().and_then(|node| node.parent()) {
                    Some(parent) if scratch.path.len() < 100_000 => current = parent,
                    _ => break false,
                }
            };
            let mark = if answer { ATTACHED } else { DETACHED };
            for visited in scratch.path.drain(..) {
                scratch.attached.set(self, visited, mark);
            }
            output.push(answer.then_some(resolved));
        }
    }

    /// Drains the parents recorded via [`Applier::record_structural_change`],
    /// keeping only nodes still attached to `root` (a parent that was itself
    /// removed is covered by its own surviving ancestor's record). A virtual
    /// parent — a subcompose slot wrapper the render graph never contains —
    /// is reported as its nearest non-virtual ancestor: that is the node
    /// whose graph child set the change altered, and an id the graph cannot
    /// resolve would force the scoped scene update to give up and rebuild.
    pub fn take_structural_change_parents_attached_to(&mut self, root: NodeId) -> Vec<NodeId> {
        let mut candidates = Vec::new();
        self.take_structural_change_parents_into(&mut candidates);
        let mut seen = HashSet::<NodeId>::default();
        candidates.retain_mut(|parent_id| {
            let Some(resolved) = self.first_non_virtual_ancestor(*parent_id) else {
                return false;
            };
            *parent_id = resolved;
            self.is_attached_to(resolved, root) && seen.insert(resolved)
        });
        candidates
    }

    /// Takes the recorded structural-change candidates into reusable storage.
    /// Candidates are not resolved or attachment-filtered; callers that route
    /// them into scenes should use [`Self::scene_nodes_attached_to_into`].
    ///
    /// ```
    /// use cranpose_core::MemoryApplier;
    ///
    /// let mut applier = MemoryApplier::new();
    /// let mut nodes = Vec::new();
    /// applier.take_structural_change_parents_into(&mut nodes);
    /// assert!(nodes.is_empty());
    /// ```
    pub fn take_structural_change_parents_into(&mut self, output: &mut Vec<NodeId>) {
        output.clear();
        std::mem::swap(&mut self.state.structural_change_parents, output);
    }

    fn first_non_virtual_ancestor(&mut self, node_id: NodeId) -> Option<NodeId> {
        if self.state.virtual_node_ids.is_empty() {
            return Some(node_id);
        }
        let mut current = node_id;
        for _ in 0..100_000 {
            if !self.state.virtual_node_ids.contains(&current) {
                return Some(current);
            }
            match self.get_mut(current) {
                Ok(node) => current = node.parent()?,
                Err(_) => return None,
            }
        }
        None
    }

    fn is_attached_to(&mut self, node_id: NodeId, root: NodeId) -> bool {
        let mut current = node_id;
        for _ in 0..100_000 {
            if current == root {
                return true;
            }
            match self.get_mut(current) {
                Ok(node) => match node.parent() {
                    Some(parent) => current = parent,
                    None => return false,
                },
                Err(_) => return false,
            }
        }
        false
    }

    /// Runs `f` on the node `id` as an `N`. Returns an error if the node does not exist or is not
    /// an `N`.
    pub fn with_node<N: Node + 'static, R>(
        &mut self,
        id: NodeId,
        f: impl FnOnce(&mut N) -> R,
    ) -> Result<R, NodeError> {
        let typed = self
            .get_mut(id)?
            .as_any_mut()
            .downcast_mut::<N>()
            .ok_or_else(|| NodeError::TypeMismatch {
                id,
                expected: std::any::type_name::<N>(),
            })?;
        Ok(f(typed))
    }

    /// Returns the number of live nodes.
    pub fn len(&self) -> usize {
        self.state.nodes.iter().filter(|n| n.is_some()).count()
    }

    /// Returns the number of slots in the dense node table, both live and free.
    pub fn capacity(&self) -> usize {
        self.state.nodes.len()
    }

    /// Returns the number of free slots in the dense node table.
    pub fn tombstone_count(&self) -> usize {
        self.state.nodes.iter().filter(|n| n.is_none()).count()
    }

    /// Returns the number of freed table slots that new nodes can reuse.
    pub fn freelist_len(&self) -> usize {
        self.state.free_ids.len()
    }

    /// Returns the number of pooled recycled nodes of all types.
    pub fn debug_recycled_node_count(&self) -> usize {
        self.total_recycled_node_count()
    }

    /// Returns the number of pooled recycled nodes of type `N`.
    pub fn debug_recycled_node_count_for<N: Node + 'static>(&self) -> usize {
        let key = TypeId::of::<N>();
        self.state
            .recycle_pools
            .get(&key)
            .map_or(0, RecyclePool::len)
    }

    /// Returns the sizes of the applier's tables, for leak and memory checks.
    pub fn debug_stats(&self) -> MemoryApplierDebugStats {
        MemoryApplierDebugStats {
            next_stable_id: self.state.next_stable_id,
            nodes_len: self.len(),
            nodes_cap: self.state.nodes.len(),
            physical_stable_ids_len: self.state.physical_stable_ids.len(),
            physical_stable_ids_cap: self.state.physical_stable_ids.capacity(),
            stable_to_physical_len: self.state.stable_index.live(),
            stable_to_physical_cap: self.state.stable_index.capacity(),
            stable_generations_len: self.state.stable_index.occupied(),
            stable_generations_cap: self.state.stable_index.capacity(),
            free_ids_len: self.state.free_ids.len(),
            free_ids_cap: self.state.free_ids.capacity(),
            high_id_nodes_len: self.high_id_nodes().count(),
            high_id_nodes_cap: self.state.high_ids.capacity(),
            high_id_generations_len: self.state.high_ids.len(),
            high_id_generations_cap: self.state.high_ids.capacity(),
            recycled_type_count: self
                .state
                .recycle_pools
                .values()
                .filter(|pool| pool.len() > 0)
                .count(),
            recycled_type_cap: self.state.recycle_pools.capacity(),
            recycled_node_count: self.total_recycled_node_count(),
            recycled_node_capacity: self.total_recycled_node_capacity(),
            warm_recycled_node_id_count: self.total_warm_recycled_node_id_count(),
            warm_recycled_node_id_capacity: self.total_warm_recycled_node_id_capacity(),
        }
    }

    /// Returns `true` if the applier holds no live nodes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Calls `visit` with every live node, dense and high-id alike.
    pub fn for_each_node_mut(&mut self, mut visit: impl FnMut(&mut dyn Node)) {
        for node in self.state.nodes.iter_mut().flatten() {
            visit(node.as_mut());
        }
        for node in self
            .state
            .high_ids
            .values_mut()
            .filter_map(|slot| slot.node.as_mut())
        {
            visit(node.as_mut());
        }
    }

    /// Returns the heap bytes that the live nodes take, including what they own.
    pub fn debug_live_node_heap_bytes(&self) -> usize {
        let dense_nodes = self
            .state
            .nodes
            .iter()
            .flatten()
            .map(|node| size_of_val(&**node) + node.debug_heap_bytes())
            .sum::<usize>();
        let high_id_nodes = self
            .high_id_nodes()
            .map(|node| size_of_val(node) + node.debug_heap_bytes())
            .sum::<usize>();
        dense_nodes + high_id_nodes
    }

    /// Returns the heap bytes that the pooled recycled nodes take, including what they own.
    pub fn debug_recycled_node_heap_bytes(&self) -> usize {
        self.state
            .recycle_pools
            .values()
            .flat_map(RecyclePool::nodes)
            .map(|node| size_of_val(&*node.node) + node.node.debug_heap_bytes())
            .sum::<usize>()
    }

    /// Gives the applier the runtime of its composition, so code that runs during layout with only
    /// the applier in hand can reach the runtime.
    pub fn set_runtime_handle(&mut self, handle: RuntimeHandle) {
        self.state.layout_runtime = Some(handle);
    }

    /// Removes the runtime handle.
    pub fn clear_runtime_handle(&mut self) {
        self.state.layout_runtime = None;
    }

    /// Returns the runtime handle set with `set_runtime_handle`, if any.
    pub fn runtime_handle(&self) -> Option<RuntimeHandle> {
        self.state.layout_runtime.clone()
    }

    fn total_recycled_node_count(&self) -> usize {
        self.state
            .recycle_pools
            .values()
            .map(RecyclePool::len)
            .sum()
    }

    fn total_recycled_node_capacity(&self) -> usize {
        self.state
            .recycle_pools
            .values()
            .map(RecyclePool::capacity)
            .sum()
    }

    fn total_warm_recycled_node_id_count(&self) -> usize {
        self.live_warm_recycled_origin_count()
            + self
                .state
                .recycle_pools
                .values()
                .map(|pool| pool.warm.len() + pool.returning.len())
                .sum::<usize>()
    }

    fn total_warm_recycled_node_id_capacity(&self) -> usize {
        self.live_warm_recycled_origin_capacity()
            + self
                .state
                .recycle_pools
                .values()
                .map(|pool| pool.warm.capacity() + pool.returning.capacity())
                .sum::<usize>()
    }

    fn remember_recycle_pool_limit(&mut self, key: TypeId, recycle_pool_limit: Option<usize>) {
        if recycle_pool_limit.is_some() || self.state.recycle_pools.contains_key(&key) {
            self.state.recycle_pools.entry(key).or_default().limit = recycle_pool_limit;
        }
    }

    fn warm_recycled_pool_len(&self, key: TypeId) -> usize {
        self.state
            .recycle_pools
            .get(&key)
            .map_or(0, |pool| pool.warm.len())
    }

    fn live_warm_recycled_origin_count(&self) -> usize {
        self.state
            .physical_warm_recycled_origins
            .iter()
            .zip(self.state.nodes.iter())
            .filter(|(warm_origin, node)| **warm_origin && node.is_some())
            .count()
            + self
                .state
                .high_ids
                .values()
                .filter(|slot| slot.warm_origin && slot.node.is_some())
                .count()
    }

    fn high_id_nodes(&self) -> impl Iterator<Item = &dyn Node> {
        self.state
            .high_ids
            .values()
            .filter_map(|slot| slot.node.as_deref())
    }

    fn live_warm_recycled_origin_capacity(&self) -> usize {
        self.state.physical_warm_recycled_origins.capacity() + self.state.high_ids.capacity()
    }

    fn push_recycled_node(
        &mut self,
        key: TypeId,
        recycle_pool_limit: Option<usize>,
        recycled: RecycledNode,
    ) {
        self.remember_recycle_pool_limit(key, recycle_pool_limit);
        let pool = self.state.recycle_pools.entry(key).or_default();
        pool.remember_prototype(recycled.node.as_ref());
        pool.push(recycled);
    }

    fn seed_recycled_node_shell_impl(
        &mut self,
        key: TypeId,
        recycle_pool_limit: Option<usize>,
        shell: Box<dyn Node>,
    ) {
        let limit = recycle_pool_limit.unwrap_or(usize::MAX);
        if self.warm_recycled_pool_len(key) >= limit {
            return;
        }

        self.remember_recycle_pool_limit(key, recycle_pool_limit);
        let stable_id = self.state.next_stable_id;
        self.state.next_stable_id = self.state.next_stable_id.saturating_add(1);
        let pool = self.state.recycle_pools.entry(key).or_default();
        pool.remember_prototype(shell.as_ref());
        pool.push_warm(RecycledNode::from_shell(stable_id, shell, true));
    }

    fn prune_stable_generations(&mut self) {
        let retained_len = self.state.stable_index.live() + self.total_recycled_node_count();
        if retained_len == self.state.stable_index.occupied() {
            return;
        }

        let recycled: HashSet<NodeId> = self
            .state
            .recycle_pools
            .values()
            .flat_map(RecyclePool::nodes)
            .map(RecycledNode::stable_id)
            .collect();
        self.state
            .stable_index
            .retain_retired(|stable_id| recycled.contains(&stable_id));
    }

    /// Returns an indented text outline of the tree below `root` for debugging, or a `(no root)`
    /// line when `root` is `None`.
    pub fn dump_tree(&self, root: Option<NodeId>) -> String {
        let mut output = String::new();
        if let Some(root_id) = root {
            self.dump_node(&mut output, root_id, 0);
        } else {
            output.push_str("(no root)\n");
        }
        output
    }

    fn dump_node(&self, output: &mut String, id: NodeId, depth: usize) {
        let indent = "  ".repeat(depth);
        let Ok(node) = self.get_ref(id) else {
            if let Some(physical_id) = self.resolve_node_index(id) {
                output.push_str(&format!(
                    "{indent}[{id}] (missing physical node {physical_id})\n"
                ));
            } else {
                output.push_str(&format!("{indent}[{id}] (missing)\n"));
            }
            return;
        };
        let type_name = std::any::type_name_of_val(node);
        output.push_str(&format!("{indent}[{id}] {type_name}\n"));

        let mut children = SmallVec::<[NodeId; 8]>::new();
        node.collect_children_into(&mut children);
        for child_id in children {
            self.dump_node(output, child_id, depth + 1);
        }
    }

    /// Moves into `out` the nodes the layout dirty bubbles started from
    /// since the last call, for a layout pass that starts from them, and
    /// forgets them. `false` when there were more than the applier keeps:
    /// `out` is then empty and the pass walks down from the root.
    pub fn take_layout_dirt_roots(&mut self, out: &mut Vec<NodeId>) -> bool {
        self.state.layout_dirt_roots.take_into(out)
    }

    #[inline]
    fn resolve_node_index(&self, id: NodeId) -> Option<usize> {
        self.state.stable_index.slot(id)
    }

    fn contains_node_id(&self, id: NodeId) -> bool {
        self.resolve_node_index(id).is_some() || self.high_id_node(id).is_ok()
    }

    fn insert_high_id_node(&mut self, stable_id: NodeId, node: Box<dyn Node>, warm_origin: bool) {
        let slot = self.state.high_ids.entry(stable_id).or_default();
        slot.node = Some(node);
        slot.warm_origin = warm_origin;
    }

    fn insert_available_with_id(&mut self, stable_id: NodeId, node: Box<dyn Node>) {
        if stable_id >= Self::HIGH_ID_THRESHOLD {
            self.insert_high_id_node(stable_id, node, false);
            return;
        }

        let physical_id = if let Some(Reverse(free_physical_id)) = self.state.free_ids.pop() {
            self.state.nodes[free_physical_id] = Some(node);
            self.state.physical_stable_ids[free_physical_id] = Self::pack_stable_id(stable_id);
            self.state.physical_warm_recycled_origins[free_physical_id] = false;
            free_physical_id
        } else {
            self.ensure_dense_node_storage_capacity();
            let physical_id = self.state.nodes.len();
            self.state.nodes.push(Some(node));
            self.state
                .physical_stable_ids
                .push(Self::pack_stable_id(stable_id));
            self.state.physical_warm_recycled_origins.push(false);
            physical_id
        };

        self.state.next_stable_id = self.state.next_stable_id.max(stable_id.saturating_add(1));
        self.state.physical_stable_ids[physical_id] = Self::pack_stable_id(stable_id);
        self.state.stable_index.set_slot(stable_id, physical_id);
    }

    /// Borrows a live node without requiring exclusive access to the tree.
    /// Returns [`NodeError::Missing`] when `id` does not name a live node.
    #[inline]
    pub fn get_ref(&self, id: NodeId) -> Result<&dyn Node, NodeError> {
        match self.resolve_node_index(id) {
            Some(physical_id) => self
                .state
                .nodes
                .get(physical_id)
                .and_then(Option::as_deref)
                .ok_or(NodeError::Missing { id }),
            None => self.high_id_node(id),
        }
    }

    fn high_id_node(&self, id: NodeId) -> Result<&dyn Node, NodeError> {
        self.state
            .high_ids
            .get(&id)
            .and_then(|slot| slot.node.as_deref())
            .ok_or(NodeError::Missing { id })
    }

    fn high_id_node_mut(&mut self, id: NodeId) -> Result<&mut dyn Node, NodeError> {
        self.state
            .high_ids
            .get_mut(&id)
            .and_then(|slot| slot.node.as_deref_mut())
            .ok_or(NodeError::Missing { id })
    }

    fn node_parent(&self, id: NodeId) -> Result<Option<NodeId>, NodeError> {
        Ok(self.get_ref(id)?.parent())
    }

    fn collect_owned_children(
        &self,
        node_id: NodeId,
        out: &mut SmallVec<[NodeId; 8]>,
    ) -> Result<(), NodeError> {
        self.get_ref(node_id)?.collect_owned_children_into(out);
        out.retain(|child_id| {
            self.node_parent(*child_id)
                .is_ok_and(|parent| parent == Some(node_id))
        });
        Ok(())
    }

    fn remove_node_storage(&mut self, node_id: NodeId) -> Result<(), NodeError> {
        self.state.virtual_node_ids.remove(&node_id);
        if let Some(slot) = self.state.high_ids.get_mut(&node_id)
            && let Some(node) = slot.node.take()
        {
            let warm_origin = std::mem::take(&mut slot.warm_origin);
            slot.generation = slot.generation.wrapping_add(1);
            if let Some(key) = node.recycle_key() {
                let recycle_pool_limit = node.recycle_pool_limit();
                self.push_recycled_node(
                    key,
                    recycle_pool_limit,
                    RecycledNode::new(node_id, node, warm_origin),
                );
            }
            return Ok(());
        }

        let physical_id = self
            .resolve_node_index(node_id)
            .ok_or(NodeError::Missing { id: node_id })?;
        if let Some(node) = self.state.nodes[physical_id].take()
            && let Some(key) = node.recycle_key()
        {
            let recycle_pool_limit = node.recycle_pool_limit();
            let warm_origin = self
                .state
                .physical_warm_recycled_origins
                .get_mut(physical_id)
                .is_some_and(std::mem::take);
            self.push_recycled_node(
                key,
                recycle_pool_limit,
                RecycledNode::new(node_id, node, warm_origin),
            );
        }
        self.state.physical_stable_ids[physical_id] = Self::INVALID_STABLE_ID;
        self.state.stable_index.release(node_id);
        self.state.free_ids.push(Reverse(physical_id));
        Ok(())
    }

    fn remove_subtree_postorder(&mut self, id: NodeId) -> Result<usize, NodeError> {
        self.get_ref(id)?;

        let mut root_children = SmallVec::<[NodeId; 8]>::new();
        self.collect_owned_children(id, &mut root_children)?;

        let mut stack = Vec::new();
        stack.push(RemovalFrame {
            node_id: id,
            children: root_children,
            next_child: 0,
        });
        let mut max_depth = stack.len();

        while let Some(frame) = stack.last_mut() {
            if frame.next_child < frame.children.len() {
                let child_id = frame.children[frame.next_child];
                frame.next_child += 1;

                if let Ok(child) = self.get_mut(child_id) {
                    child.on_removed_from_parent();
                    child.unmount();
                }

                let mut child_children = SmallVec::<[NodeId; 8]>::new();
                self.collect_owned_children(child_id, &mut child_children)?;
                stack.push(RemovalFrame {
                    node_id: child_id,
                    children: child_children,
                    next_child: 0,
                });
                max_depth = max_depth.max(stack.len());
                continue;
            }

            let node_id = frame.node_id;
            stack.pop();
            self.remove_node_storage(node_id)?;
        }

        Ok(max_depth)
    }

    #[cfg(test)]
    fn debug_remove_max_traversal_depth(&mut self, id: NodeId) -> Result<usize, NodeError> {
        self.remove_subtree_postorder(id)
    }
}

impl Applier for MemoryApplier {
    fn record_structural_change(&mut self, parent_id: NodeId) {
        if self.state.structural_change_parents.last() != Some(&parent_id) {
            self.state.structural_change_parents.push(parent_id);
        }
    }

    fn create(&mut self, node: Box<dyn Node>) -> NodeId {
        let stable_id = self.state.next_stable_id;
        self.state.next_stable_id = self.state.next_stable_id.saturating_add(1);
        if stable_id >= Self::HIGH_ID_THRESHOLD {
            self.insert_high_id_node(stable_id, node, false);
            return stable_id;
        }

        let physical_id = if let Some(Reverse(id)) = self.state.free_ids.pop() {
            debug_assert!(
                self.state.nodes[id].is_none(),
                "freelist entry {id} is not None"
            );
            self.state.nodes[id] = Some(node);
            self.state.physical_stable_ids[id] = Self::pack_stable_id(stable_id);
            self.state.physical_warm_recycled_origins[id] = false;
            id
        } else {
            self.ensure_dense_node_storage_capacity();
            let id = self.state.nodes.len();
            self.state.nodes.push(Some(node));
            self.state
                .physical_stable_ids
                .push(Self::pack_stable_id(stable_id));
            self.state.physical_warm_recycled_origins.push(false);
            id
        };
        self.state.stable_index.insert_fresh(stable_id, physical_id);
        stable_id
    }

    fn node_generation(&self, id: NodeId) -> u32 {
        if id >= Self::HIGH_ID_THRESHOLD {
            return self
                .state
                .high_ids
                .get(&id)
                .map_or(0, |slot| slot.generation);
        }
        self.state.stable_index.generation(id)
    }

    #[inline]
    fn get_mut(&mut self, id: NodeId) -> Result<&mut dyn Node, NodeError> {
        match self.resolve_node_index(id) {
            Some(physical_id) => self
                .state
                .nodes
                .get_mut(physical_id)
                .and_then(Option::as_deref_mut)
                .ok_or(NodeError::Missing { id }),
            None => self.high_id_node_mut(id),
        }
    }

    fn remove(&mut self, id: NodeId) -> Result<(), NodeError> {
        self.state.bubble_passes.forget();
        self.remove_subtree_postorder(id).map(|_| ())
    }

    fn begin_bubble_batch(&mut self) {
        self.state.bubble_passes.begin_batch();
    }

    fn note_layout_dirt(&mut self, id: NodeId) {
        self.state.layout_dirt_roots.note(id);
    }

    fn end_bubble_batch(&mut self) {
        self.state.bubble_passes.end_batch();
    }

    fn forget_bubble_passes(&mut self) {
        self.state.bubble_passes.forget();
    }

    fn note_bubble_pass(&mut self, id: NodeId, flags: u8) -> bool {
        self.resolve_node_index(id)
            .is_some_and(|index| self.state.bubble_passes.note(index, flags))
    }

    /// One lookup of the node's slot serves the pass note and the node.
    fn bubble_node(&mut self, id: NodeId, flags: u8, note: bool) -> Option<&mut dyn Node> {
        let Some(index) = self.resolve_node_index(id) else {
            return self
                .state
                .high_ids
                .get_mut(&id)
                .and_then(|slot| slot.node.as_deref_mut());
        };
        if note && self.state.bubble_passes.note(index, flags) {
            return None;
        }
        self.state.nodes.get_mut(index)?.as_deref_mut()
    }

    fn get_ref(&self, id: NodeId) -> Result<&dyn Node, NodeError> {
        Self::get_ref(self, id)
    }

    fn create_virtual(&mut self, node: Box<dyn Node>) -> NodeId {
        let id = self.create(node);
        self.state.virtual_node_ids.insert(id);
        id
    }

    fn insert_recycled_node_or_create(
        &mut self,
        stable_id: NodeId,
        node: Box<dyn Node>,
    ) -> RecycledNodeInsertion {
        if self.contains_node_id(stable_id) {
            let id = self.create(node);
            return RecycledNodeInsertion::fresh(
                id,
                Some(NodeError::AlreadyExists { id: stable_id }),
            );
        }

        self.insert_available_with_id(stable_id, node);
        RecycledNodeInsertion::reused(stable_id)
    }

    fn compact(&mut self) {
        let live_count = self
            .state
            .nodes
            .iter()
            .filter(|slot| slot.is_some())
            .count();
        let tombstone_count = self.state.nodes.len().saturating_sub(live_count);
        if tombstone_count == 0 {
            return;
        }
        if self.state.nodes.len() > Self::EAGER_COMPACT_NODE_LEN && tombstone_count < live_count {
            return;
        }
        let rehouse_live_nodes = tombstone_count >= live_count;
        let mut packed_nodes = Vec::with_capacity(live_count);
        let mut packed_physical_stable_ids = Vec::with_capacity(live_count);
        let mut packed_warm_recycled_origins = Vec::with_capacity(live_count);

        for physical_id in 0..self.state.nodes.len() {
            let Some(mut node) = self.state.nodes[physical_id].take() else {
                continue;
            };
            if rehouse_live_nodes && let Some(rehoused) = node.rehouse_for_live_compaction() {
                node = rehoused;
            }
            let stable_id = std::mem::replace(
                &mut self.state.physical_stable_ids[physical_id],
                Self::INVALID_STABLE_ID,
            );
            debug_assert_ne!(
                stable_id,
                Self::INVALID_STABLE_ID,
                "live physical slot must have a stable id",
            );
            let stable_id = Self::unpack_stable_id(stable_id);
            packed_nodes.push(Some(node));
            packed_physical_stable_ids.push(Self::pack_stable_id(stable_id));
            packed_warm_recycled_origins
                .push(self.state.physical_warm_recycled_origins[physical_id]);
            self.state
                .stable_index
                .set_slot(stable_id, packed_nodes.len() - 1);
        }

        self.state.nodes = packed_nodes;
        self.state.physical_stable_ids = packed_physical_stable_ids;
        self.state.physical_warm_recycled_origins = packed_warm_recycled_origins;
        self.state.free_ids = BinaryHeap::new();
        debug_assert_eq!(
            self.state.stable_index.live(),
            live_count,
            "every live node keeps one indexed slot through compaction",
        );
        self.prune_stable_generations();
    }

    fn take_recycled_node(&mut self, key: TypeId) -> Option<RecycledNode> {
        self.state.recycle_pools.get_mut(&key)?.take()
    }

    fn set_recycled_node_origin(&mut self, id: NodeId, warm_origin: bool) {
        if let Some(physical_id) = self.resolve_node_index(id) {
            self.state.physical_warm_recycled_origins[physical_id] = warm_origin;
        } else if let Some(slot) = self.state.high_ids.get_mut(&id)
            && slot.node.is_some()
        {
            slot.warm_origin = warm_origin;
        }
    }

    fn seed_recycled_node_shell(
        &mut self,
        key: TypeId,
        recycle_pool_limit: Option<usize>,
        shell: Box<dyn Node>,
    ) {
        self.seed_recycled_node_shell_impl(key, recycle_pool_limit, shell);
    }

    fn record_fresh_recyclable_creation(&mut self, key: TypeId) {
        self.state
            .recycle_pools
            .entry(key)
            .or_default()
            .fresh_creations += 1;
    }

    fn clear_recycled_nodes(&mut self) {
        let next_stable_id = &mut self.state.next_stable_id;
        for pool in self.state.recycle_pools.values_mut() {
            pool.end_pass(next_stable_id);
        }
        self.prune_stable_generations();
        self.compact();
    }
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
