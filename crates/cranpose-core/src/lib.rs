#![doc = r"Core runtime pieces for the Cranpose experiment."]

pub extern crate self as cranpose_core;

mod callbacks;
mod composer;
pub mod composer_context;
mod composition;
mod composition_locals;
mod debug_trace;
mod emit;
#[cfg(any(feature = "internal", test))]
mod frame_clock;
mod hooks;
mod launched_effect;
pub mod owned;
pub mod platform;
mod recompose;
mod retention;
pub mod runtime;
mod slot;
mod slot_storage;
pub mod snapshot_double_index_heap;
pub mod snapshot_id_set;
pub mod snapshot_pinning;
pub mod snapshot_state_observer;
pub mod snapshot_v2;
mod snapshot_weak_set;
mod state;
pub mod subcompose;

#[cfg(feature = "internal")]
#[doc(hidden)]
pub mod internal {
    pub use crate::frame_clock::{FrameCallbackRegistration, FrameClock};
}
pub use callbacks::{CallbackHolder, CallbackHolder1, ParamSlot, ParamState, ReturnSlot};
pub use composer::{Composer, ValueSlotHandle};
pub(crate) use composer::{ComposerCore, EmittedNode, ParentAttachMode, ParentFrame};
pub use composition::{Composition, ROOT_RENDER_REPLAY_LIMIT};
pub use composition_locals::{
    compositionLocalOf, compositionLocalOfWithPolicy, staticCompositionLocalOf, CompositionLocal,
    CompositionLocalProvider, ProvidedValue, StaticCompositionLocal,
};
pub(crate) use composition_locals::{LocalStateEntry, StaticLocalEntry};
#[doc(hidden)]
pub use debug_trace::{
    debug_label_current_scope, debug_live_recompose_scope_count,
    debug_recompose_scope_registry_stats, debug_scope_invalidation_sources, debug_scope_label,
};
pub use hooks::{
    derivedStateOf, mutableStateList, mutableStateListOf, mutableStateMap, mutableStateMapOf,
    mutableStateOf, ownedMutableStateOf, remember, rememberUpdatedState, try_mutableStateOf,
    useState,
};
#[cfg(feature = "internal")]
#[doc(hidden)]
pub use hooks::{withFrameMillis, withFrameNanos};
pub use launched_effect::{
    __launched_effect_async_impl, __launched_effect_impl, CancelToken, LaunchedEffectScope,
};
pub use owned::Owned;
pub use platform::{Clock, RuntimeScheduler};
pub use retention::{RetentionBudget, RetentionEvictionPolicy, RetentionMode, RetentionPolicy};
#[doc(hidden)]
pub use runtime::{
    current_runtime_handle, schedule_frame, schedule_node_update, DefaultScheduler, Runtime,
    RuntimeHandle, StateId, TaskHandle,
};
pub use slot::{
    SlotDebugAnchor, SlotDebugEntry, SlotDebugEntryKind, SlotDebugGroup, SlotDebugScope,
    SlotDebugSnapshot, SlotTable, SlotTableDebugStats, SlotTableMutationDebugStats,
};
pub use slot_storage::{
    BeginGroupInput, GroupAnchor, GroupId, GroupKey, GroupStart, GroupStartKind, NodeRecordResult,
    ValueSlotId,
};
#[doc(hidden)]
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

    // Mark that we're in an applied snapshot context
    IN_APPLIED_SNAPSHOT.with(|c| c.set(true));
    let value = snapshot.enter(block);
    IN_APPLIED_SNAPSHOT.with(|c| c.set(false));

    match snapshot.apply() {
        snapshot_v2::SnapshotApplyResult::Success => Ok(value),
        snapshot_v2::SnapshotApplyResult::Failure => Err("Snapshot apply failed"),
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

// ─── Event Handler Context Tracking ─────────────────────────────────────────
//
// These thread-locals track whether code is running in an event handler context
// and whether it's properly wrapped in run_in_mutable_snapshot. This allows
// debug-mode warnings when state is modified without proper snapshot handling.

thread_local! {
    /// Tracks if we're in a UI event handler context (keyboard, mouse, etc.)
    pub(crate) static IN_EVENT_HANDLER: Cell<bool> = const { Cell::new(false) };
    /// Tracks if we're in a properly-applied mutable snapshot
    pub(crate) static IN_APPLIED_SNAPSHOT: Cell<bool> = const { Cell::new(false) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompositionPassDebugStats {
    pub commands_len: usize,
    pub commands_cap: usize,
    pub command_payload_len_bytes: usize,
    pub command_payload_cap_bytes: usize,
    pub sync_children_len: usize,
    pub sync_children_cap: usize,
    pub sync_child_ids_len: usize,
    pub sync_child_ids_cap: usize,
    pub side_effects_len: usize,
    pub side_effects_cap: usize,
}

/// Marks the start of an event handler context.
/// Call this at the start of keyboard/mouse/touch event handling.
/// Use `run_in_mutable_snapshot` or `dispatch_ui_event` inside to ensure proper snapshot handling.
pub fn enter_event_handler() {
    IN_EVENT_HANDLER.with(|c| c.set(true));
}

/// Marks the end of an event handler context.
pub fn exit_event_handler() {
    IN_EVENT_HANDLER.with(|c| c.set(false));
}

/// Returns true if currently in an event handler context.
pub fn in_event_handler() -> bool {
    IN_EVENT_HANDLER.with(|c| c.get())
}

/// Returns true if currently in an applied snapshot context.
pub fn in_applied_snapshot() -> bool {
    IN_APPLIED_SNAPSHOT.with(|c| c.get())
}

#[cfg(test)]
pub use runtime::{TestRuntime, TestScheduler};

use crate::collections::map::{HashMap, HashSet};
use smallvec::SmallVec;
use std::any::{Any, TypeId};
use std::cell::{Cell, Ref, RefCell, RefMut};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(any(test, debug_assertions))]
use std::sync::{Mutex, OnceLock};

pub type Key = u64;
pub type NodeId = usize;

#[cfg(any(test, debug_assertions))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct LocationKeyDebugInfo {
    file: String,
    line: u32,
    column: u32,
}

#[cfg(any(test, debug_assertions))]
fn location_key_registry() -> &'static Mutex<HashMap<Key, LocationKeyDebugInfo>> {
    static REGISTRY: OnceLock<Mutex<HashMap<Key, LocationKeyDebugInfo>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::default()))
}

#[cfg(any(test, debug_assertions))]
fn lock_location_key_registry() -> std::sync::MutexGuard<'static, HashMap<Key, LocationKeyDebugInfo>>
{
    location_key_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(any(test, debug_assertions))]
fn register_location_key_debug_info(key: Key, file: &str, line: u32, column: u32) {
    let info = LocationKeyDebugInfo {
        file: file.to_owned(),
        line,
        column,
    };
    let collision = {
        let mut registry = lock_location_key_registry();
        match registry.entry(key) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(info);
                None
            }
            std::collections::hash_map::Entry::Occupied(entry) => {
                let existing = entry.get();
                (existing != &info).then(|| (existing.clone(), info))
            }
        }
    };
    if let Some((existing, incoming)) = collision {
        panic!("location key collision: key={key} first={existing:?} second={incoming:?}");
    }
}

#[cfg(all(debug_assertions, not(test)))]
fn location_key_diagnostics_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_LOCATION_KEY_DIAGNOSTICS").is_some())
}

#[cfg(test)]
pub(crate) fn register_location_key_debug_info_for_test(
    key: Key,
    file: &str,
    line: u32,
    column: u32,
) {
    register_location_key_debug_info(key, file, line, column);
}

#[cfg(test)]
pub(crate) fn slot_validation_diagnostics_enabled() -> bool {
    true
}

#[cfg(all(debug_assertions, not(test)))]
pub(crate) fn slot_validation_diagnostics_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_VALIDATE_SLOTS").is_some())
}

#[cfg(not(any(test, debug_assertions)))]
pub(crate) fn slot_validation_diagnostics_enabled() -> bool {
    false
}

fn source_location_key(file: &str, line: u32, column: u32) -> Key {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    hash = fnv1a_location_key_bytes(hash, file.as_bytes());
    hash = fnv1a_location_key_bytes(hash, &[0xff]);
    hash = fnv1a_location_key_bytes(hash, &line.to_le_bytes());
    hash = fnv1a_location_key_bytes(hash, &[0xfe]);
    hash = fnv1a_location_key_bytes(hash, &column.to_le_bytes());
    avalanche_location_key(hash)
}

fn fnv1a_location_key_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn avalanche_location_key(mut value: u64) -> u64 {
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51_afd7_ed55_8ccd);
    value ^= value >> 33;
    value = value.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    value ^ (value >> 33)
}

pub fn location_key(file: &str, line: u32, column: u32) -> Key {
    let key = source_location_key(file, line, column);
    #[cfg(test)]
    register_location_key_debug_info(key, file, line, column);
    #[cfg(all(debug_assertions, not(test)))]
    if location_key_diagnostics_enabled() {
        register_location_key_debug_info(key, file, line, column);
    }
    key
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
    /// Invalid anchor that represents no anchor.
    pub(crate) const INVALID: AnchorId = AnchorId {
        id: 0,
        generation: 0,
    };

    pub(crate) fn new(id: usize) -> Self {
        Self {
            id: id as u32,
            generation: 1,
        }
    }

    /// Check if this anchor is valid (non-zero).
    pub fn is_valid(&self) -> bool {
        self.id != 0
    }
}

pub(crate) type ScopeId = usize;
type LocalKey = usize;
pub(crate) type FrameCallbackId = u64;
type LocalStackSnapshot = Rc<Vec<composer::LocalContext>>;

thread_local! {
    static EMPTY_LOCAL_STACK: LocalStackSnapshot = Rc::new(Vec::new());
    #[cfg(debug_assertions)]
    static DEBUG_SCOPE_LABELS: RefCell<HashMap<usize, &'static str>> = RefCell::new(HashMap::default());
    #[cfg(debug_assertions)]
    static DEBUG_SCOPE_INVALIDATION_SOURCES: RefCell<HashMap<usize, HashSet<String>>> =
        RefCell::new(HashMap::default());
    #[cfg(all(test, debug_assertions))]
    static DEBUG_SCOPE_TRACKING_OVERRIDE: Cell<Option<bool>> = const { Cell::new(None) };
}

static NEXT_SCOPE_ID: AtomicUsize = AtomicUsize::new(1);
static NEXT_LOCAL_KEY: AtomicUsize = AtomicUsize::new(1);
static LIVE_RECOMPOSE_SCOPE_COUNT: AtomicUsize = AtomicUsize::new(0);

fn next_scope_id() -> ScopeId {
    NEXT_SCOPE_ID.fetch_add(1, Ordering::Relaxed)
}

fn next_local_key() -> LocalKey {
    NEXT_LOCAL_KEY.fetch_add(1, Ordering::Relaxed)
}

fn empty_local_stack() -> LocalStackSnapshot {
    EMPTY_LOCAL_STACK.with(Rc::clone)
}

enum RecomposeCallback {
    Static(fn(&Composer)),
    Dynamic(Box<dyn FnMut(&Composer) + 'static>),
}

pub(crate) struct RecomposeScopeInner {
    id: ScopeId,
    runtime: RuntimeHandle,
    invalid: Cell<bool>,
    enqueued: Cell<bool>,
    active: Cell<bool>,
    composed_once: Cell<bool>,
    pending_recompose: Cell<bool>,
    force_reuse: Cell<bool>,
    force_recompose: Cell<bool>,
    retention_mode: Cell<RetentionMode>,
    group_anchor: Cell<AnchorId>,
    parent_hint: Cell<Option<NodeId>>,
    recompose: RefCell<Option<RecomposeCallback>>,
    parent_scope: RefCell<Option<Weak<RecomposeScopeInner>>>,
    local_stack: RefCell<LocalStackSnapshot>,
    slots_storage_key: Cell<usize>,
    slots_runtime_state: RefCell<Option<std::rc::Weak<crate::composer::ComposerRuntimeState>>>,
    state_subscriptions: RefCell<HashSet<StateId>>,
}

impl RecomposeScopeInner {
    fn new(runtime: RuntimeHandle) -> Self {
        LIVE_RECOMPOSE_SCOPE_COUNT.fetch_add(1, Ordering::Relaxed);
        Self {
            id: next_scope_id(),
            runtime,
            invalid: Cell::new(false),
            enqueued: Cell::new(false),
            active: Cell::new(true),
            composed_once: Cell::new(false),
            pending_recompose: Cell::new(false),
            force_reuse: Cell::new(false),
            force_recompose: Cell::new(false),
            retention_mode: Cell::new(RetentionMode::DisposeWhenInactive),
            group_anchor: Cell::new(AnchorId::INVALID),
            parent_hint: Cell::new(None),
            recompose: RefCell::new(None),
            parent_scope: RefCell::new(None),
            local_stack: RefCell::new(empty_local_stack()),
            slots_storage_key: Cell::new(0),
            slots_runtime_state: RefCell::new(None),
            state_subscriptions: RefCell::new(HashSet::default()),
        }
    }
}

impl Drop for RecomposeScopeInner {
    fn drop(&mut self) {
        LIVE_RECOMPOSE_SCOPE_COUNT.fetch_sub(1, Ordering::Relaxed);
        let subscriptions = std::mem::take(self.state_subscriptions.get_mut());
        for state_id in subscriptions {
            self.runtime.unregister_state_scope(state_id, self.id);
        }
        #[cfg(debug_assertions)]
        {
            let _ = DEBUG_SCOPE_LABELS.try_with(|labels| {
                labels.borrow_mut().remove(&self.id);
            });
            let _ = DEBUG_SCOPE_INVALIDATION_SOURCES.try_with(|sources| {
                sources.borrow_mut().remove(&self.id);
            });
        }
        if self.enqueued.replace(false) {
            self.runtime.mark_scope_recomposed(self.id);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecomposeScopeRegistryDebugStats {
    pub len: usize,
    pub capacity: usize,
}

#[derive(Clone)]
pub struct RecomposeScope {
    inner: Rc<RecomposeScopeInner>,
}

impl PartialEq for RecomposeScope {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for RecomposeScope {}

impl Hash for RecomposeScope {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

impl RecomposeScope {
    fn new(runtime: RuntimeHandle) -> Self {
        Self {
            inner: Rc::new(RecomposeScopeInner::new(runtime)),
        }
    }

    pub(crate) fn downgrade(&self) -> Weak<RecomposeScopeInner> {
        Rc::downgrade(&self.inner)
    }

    pub fn id(&self) -> ScopeId {
        self.inner.id
    }

    pub fn is_invalid(&self) -> bool {
        self.inner.invalid.get()
    }

    pub fn is_active(&self) -> bool {
        self.inner.active.get()
    }

    fn record_state_subscription(&self, state_id: StateId) {
        self.inner.state_subscriptions.borrow_mut().insert(state_id);
    }

    fn invalidate(&self) {
        self.inner.invalid.set(true);
        if !self.inner.active.get() {
            return;
        }
        if !self.inner.enqueued.replace(true) {
            self.inner
                .runtime
                .register_invalid_scope(self.inner.id, self.downgrade());
        }
    }

    fn mark_recomposed(&self) {
        self.inner.invalid.set(false);
        self.inner.force_reuse.set(false);
        self.inner.force_recompose.set(false);
        if self.inner.enqueued.replace(false) {
            self.inner.runtime.mark_scope_recomposed(self.inner.id);
        }
        let pending = self.inner.pending_recompose.replace(false);
        if pending {
            if self.inner.active.get() {
                self.invalidate();
            } else {
                self.inner.invalid.set(true);
            }
        }
    }

    fn set_recompose(&self, callback: Box<dyn FnMut(&Composer) + 'static>) {
        *self.inner.recompose.borrow_mut() = Some(RecomposeCallback::Dynamic(callback));
    }

    fn set_recompose_fn(&self, callback: fn(&Composer)) {
        *self.inner.recompose.borrow_mut() = Some(RecomposeCallback::Static(callback));
    }

    /// Returns true if a callback was found and executed, false if no callback was registered.
    fn run_recompose(&self, composer: &Composer) -> bool {
        let callback = self.inner.recompose.borrow_mut().take();
        if let Some(callback) = callback {
            match callback {
                RecomposeCallback::Static(callback) => callback(composer),
                RecomposeCallback::Dynamic(mut callback) => callback(composer),
            }
            true
        } else {
            false
        }
    }

    fn has_recompose_callback(&self) -> bool {
        self.inner.recompose.borrow().is_some()
    }

    fn set_group_anchor(&self, anchor: AnchorId) {
        self.inner.group_anchor.set(anchor);
    }

    #[cfg(test)]
    pub(crate) fn group_anchor(&self) -> AnchorId {
        self.inner.group_anchor.get()
    }

    fn snapshot_locals(&self, stack: LocalStackSnapshot) {
        *self.inner.local_stack.borrow_mut() = stack;
    }

    fn local_stack(&self) -> LocalStackSnapshot {
        self.inner.local_stack.borrow().clone()
    }

    fn set_parent_hint(&self, parent: Option<NodeId>) {
        self.inner.parent_hint.set(parent);
    }

    fn set_parent_scope(&self, parent: Option<RecomposeScope>) {
        *self.inner.parent_scope.borrow_mut() = parent.map(|scope| scope.downgrade());
    }

    fn parent_scope(&self) -> Option<RecomposeScope> {
        self.inner
            .parent_scope
            .borrow()
            .as_ref()
            .and_then(Weak::upgrade)
            .map(|inner| RecomposeScope { inner })
    }

    fn callback_promotion_target(&self) -> Option<RecomposeScope> {
        let mut current = self.parent_scope();
        while let Some(scope) = current {
            if scope.has_recompose_callback() {
                return Some(scope);
            }
            current = scope.parent_scope();
        }
        None
    }

    fn parent_hint(&self) -> Option<NodeId> {
        self.inner.parent_hint.get()
    }

    fn set_slots_host(&self, host: &Rc<SlotsHost>) {
        self.inner.slots_storage_key.set(host.storage_key());
        *self.inner.slots_runtime_state.borrow_mut() =
            host.runtime_state().map(|state| Rc::downgrade(&state));
    }

    pub(crate) fn slots_storage_key(&self) -> Option<usize> {
        let key = self.inner.slots_storage_key.get();
        (key != 0).then_some(key)
    }

    pub(crate) fn slots_runtime_state(&self) -> Option<Rc<crate::composer::ComposerRuntimeState>> {
        self.inner
            .slots_runtime_state
            .borrow()
            .as_ref()
            .and_then(std::rc::Weak::upgrade)
    }

    pub fn deactivate(&self) {
        if !self.inner.active.replace(false) {
            return;
        }
        if self.inner.enqueued.replace(false) {
            self.inner.runtime.mark_scope_recomposed(self.inner.id);
        }
    }

    pub fn reactivate(&self) {
        if self.inner.active.replace(true) {
            return;
        }
        if self.inner.invalid.get() && !self.inner.enqueued.replace(true) {
            self.inner
                .runtime
                .register_invalid_scope(self.inner.id, self.downgrade());
        }
    }

    pub fn force_reuse(&self) {
        self.inner.force_reuse.set(true);
        self.inner.force_recompose.set(false);
        self.inner.pending_recompose.set(true);
    }

    pub fn force_recompose(&self) {
        self.inner.force_recompose.set(true);
        self.inner.force_reuse.set(false);
        self.inner.pending_recompose.set(false);
    }

    pub(crate) fn set_retention_mode(&self, mode: RetentionMode) {
        self.inner.retention_mode.set(mode);
    }

    pub(crate) fn retention_mode(&self) -> RetentionMode {
        self.inner.retention_mode.get()
    }

    pub fn should_recompose(&self) -> bool {
        if self.inner.force_recompose.replace(false) {
            self.inner.force_reuse.set(false);
            return true;
        }
        if self.inner.force_reuse.replace(false) {
            return false;
        }
        self.is_invalid()
    }

    pub fn has_composed_once(&self) -> bool {
        self.inner.composed_once.get()
    }

    fn mark_composed_once(&self) {
        self.inner.composed_once.set(true);
    }
}

#[cfg(test)]
impl RecomposeScope {
    pub(crate) fn new_for_test(runtime: RuntimeHandle) -> Self {
        Self::new(runtime)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RecomposeOptions {
    pub force_reuse: bool,
    pub force_recompose: bool,
    pub retention: RetentionMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeError {
    Missing { id: NodeId },
    TypeMismatch { id: NodeId, expected: &'static str },
    MissingContext { id: NodeId, reason: &'static str },
    AlreadyExists { id: NodeId },
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeError::Missing { id } => write!(f, "node {id} missing"),
            NodeError::TypeMismatch { id, expected } => {
                write!(f, "node {id} type mismatch; expected {expected}")
            }
            NodeError::MissingContext { id, reason } => {
                write!(f, "missing context for node {id}: {reason}")
            }
            NodeError::AlreadyExists { id } => {
                write!(f, "node {id} already exists")
            }
        }
    }
}

impl std::error::Error for NodeError {}

pub use subcompose::{
    ContentTypeReusePolicy, DefaultSlotReusePolicy, SlotId, SlotReusePolicy, SubcomposeState,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Compose,
    Measure,
    Layout,
}

pub use composer_context::with_composer as with_current_composer;

#[allow(non_snake_case)]
pub fn withCurrentComposer<R>(f: impl FnOnce(&Composer) -> R) -> R {
    composer_context::with_composer(f)
}

fn with_current_composer_opt<R>(f: impl FnOnce(&Composer) -> R) -> Option<R> {
    composer_context::try_with_composer(f)
}

#[track_caller]
pub fn with_key<K: Hash>(key: &K, content: impl FnOnce()) {
    let seed = explicit_group_key_seed(key, std::panic::Location::caller());
    with_current_composer(|composer| composer.with_group_seed(seed, |_| content()));
}

#[allow(non_snake_case)]
pub fn withKey<K: Hash>(key: &K, content: impl FnOnce()) {
    with_key(key, content)
}

#[derive(Default)]
struct DisposableEffectState {
    key: Option<Key>,
    cleanup: Option<Box<dyn FnOnce()>>,
}

impl DisposableEffectState {
    fn should_run(&self, key: Key) -> bool {
        match self.key {
            Some(current) => current != key,
            None => true,
        }
    }

    fn set_key(&mut self, key: Key) {
        self.key = Some(key);
    }

    fn set_cleanup(&mut self, cleanup: Option<Box<dyn FnOnce()>>) {
        self.cleanup = cleanup;
    }

    fn run_cleanup(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}

impl Drop for DisposableEffectState {
    fn drop(&mut self) {
        self.run_cleanup();
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DisposableEffectScope;

#[derive(Default)]
pub struct DisposableEffectResult {
    cleanup: Option<Box<dyn FnOnce()>>,
}

impl DisposableEffectScope {
    pub fn on_dispose(&self, cleanup: impl FnOnce() + 'static) -> DisposableEffectResult {
        DisposableEffectResult::new(cleanup)
    }
}

impl DisposableEffectResult {
    pub fn new(cleanup: impl FnOnce() + 'static) -> Self {
        Self {
            cleanup: Some(Box::new(cleanup)),
        }
    }

    fn into_cleanup(self) -> Option<Box<dyn FnOnce()>> {
        self.cleanup
    }
}

#[allow(non_snake_case)]
pub fn SideEffect(effect: impl FnOnce() + 'static) {
    with_current_composer(|composer| composer.register_side_effect(effect));
}

pub fn __disposable_effect_impl<K, F>(group_key: Key, keys: K, effect: F)
where
    K: Hash,
    F: FnOnce(DisposableEffectScope) -> DisposableEffectResult + 'static,
{
    // Create a group using the caller's location to ensure each DisposableEffect
    // gets its own slot table entry, even in conditional branches
    with_current_composer(|composer| {
        composer.with_group(group_key, |composer| {
            let key_hash = hash_key(&keys);
            let state = composer.remember_effect(DisposableEffectState::default);
            if state.with(|state| state.should_run(key_hash)) {
                state.update(|state| {
                    state.run_cleanup();
                    state.set_key(key_hash);
                });
                let state_for_effect = state.clone();
                let mut effect_opt = Some(effect);
                composer.register_side_effect(move || {
                    if let Some(effect) = effect_opt.take() {
                        let result = effect(DisposableEffectScope);
                        state_for_effect.update(|state| state.set_cleanup(result.into_cleanup()));
                    }
                });
            }
        });
    });
}

#[macro_export]
macro_rules! DisposableEffect {
    ($keys:expr, $effect:expr) => {
        $crate::__disposable_effect_impl(
            $crate::location_key(file!(), line!(), column!()),
            $keys,
            $effect,
        )
    };
}

#[macro_export]
macro_rules! clone_captures {
    ($($alias:ident $(= $value:expr)?),+ $(,)?; $body:expr) => {{
        $(let $alias = $crate::clone_captures!(@clone $alias $(= $value)?);)+
        $body
    }};
    (@clone $alias:ident = $value:expr) => {
        ($value).clone()
    };
    (@clone $alias:ident) => {
        $alias.clone()
    };
}

pub fn with_node_mut<N: Node + 'static, R>(
    id: NodeId,
    f: impl FnOnce(&mut N) -> R,
) -> Result<R, NodeError> {
    with_current_composer(|composer| composer.with_node_mut(id, f))
}

pub fn push_parent(id: NodeId) {
    with_current_composer(|composer| composer.push_parent(id));
}

pub fn pop_parent() {
    with_current_composer(|composer| composer.pop_parent());
}

// ═══════════════════════════════════════════════════════════════════════════
// Public slot storage helper types
// ═══════════════════════════════════════════════════════════════════════════

pub trait Node: Any {
    fn mount(&mut self) {}
    fn update(&mut self) {}
    fn unmount(&mut self) {}
    fn insert_child(&mut self, _child: NodeId) {}
    fn remove_child(&mut self, _child: NodeId) {}
    fn move_child(&mut self, _from: usize, _to: usize) {}
    fn update_children(&mut self, _children: &[NodeId]) {}
    fn children(&self) -> Vec<NodeId> {
        Vec::new()
    }
    /// Copies child IDs into the provided scratch buffer without allocating a
    /// fresh container for every traversal.
    fn collect_children_into(&self, out: &mut SmallVec<[NodeId; 8]>) {
        out.clear();
        out.extend(self.children());
    }
    /// Called after the node is created to record its own ID.
    /// Useful for nodes that need to store their ID for later operations.
    fn set_node_id(&mut self, _id: NodeId) {}
    /// Called when this node is attached to a parent.
    /// Nodes with parent tracking should set their parent reference here.
    fn on_attached_to_parent(&mut self, _parent: NodeId) {}
    /// Called when this node is removed from its parent.
    /// Nodes with parent tracking should clear their parent reference here.
    fn on_removed_from_parent(&mut self) {}
    /// Get this node's parent ID (for nodes that track parents).
    /// Returns None if node has no parent or doesn't track parents.
    fn parent(&self) -> Option<NodeId> {
        None
    }
    /// Mark this node as needing layout (for nodes with dirty flags).
    /// Called during bubbling to propagate dirtiness up the tree.
    fn mark_needs_layout(&self) {}
    /// Check if this node needs layout (for nodes with dirty flags).
    fn needs_layout(&self) -> bool {
        false
    }
    /// Mark this node as needing measure (size may have changed).
    /// Called during bubbling when children are added/removed.
    fn mark_needs_measure(&self) {}
    /// Check if this node needs measure (for nodes with dirty flags).
    fn needs_measure(&self) -> bool {
        false
    }
    /// Mark this node as needing semantics recomputation.
    fn mark_needs_semantics(&self) {}
    /// Check if this node needs semantics recomputation.
    fn needs_semantics(&self) -> bool {
        false
    }
    /// Set parent reference for dirty flag bubbling ONLY.
    /// This is a minimal version of on_attached_to_parent that doesn't trigger
    /// registry updates or other side effects. Used during measurement when we
    /// need to establish parent connections for bubble_measure_dirty without
    /// causing the full attachment lifecycle.
    ///
    /// Default: delegates to on_attached_to_parent for backward compatibility.
    fn set_parent_for_bubbling(&mut self, parent: NodeId) {
        self.on_attached_to_parent(parent);
    }

    /// Returns a recycle pool key when this node supports shell reuse.
    fn recycle_key(&self) -> Option<TypeId> {
        None
    }

    /// Bounds how many recyclable shells of this node type should be retained.
    fn recycle_pool_limit(&self) -> Option<usize> {
        None
    }

    /// Clears live attachments before the node shell enters a recycle pool.
    fn prepare_for_recycle(&mut self) {}

    /// Optionally provides a compact replacement box for this recycled shell.
    ///
    /// Returning `Some` lets nodes move pooled survivors onto fresh compact
    /// storage so the recycle pool does not pin large spike-era allocations.
    fn rehouse_for_recycle(&self) -> Option<Box<dyn Node>> {
        None
    }

    /// Optionally moves a live node onto a fresh box during applier compaction.
    ///
    /// This is used after large-majority teardowns where a small surviving live
    /// tree can otherwise pin allocator pages from a much larger spike-era node
    /// population. Implementations must preserve the node's live state.
    fn rehouse_for_live_compaction(&mut self) -> Option<Box<dyn Node>> {
        None
    }

    /// Returns the node-owned heap retained beyond the node's own box allocation.
    fn debug_heap_bytes(&self) -> usize {
        0
    }
}

/// Unified API for bubbling layout dirty flags from a node to the root (Applier context).
///
/// This is the canonical function for dirty bubbling during the apply phase (structural changes).
/// Call this after mutations like insert/remove/move that happen during apply.
///
/// # Behavior
/// 1. Marks the starting node as needing layout
/// 2. Walks up the parent chain, marking each ancestor
/// 3. Stops when it reaches a node that's already dirty (O(1) optimization)
/// 4. Stops at the root (node with no parent)
///
/// # Performance
/// This function is O(height) in the worst case, but typically O(1) due to early exit
/// when encountering an already-dirty ancestor.
///
/// # Usage
/// - Call from composer mutations (insert/remove/move) during apply phase
/// - Call from applier-level operations that modify the tree structure
pub fn bubble_layout_dirty(applier: &mut dyn Applier, node_id: NodeId) {
    bubble_layout_dirty_applier(applier, node_id);
}

/// Unified API for bubbling measure dirty flags from a node to the root (Applier context).
///
/// Call this when a node's size may have changed (children added/removed, modifier changed).
/// This ensures that measure_layout will increment the cache epoch and re-measure the subtree.
///
/// # Behavior
/// 1. Marks the starting node as needing measure
/// 2. Walks up the parent chain, marking each ancestor
/// 3. Stops when it reaches a node that's already dirty (O(1) optimization)
/// 4. Stops at the root (node with no parent)
pub fn bubble_measure_dirty(applier: &mut dyn Applier, node_id: NodeId) {
    bubble_measure_dirty_applier(applier, node_id);
}

/// Unified API for bubbling semantics dirty flags from a node to the root (Applier context).
///
/// This mirrors [`bubble_layout_dirty`] but toggles semantics-specific dirty
/// flags instead of layout ones, allowing semantics updates to propagate during
/// the apply phase without forcing layout work.
pub fn bubble_semantics_dirty(applier: &mut dyn Applier, node_id: NodeId) {
    bubble_semantics_dirty_applier(applier, node_id);
}

/// Schedules semantics bubbling for a node using the active composer if present.
///
/// This defers the work to the apply phase where we can safely mutate the
/// applier tree without re-entrantly borrowing the composer during composition.
pub fn queue_semantics_invalidation(node_id: NodeId) {
    let _ = composer_context::try_with_composer(|composer| {
        composer.enqueue_semantics_invalidation(node_id);
    });
}

/// Unified API for bubbling layout dirty flags from a node to the root (Composer context).
///
/// This is the canonical function for dirty bubbling during composition (property changes).
/// Call this after property changes that happen during composition via with_node_mut.
///
/// # Behavior
/// 1. Marks the starting node as needing layout
/// 2. Walks up the parent chain, marking each ancestor
/// 3. Stops when it reaches a node that's already dirty (O(1) optimization)
/// 4. Stops at the root (node with no parent)
///
/// # Performance
/// This function is O(height) in the worst case, but typically O(1) due to early exit
/// when encountering an already-dirty ancestor.
///
/// # Type Requirements
/// The node type N must implement Node (which includes mark_needs_layout, parent, etc.).
/// Typically this will be LayoutNode or similar layout-aware node types.
///
/// # Usage
/// - Call from property setters during composition (e.g., set_modifier, set_measure_policy)
/// - Call from widget composition when layout-affecting state changes
pub fn bubble_layout_dirty_in_composer<N: Node + 'static>(node_id: NodeId) {
    bubble_layout_dirty_composer::<N>(node_id);
}

/// Unified API for bubbling measure dirty flags from a node to the root during composition.
///
/// This queues a dirty-bubble command on the active composer so measure invalidation
/// runs during the apply phase, avoiding re-entrant applier borrows while widgets are
/// mutating nodes via `with_node_mut`.
pub fn bubble_measure_dirty_in_composer(node_id: NodeId) {
    with_current_composer(|composer| {
        composer.commands_mut().push(Command::BubbleDirty {
            node_id,
            bubble: DirtyBubble {
                layout: false,
                measure: true,
                semantics: false,
            },
        });
    });
}

/// Unified API for bubbling semantics dirty flags from a node to the root (Composer context).
///
/// This mirrors [`bubble_layout_dirty_in_composer`] but routes through the semantics
/// dirty flag instead of the layout one. Modifier nodes can request semantics
/// invalidations without triggering measure/layout work, and the runtime can
/// query the root to determine whether the semantics tree needs rebuilding.
pub fn bubble_semantics_dirty_in_composer<N: Node + 'static>(node_id: NodeId) {
    bubble_semantics_dirty_composer::<N>(node_id);
}

/// Internal implementation for applier-based bubbling.
fn bubble_layout_dirty_applier(applier: &mut dyn Applier, mut node_id: NodeId) {
    // First, mark the starting node dirty (critical!)
    // This ensures root gets marked even if it has no parent
    if let Ok(node) = applier.get_mut(node_id) {
        node.mark_needs_layout();
    }

    // Then bubble up to ancestors
    loop {
        // Get parent of current node
        let parent_id = match applier.get_mut(node_id) {
            Ok(node) => node.parent(),
            Err(_) => None,
        };

        match parent_id {
            Some(pid) => {
                // Mark parent as needing layout
                if let Ok(parent) = applier.get_mut(pid) {
                    let parent_already_dirty = parent.needs_layout();
                    if !parent_already_dirty {
                        parent.mark_needs_layout();
                    }
                    node_id = pid;
                } else {
                    break;
                }
            }
            None => break, // No parent, stop
        }
    }
}

/// Internal implementation for applier-based bubbling of measure dirtiness.
fn bubble_measure_dirty_applier(applier: &mut dyn Applier, mut node_id: NodeId) {
    // First, mark the starting node as needing measure
    if let Ok(node) = applier.get_mut(node_id) {
        node.mark_needs_measure();
    }

    // Then bubble up to ancestors
    loop {
        // Get parent of current node
        let parent_id = match applier.get_mut(node_id) {
            Ok(node) => node.parent(),
            Err(_) => None,
        };

        match parent_id {
            Some(pid) => {
                // Mark parent as needing measure
                if let Ok(parent) = applier.get_mut(pid) {
                    if !parent.needs_measure() {
                        parent.mark_needs_measure();
                    }
                    node_id = pid;
                } else {
                    break;
                }
            }
            None => {
                break; // No parent, stop
            }
        }
    }
}

/// Internal implementation for applier-based bubbling of semantics dirtiness.
fn bubble_semantics_dirty_applier(applier: &mut dyn Applier, mut node_id: NodeId) {
    if let Ok(node) = applier.get_mut(node_id) {
        node.mark_needs_semantics();
    }

    loop {
        let parent_id = match applier.get_mut(node_id) {
            Ok(node) => node.parent(),
            Err(_) => None,
        };

        match parent_id {
            Some(pid) => {
                if let Ok(parent) = applier.get_mut(pid) {
                    if !parent.needs_semantics() {
                        parent.mark_needs_semantics();
                    }
                    node_id = pid;
                } else {
                    break;
                }
            }
            None => break,
        }
    }
}

/// Internal implementation for composer-based bubbling.
/// This uses with_node_mut and works during composition with a concrete node type.
/// The node type N must implement Node (which includes mark_needs_layout, parent, etc.).
fn bubble_layout_dirty_composer<N: Node + 'static>(mut node_id: NodeId) {
    // Mark the starting node dirty
    let _ = with_node_mut(node_id, |node: &mut N| {
        node.mark_needs_layout();
    });

    // Then bubble up to ancestors
    while let Ok(Some(pid)) = with_node_mut(node_id, |node: &mut N| node.parent()) {
        let parent_id = pid;

        // Mark parent as needing layout
        let advanced = with_node_mut(parent_id, |node: &mut N| {
            if !node.needs_layout() {
                node.mark_needs_layout();
            }
            true
        })
        .unwrap_or(false);

        if advanced {
            node_id = parent_id;
        } else {
            break;
        }
    }
}

/// Internal implementation for composer-based bubbling of semantics dirtiness.
fn bubble_semantics_dirty_composer<N: Node + 'static>(mut node_id: NodeId) {
    // Mark the starting node semantics-dirty.
    let _ = with_node_mut(node_id, |node: &mut N| {
        node.mark_needs_semantics();
    });

    while let Ok(Some(pid)) = with_node_mut(node_id, |node: &mut N| node.parent()) {
        let parent_id = pid;

        let advanced = with_node_mut(parent_id, |node: &mut N| {
            if !node.needs_semantics() {
                node.mark_needs_semantics();
            }
            true
        })
        .unwrap_or(false);

        if advanced {
            node_id = parent_id;
        } else {
            break;
        }
    }
}

impl dyn Node {
    pub fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct RecycledNode {
    stable_id: NodeId,
    node: Box<dyn Node>,
    warm_origin: bool,
}

impl RecycledNode {
    fn new(stable_id: NodeId, node: Box<dyn Node>, warm_origin: bool) -> Self {
        let node = node.rehouse_for_recycle().unwrap_or(node);
        Self {
            stable_id,
            node,
            warm_origin,
        }
    }

    fn from_shell(stable_id: NodeId, node: Box<dyn Node>, warm_origin: bool) -> Self {
        Self {
            stable_id,
            node,
            warm_origin,
        }
    }

    pub fn stable_id(&self) -> NodeId {
        self.stable_id
    }

    fn warm_origin(&self) -> bool {
        self.warm_origin
    }

    fn set_warm_origin(&mut self, warm_origin: bool) {
        self.warm_origin = warm_origin;
    }

    pub fn node_mut(&mut self) -> &mut dyn Node {
        self.node.as_mut()
    }

    pub fn into_parts(self) -> (NodeId, Box<dyn Node>, bool) {
        (self.stable_id, self.node, self.warm_origin)
    }
}

pub trait Applier: Any {
    fn create(&mut self, node: Box<dyn Node>) -> NodeId;
    fn get_mut(&mut self, id: NodeId) -> Result<&mut dyn Node, NodeError>;
    fn remove(&mut self, id: NodeId) -> Result<(), NodeError>;

    /// Returns the current generation for a node index.
    /// Generation is incremented when an index is reused from the freelist,
    /// preventing stale slot entries from matching recycled nodes.
    fn node_generation(&self, id: NodeId) -> u32;

    /// Inserts a node with a pre-assigned ID.
    ///
    /// This is used for virtual nodes whose IDs are allocated separately
    /// (e.g., via allocate_virtual_node_id()). Unlike `create()` which assigns
    /// a new ID, this method uses the provided ID.
    ///
    /// Returns Ok(()) if successful, or an error if the ID is already in use.
    fn insert_with_id(&mut self, id: NodeId, node: Box<dyn Node>) -> Result<(), NodeError>;

    fn as_any(&self) -> &dyn Any
    where
        Self: Sized,
    {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any
    where
        Self: Sized,
    {
        self
    }

    /// Trim trailing tombstones/unused capacity after structural changes.
    fn compact(&mut self) {}

    /// Returns a previously recycled node shell and its stable ID for the requested concrete type.
    fn take_recycled_node(&mut self, _key: TypeId) -> Option<RecycledNode> {
        None
    }

    /// Marks whether a reinserted recycled node originated from the warm recycle path.
    fn set_recycled_node_origin(&mut self, _id: NodeId, _warm_origin: bool) {}

    /// Seeds a warm recyclable shell for future reuse without requiring a prior removal.
    fn seed_recycled_node_shell(
        &mut self,
        _key: TypeId,
        _recycle_pool_limit: Option<usize>,
        _shell: Box<dyn Node>,
    ) {
    }

    /// Records that the current apply pass had to allocate a fresh recyclable shell for this type.
    fn record_fresh_recyclable_creation(&mut self, _key: TypeId) {}

    /// Drops any recyclable shells that should not survive beyond the current apply pass.
    fn clear_recycled_nodes(&mut self) {}
}

type TypedNodeUpdate = fn(&mut dyn Node, NodeId) -> Result<(), NodeError>;
type CommandCallback = Box<dyn FnOnce(&mut dyn Applier) -> Result<(), NodeError> + 'static>;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirtyBubble {
    layout: bool,
    measure: bool,
    semantics: bool,
}

impl DirtyBubble {
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

    fn apply(self, applier: &mut dyn Applier, node_id: NodeId) {
        if self.layout {
            bubble_layout_dirty(applier, node_id);
        }
        if self.measure {
            bubble_measure_dirty(applier, node_id);
        }
        if self.semantics {
            bubble_semantics_dirty(applier, node_id);
        }
    }
}

pub(crate) enum Command {
    BubbleDirty {
        node_id: NodeId,
        bubble: DirtyBubble,
    },
    UpdateTypedNode {
        id: NodeId,
        updater: TypedNodeUpdate,
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
    SyncChildren {
        parent_id: NodeId,
        expected_children: ChildList,
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

    fn flush(self, applier: &mut dyn Applier) -> Result<(), NodeError> {
        for cleanup in self.pending {
            cleanup_detached_child(applier, cleanup)?;
        }
        Ok(())
    }
}

impl Command {
    pub(crate) fn update_node<N: Node + 'static>(id: NodeId) -> Self {
        Self::UpdateTypedNode {
            id,
            updater: update_typed_node::<N>,
        }
    }

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
            Self::UpdateTypedNode { id, updater } => {
                let node = match applier.get_mut(id) {
                    Ok(node) => node,
                    Err(NodeError::Missing { .. }) => return Ok(()),
                    Err(err) => return Err(err),
                };
                updater(node, id)
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
                bubble,
            } => {
                insert_child_with_reparenting(applier, parent_id, child_id);
                bubble.apply(applier, parent_id);
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
                if insert_index != appended_index {
                    if let Ok(parent_node) = applier.get_mut(parent_id) {
                        parent_node.move_child(appended_index, insert_index);
                    }
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
            } => sync_children(applier, parent_id, &expected_children, deferred_cleanup),
            Self::Callback(callback) => callback(applier),
        }
    }
}

const COMMAND_CHUNK_CAPACITY: usize = 1024;
const COMMAND_FLUSH_THRESHOLD: usize = COMMAND_CHUNK_CAPACITY * 4;
type ChildList = SmallVec<[NodeId; 4]>;
const SMALL_CHILD_SYNC_LINEAR_THRESHOLD: usize = 8;

#[derive(Copy, Clone)]
enum CommandTag {
    BubbleDirty,
    UpdateTypedNode,
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

#[derive(Copy, Clone)]
struct BubbleDirtyCommand {
    node_id: NodeId,
    bubble: DirtyBubble,
}

#[derive(Copy, Clone)]
struct UpdateTypedNodeCommand {
    id: NodeId,
    updater: TypedNodeUpdate,
}

#[derive(Copy, Clone)]
struct AttachChildCommand {
    parent_id: NodeId,
    child_id: NodeId,
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
    child_start: usize,
    child_len: usize,
}

#[derive(Default)]
struct CommandQueue {
    chunks: Vec<Vec<CommandTag>>,
    len: usize,
    bubble_dirty: Vec<BubbleDirtyCommand>,
    update_typed_nodes: Vec<UpdateTypedNodeCommand>,
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
}

impl CommandQueue {
    fn push_tag(&mut self, tag: CommandTag) {
        let needs_chunk = self
            .chunks
            .last()
            .map(|chunk| chunk.len() == chunk.capacity())
            .unwrap_or(true);
        if needs_chunk {
            self.chunks.push(Vec::with_capacity(COMMAND_CHUNK_CAPACITY));
        }
        self.chunks
            .last_mut()
            .expect("command chunk should exist")
            .push(tag);
        self.len += 1;
    }

    fn push(&mut self, command: Command) {
        match command {
            Command::BubbleDirty { node_id, bubble } => {
                self.bubble_dirty
                    .push(BubbleDirtyCommand { node_id, bubble });
                self.push_tag(CommandTag::BubbleDirty);
            }
            Command::UpdateTypedNode { id, updater } => {
                self.update_typed_nodes
                    .push(UpdateTypedNodeCommand { id, updater });
                self.push_tag(CommandTag::UpdateTypedNode);
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
                bubble,
            } => {
                self.attach_children.push(AttachChildCommand {
                    parent_id,
                    child_id,
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
                let child_start = self.sync_child_ids.len();
                let child_len = expected_children.len();
                self.sync_child_ids.extend(expected_children);
                self.sync_children.push(SyncChildrenCommand {
                    parent_id,
                    child_start,
                    child_len,
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
        self.len
    }

    fn capacity(&self) -> usize {
        self.chunks.iter().map(Vec::capacity).sum()
    }

    fn payload_len_bytes(&self) -> usize {
        self.bubble_dirty
            .len()
            .saturating_mul(std::mem::size_of::<BubbleDirtyCommand>())
            .saturating_add(
                self.update_typed_nodes
                    .len()
                    .saturating_mul(std::mem::size_of::<UpdateTypedNodeCommand>()),
            )
            .saturating_add(
                self.remove_nodes
                    .len()
                    .saturating_mul(std::mem::size_of::<NodeId>()),
            )
            .saturating_add(
                self.mount_nodes
                    .len()
                    .saturating_mul(std::mem::size_of::<NodeId>()),
            )
            .saturating_add(
                self.attach_children
                    .len()
                    .saturating_mul(std::mem::size_of::<AttachChildCommand>()),
            )
            .saturating_add(
                self.insert_children
                    .len()
                    .saturating_mul(std::mem::size_of::<InsertChildCommand>()),
            )
            .saturating_add(
                self.move_children
                    .len()
                    .saturating_mul(std::mem::size_of::<MoveChildCommand>()),
            )
            .saturating_add(
                self.remove_children
                    .len()
                    .saturating_mul(std::mem::size_of::<RemoveChildCommand>()),
            )
            .saturating_add(
                self.detach_children
                    .len()
                    .saturating_mul(std::mem::size_of::<DetachChildCommand>()),
            )
            .saturating_add(
                self.sync_children
                    .len()
                    .saturating_mul(std::mem::size_of::<SyncChildrenCommand>()),
            )
            .saturating_add(
                self.sync_child_ids
                    .len()
                    .saturating_mul(std::mem::size_of::<NodeId>()),
            )
            .saturating_add(
                self.callbacks
                    .len()
                    .saturating_mul(std::mem::size_of::<CommandCallback>()),
            )
    }

    fn payload_capacity_bytes(&self) -> usize {
        self.bubble_dirty
            .capacity()
            .saturating_mul(std::mem::size_of::<BubbleDirtyCommand>())
            .saturating_add(
                self.update_typed_nodes
                    .capacity()
                    .saturating_mul(std::mem::size_of::<UpdateTypedNodeCommand>()),
            )
            .saturating_add(
                self.remove_nodes
                    .capacity()
                    .saturating_mul(std::mem::size_of::<NodeId>()),
            )
            .saturating_add(
                self.mount_nodes
                    .capacity()
                    .saturating_mul(std::mem::size_of::<NodeId>()),
            )
            .saturating_add(
                self.attach_children
                    .capacity()
                    .saturating_mul(std::mem::size_of::<AttachChildCommand>()),
            )
            .saturating_add(
                self.insert_children
                    .capacity()
                    .saturating_mul(std::mem::size_of::<InsertChildCommand>()),
            )
            .saturating_add(
                self.move_children
                    .capacity()
                    .saturating_mul(std::mem::size_of::<MoveChildCommand>()),
            )
            .saturating_add(
                self.remove_children
                    .capacity()
                    .saturating_mul(std::mem::size_of::<RemoveChildCommand>()),
            )
            .saturating_add(
                self.detach_children
                    .capacity()
                    .saturating_mul(std::mem::size_of::<DetachChildCommand>()),
            )
            .saturating_add(
                self.sync_children
                    .capacity()
                    .saturating_mul(std::mem::size_of::<SyncChildrenCommand>()),
            )
            .saturating_add(
                self.sync_child_ids
                    .capacity()
                    .saturating_mul(std::mem::size_of::<NodeId>()),
            )
            .saturating_add(
                self.callbacks
                    .capacity()
                    .saturating_mul(std::mem::size_of::<CommandCallback>()),
            )
    }

    fn apply(self, applier: &mut dyn Applier) -> Result<(), NodeError> {
        let mut bubble_dirty = self.bubble_dirty.into_iter();
        let mut update_typed_nodes = self.update_typed_nodes.into_iter();
        let mut remove_nodes = self.remove_nodes.into_iter();
        let mut mount_nodes = self.mount_nodes.into_iter();
        let mut attach_children = self.attach_children.into_iter();
        let mut insert_children = self.insert_children.into_iter();
        let mut move_children = self.move_children.into_iter();
        let mut remove_children = self.remove_children.into_iter();
        let mut detach_children = self.detach_children.into_iter();
        let mut sync_children_commands = self.sync_children.into_iter();
        let sync_child_ids = self.sync_child_ids;
        let mut callbacks = self.callbacks.into_iter();
        let mut deferred_cleanup = DeferredChildCleanupQueue::default();

        for chunk in self.chunks {
            for tag in chunk {
                match tag {
                    CommandTag::BubbleDirty => {
                        let BubbleDirtyCommand { node_id, bubble } =
                            bubble_dirty.next().expect("missing BubbleDirty payload");
                        Command::BubbleDirty { node_id, bubble }
                            .apply_with_cleanup(applier, &mut deferred_cleanup)?;
                    }
                    CommandTag::UpdateTypedNode => {
                        let UpdateTypedNodeCommand { id, updater } = update_typed_nodes
                            .next()
                            .expect("missing UpdateTypedNode payload");
                        Command::UpdateTypedNode { id, updater }
                            .apply_with_cleanup(applier, &mut deferred_cleanup)?;
                    }
                    CommandTag::RemoveNode => {
                        let id = remove_nodes.next().expect("missing RemoveNode payload");
                        Command::RemoveNode { id }
                            .apply_with_cleanup(applier, &mut deferred_cleanup)?;
                    }
                    CommandTag::MountNode => {
                        let id = mount_nodes.next().expect("missing MountNode payload");
                        Command::MountNode { id }
                            .apply_with_cleanup(applier, &mut deferred_cleanup)?;
                    }
                    CommandTag::AttachChild => {
                        let AttachChildCommand {
                            parent_id,
                            child_id,
                            bubble,
                        } = attach_children.next().expect("missing AttachChild payload");
                        Command::AttachChild {
                            parent_id,
                            child_id,
                            bubble,
                        }
                        .apply_with_cleanup(applier, &mut deferred_cleanup)?;
                    }
                    CommandTag::InsertChild => {
                        let InsertChildCommand {
                            parent_id,
                            child_id,
                            appended_index,
                            insert_index,
                            bubble,
                        } = insert_children.next().expect("missing InsertChild payload");
                        Command::InsertChild {
                            parent_id,
                            child_id,
                            appended_index,
                            insert_index,
                            bubble,
                        }
                        .apply_with_cleanup(applier, &mut deferred_cleanup)?;
                    }
                    CommandTag::MoveChild => {
                        let MoveChildCommand {
                            parent_id,
                            from_index,
                            to_index,
                            bubble,
                        } = move_children.next().expect("missing MoveChild payload");
                        Command::MoveChild {
                            parent_id,
                            from_index,
                            to_index,
                            bubble,
                        }
                        .apply_with_cleanup(applier, &mut deferred_cleanup)?;
                    }
                    CommandTag::RemoveChild => {
                        let RemoveChildCommand {
                            parent_id,
                            child_id,
                        } = remove_children.next().expect("missing RemoveChild payload");
                        Command::RemoveChild {
                            parent_id,
                            child_id,
                        }
                        .apply_with_cleanup(applier, &mut deferred_cleanup)?;
                    }
                    CommandTag::DetachChild => {
                        let DetachChildCommand {
                            parent_id,
                            child_id,
                        } = detach_children.next().expect("missing DetachChild payload");
                        Command::DetachChild {
                            parent_id,
                            child_id,
                        }
                        .apply_with_cleanup(applier, &mut deferred_cleanup)?;
                    }
                    CommandTag::SyncChildren => {
                        let SyncChildrenCommand {
                            parent_id,
                            child_start,
                            child_len,
                        } = sync_children_commands
                            .next()
                            .expect("missing SyncChildren payload");
                        let expected_children =
                            &sync_child_ids[child_start..child_start + child_len];
                        sync_children(
                            applier,
                            parent_id,
                            expected_children,
                            &mut deferred_cleanup,
                        )?;
                    }
                    CommandTag::Callback => {
                        let callback = callbacks.next().expect("missing Callback payload");
                        Command::Callback(callback)
                            .apply_with_cleanup(applier, &mut deferred_cleanup)?;
                    }
                }
            }
        }

        debug_assert!(bubble_dirty.next().is_none());
        debug_assert!(update_typed_nodes.next().is_none());
        debug_assert!(remove_nodes.next().is_none());
        debug_assert!(mount_nodes.next().is_none());
        debug_assert!(attach_children.next().is_none());
        debug_assert!(insert_children.next().is_none());
        debug_assert!(move_children.next().is_none());
        debug_assert!(remove_children.next().is_none());
        debug_assert!(detach_children.next().is_none());
        debug_assert!(sync_children_commands.next().is_none());
        debug_assert!(callbacks.next().is_none());
        deferred_cleanup.flush(applier)
    }
}

fn update_typed_node<N: Node + 'static>(node: &mut dyn Node, id: NodeId) -> Result<(), NodeError> {
    let typed = node
        .as_any_mut()
        .downcast_mut::<N>()
        .ok_or(NodeError::TypeMismatch {
            id,
            expected: std::any::type_name::<N>(),
        })?;
    typed.update();
    Ok(())
}

fn insert_child_with_reparenting(applier: &mut dyn Applier, parent_id: NodeId, child_id: NodeId) {
    let old_parent = applier
        .get_mut(child_id)
        .ok()
        .and_then(|node| node.parent());
    if let Some(old_parent_id) = old_parent {
        if old_parent_id != parent_id {
            if let Ok(old_parent_node) = applier.get_mut(old_parent_id) {
                old_parent_node.remove_child(child_id);
            }
            if let Ok(child_node) = applier.get_mut(child_id) {
                child_node.on_removed_from_parent();
            }
            bubble_layout_dirty(applier, old_parent_id);
            bubble_measure_dirty(applier, old_parent_id);
        }
    }

    if let Ok(parent_node) = applier.get_mut(parent_id) {
        parent_node.insert_child(child_id);
    }
    if let Ok(child_node) = applier.get_mut(child_id) {
        child_node.on_attached_to_parent(parent_id);
    }
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
    if let Ok(parent_node) = applier.get_mut(parent_id) {
        parent_node.remove_child(child_id);
    }
    bubble_layout_dirty(applier, parent_id);
    bubble_measure_dirty(applier, parent_id);

    if let Ok(node) = applier.get_mut(child_id) {
        match node.parent() {
            Some(existing_parent_id) if existing_parent_id == parent_id => {
                node.on_removed_from_parent();
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

fn reconcile_children(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    expected_children: &[NodeId],
    needs_dirty_check: bool,
) -> Result<(), NodeError> {
    let mut repaired = false;
    for &child_id in expected_children {
        let needs_attach = if let Ok(node) = applier.get_mut(child_id) {
            node.parent() != Some(parent_id)
        } else {
            false
        };

        if needs_attach {
            insert_child_with_reparenting(applier, parent_id, child_id);
            repaired = true;
        }
    }

    let is_dirty = if needs_dirty_check {
        if let Ok(node) = applier.get_mut(parent_id) {
            node.needs_layout()
        } else {
            false
        }
    } else {
        false
    };

    if repaired {
        bubble_layout_dirty(applier, parent_id);
        bubble_measure_dirty(applier, parent_id);
    } else if is_dirty {
        bubble_layout_dirty(applier, parent_id);
    }

    Ok(())
}

#[derive(Default)]
pub struct MemoryApplier {
    nodes: Vec<Option<Box<dyn Node>>>,
    /// Stable node ID occupying each physical slot.
    physical_stable_ids: Vec<u32>,
    physical_warm_recycled_origins: Vec<bool>,
    /// Physical storage location for each live stable node ID.
    stable_to_physical: HashMap<NodeId, usize>,
    /// Generation counter per stable node ID.
    stable_generations: HashMap<NodeId, u32>,
    /// Lowest free physical slots are reused first so the dense storage stays compact.
    free_ids: BinaryHeap<Reverse<usize>>,
    /// Storage for high-ID nodes (like virtual nodes with IDs starting at 0xFFFFFFFF00000000)
    /// that can't be stored in the Vec without causing capacity overflow.
    high_id_nodes: HashMap<NodeId, Box<dyn Node>>,
    high_id_warm_recycled_origins: HashMap<NodeId, bool>,
    high_id_generations: HashMap<NodeId, u32>,
    next_stable_id: NodeId,
    layout_runtime: Option<RuntimeHandle>,
    slots: SlotTable,
    recycled_nodes: HashMap<TypeId, Vec<RecycledNode>>,
    returning_recycled_nodes: HashMap<TypeId, Vec<RecycledNode>>,
    cold_recycled_nodes: HashMap<TypeId, Vec<RecycledNode>>,
    recycled_node_limits: HashMap<TypeId, usize>,
    warm_recycled_node_targets: HashMap<TypeId, usize>,
    fresh_recyclable_creations: HashMap<TypeId, usize>,
    recycled_node_prototypes: HashMap<TypeId, Box<dyn Node>>,
}

struct RemovalFrame {
    node_id: NodeId,
    children: SmallVec<[NodeId; 8]>,
    next_child: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemoryApplierDebugStats {
    pub next_stable_id: NodeId,
    pub nodes_len: usize,
    pub nodes_cap: usize,
    pub physical_stable_ids_len: usize,
    pub physical_stable_ids_cap: usize,
    pub stable_to_physical_len: usize,
    pub stable_to_physical_cap: usize,
    pub stable_generations_len: usize,
    pub stable_generations_cap: usize,
    pub free_ids_len: usize,
    pub free_ids_cap: usize,
    pub high_id_nodes_len: usize,
    pub high_id_nodes_cap: usize,
    pub high_id_generations_len: usize,
    pub high_id_generations_cap: usize,
    pub recycled_type_count: usize,
    pub recycled_type_cap: usize,
    pub recycled_node_count: usize,
    pub recycled_node_capacity: usize,
    pub warm_recycled_node_id_count: usize,
    pub warm_recycled_node_id_capacity: usize,
}

impl MemoryApplier {
    const EAGER_COMPACT_NODE_LEN: usize = 1_024;
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
            .nodes
            .len()
            .max(self.physical_stable_ids.len())
            .max(self.physical_warm_recycled_origins.len());
        if len < self.nodes.capacity()
            && len < self.physical_stable_ids.capacity()
            && len < self.physical_warm_recycled_origins.capacity()
        {
            return;
        }

        let target = Self::next_dense_node_target_len(len);
        if self.nodes.capacity() < target {
            self.nodes
                .reserve_exact(target.saturating_sub(self.nodes.len()));
        }
        if self.physical_stable_ids.capacity() < target {
            self.physical_stable_ids
                .reserve_exact(target.saturating_sub(self.physical_stable_ids.len()));
        }
        if self.physical_warm_recycled_origins.capacity() < target {
            self.physical_warm_recycled_origins
                .reserve_exact(target.saturating_sub(self.physical_warm_recycled_origins.len()));
        }
    }

    fn ensure_stable_index_capacity(&mut self) {
        let len = self
            .stable_to_physical
            .len()
            .max(self.stable_generations.len());
        if len < self.stable_to_physical.capacity() && len < self.stable_generations.capacity() {
            return;
        }

        let target = Self::next_dense_node_target_len(len);
        let additional = target.saturating_sub(len);
        if self.stable_to_physical.capacity() < target {
            self.stable_to_physical.reserve(additional);
        }
        if self.stable_generations.capacity() < target {
            self.stable_generations.reserve(additional);
        }
    }

    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            physical_stable_ids: Vec::new(),
            physical_warm_recycled_origins: Vec::new(),
            stable_to_physical: HashMap::default(),
            stable_generations: HashMap::default(),
            free_ids: BinaryHeap::new(),
            high_id_nodes: HashMap::default(),
            high_id_warm_recycled_origins: HashMap::default(),
            high_id_generations: HashMap::default(),
            next_stable_id: 0,
            layout_runtime: None,
            slots: SlotTable::default(),
            recycled_nodes: HashMap::default(),
            returning_recycled_nodes: HashMap::default(),
            cold_recycled_nodes: HashMap::default(),
            recycled_node_limits: HashMap::default(),
            warm_recycled_node_targets: HashMap::default(),
            fresh_recyclable_creations: HashMap::default(),
            recycled_node_prototypes: HashMap::default(),
        }
    }

    pub fn slots(&mut self) -> &mut SlotTable {
        &mut self.slots
    }

    pub fn with_node<N: Node + 'static, R>(
        &mut self,
        id: NodeId,
        f: impl FnOnce(&mut N) -> R,
    ) -> Result<R, NodeError> {
        let physical_id = self
            .resolve_node_index(id)
            .ok_or(NodeError::Missing { id })?;
        let slot = self
            .nodes
            .get_mut(physical_id)
            .ok_or(NodeError::Missing { id })?
            .as_deref_mut()
            .ok_or(NodeError::Missing { id })?;
        let typed = slot
            .as_any_mut()
            .downcast_mut::<N>()
            .ok_or(NodeError::TypeMismatch {
                id,
                expected: std::any::type_name::<N>(),
            })?;
        Ok(f(typed))
    }

    pub fn len(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_some()).count()
    }

    pub fn capacity(&self) -> usize {
        self.nodes.len()
    }

    pub fn tombstone_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_none()).count()
    }

    pub fn freelist_len(&self) -> usize {
        self.free_ids.len()
    }

    pub fn debug_recycled_node_count(&self) -> usize {
        self.total_recycled_node_count()
    }

    pub fn debug_recycled_node_count_for<N: Node + 'static>(&self) -> usize {
        let key = TypeId::of::<N>();
        self.recycled_nodes.get(&key).map(Vec::len).unwrap_or(0)
            + self
                .returning_recycled_nodes
                .get(&key)
                .map(Vec::len)
                .unwrap_or(0)
            + self
                .cold_recycled_nodes
                .get(&key)
                .map(Vec::len)
                .unwrap_or(0)
    }

    pub fn debug_stats(&self) -> MemoryApplierDebugStats {
        let mut recycled_keys: HashSet<TypeId> = HashSet::default();
        recycled_keys.extend(self.recycled_nodes.keys().copied());
        recycled_keys.extend(self.returning_recycled_nodes.keys().copied());
        recycled_keys.extend(self.cold_recycled_nodes.keys().copied());

        MemoryApplierDebugStats {
            next_stable_id: self.next_stable_id,
            nodes_len: self.len(),
            nodes_cap: self.nodes.len(),
            physical_stable_ids_len: self.physical_stable_ids.len(),
            physical_stable_ids_cap: self.physical_stable_ids.capacity(),
            stable_to_physical_len: self.stable_to_physical.len(),
            stable_to_physical_cap: self.stable_to_physical.capacity(),
            stable_generations_len: self.stable_generations.len(),
            stable_generations_cap: self.stable_generations.capacity(),
            free_ids_len: self.free_ids.len(),
            free_ids_cap: self.free_ids.capacity(),
            high_id_nodes_len: self.high_id_nodes.len(),
            high_id_nodes_cap: self.high_id_nodes.capacity(),
            high_id_generations_len: self.high_id_generations.len(),
            high_id_generations_cap: self.high_id_generations.capacity(),
            recycled_type_count: recycled_keys.len(),
            recycled_type_cap: self.recycled_nodes.capacity()
                + self.returning_recycled_nodes.capacity()
                + self.cold_recycled_nodes.capacity(),
            recycled_node_count: self.total_recycled_node_count(),
            recycled_node_capacity: self.total_recycled_node_capacity(),
            warm_recycled_node_id_count: self.total_warm_recycled_node_id_count(),
            warm_recycled_node_id_capacity: self.total_warm_recycled_node_id_capacity(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn debug_live_node_heap_bytes(&self) -> usize {
        let dense_nodes = self
            .nodes
            .iter()
            .flatten()
            .map(|node| std::mem::size_of_val(&**node) + node.debug_heap_bytes())
            .sum::<usize>();
        let high_id_nodes = self
            .high_id_nodes
            .values()
            .map(|node| std::mem::size_of_val(&**node) + node.debug_heap_bytes())
            .sum::<usize>();
        dense_nodes + high_id_nodes
    }

    pub fn debug_recycled_node_heap_bytes(&self) -> usize {
        let pool_bytes = |pools: &HashMap<TypeId, Vec<RecycledNode>>| {
            pools
                .values()
                .flat_map(|nodes| nodes.iter())
                .map(|node| std::mem::size_of_val(&*node.node) + node.node.debug_heap_bytes())
                .sum::<usize>()
        };

        pool_bytes(&self.recycled_nodes)
            + pool_bytes(&self.returning_recycled_nodes)
            + pool_bytes(&self.cold_recycled_nodes)
    }

    pub fn set_runtime_handle(&mut self, handle: RuntimeHandle) {
        self.layout_runtime = Some(handle);
    }

    pub fn clear_runtime_handle(&mut self) {
        self.layout_runtime = None;
    }

    pub fn runtime_handle(&self) -> Option<RuntimeHandle> {
        self.layout_runtime.clone()
    }

    fn pool_node_count(pools: &HashMap<TypeId, Vec<RecycledNode>>) -> usize {
        pools.values().map(Vec::len).sum()
    }

    fn pool_node_capacity(pools: &HashMap<TypeId, Vec<RecycledNode>>) -> usize {
        pools.values().map(Vec::capacity).sum()
    }

    fn total_recycled_node_count(&self) -> usize {
        Self::pool_node_count(&self.recycled_nodes)
            + Self::pool_node_count(&self.returning_recycled_nodes)
            + Self::pool_node_count(&self.cold_recycled_nodes)
    }

    fn total_recycled_node_capacity(&self) -> usize {
        Self::pool_node_capacity(&self.recycled_nodes)
            + Self::pool_node_capacity(&self.returning_recycled_nodes)
            + Self::pool_node_capacity(&self.cold_recycled_nodes)
    }

    fn total_warm_recycled_node_id_count(&self) -> usize {
        self.live_warm_recycled_origin_count()
            + Self::pool_node_count(&self.recycled_nodes)
            + Self::pool_node_count(&self.returning_recycled_nodes)
    }

    fn total_warm_recycled_node_id_capacity(&self) -> usize {
        self.live_warm_recycled_origin_capacity()
            + Self::pool_node_capacity(&self.recycled_nodes)
            + Self::pool_node_capacity(&self.returning_recycled_nodes)
    }

    fn remember_recycle_pool_limit(&mut self, key: TypeId, recycle_pool_limit: Option<usize>) {
        if let Some(limit) = recycle_pool_limit {
            self.recycled_node_limits.insert(key, limit);
        } else {
            self.recycled_node_limits.remove(&key);
        }
    }

    fn recycle_pool_limit_for(&self, key: TypeId) -> Option<usize> {
        self.recycled_node_limits.get(&key).copied()
    }

    fn warm_recycled_pool_len(&self, key: TypeId) -> usize {
        self.recycled_nodes.get(&key).map(Vec::len).unwrap_or(0)
    }

    fn warm_recycled_node_target(&self, key: TypeId) -> usize {
        self.warm_recycled_node_targets
            .get(&key)
            .copied()
            .unwrap_or(0)
    }

    fn warm_recycled_node_target_limit(&self, key: TypeId) -> usize {
        let Some(limit) = self.recycle_pool_limit_for(key) else {
            return usize::MAX;
        };
        if limit <= 8 {
            limit
        } else {
            limit / 4
        }
    }

    fn update_warm_recycled_node_target(&mut self, key: TypeId, observed_demand: usize) -> usize {
        let target_limit = self.warm_recycled_node_target_limit(key);
        let existing = self.warm_recycled_node_target(key).min(target_limit);
        if observed_demand == 0 {
            return existing;
        }

        let target = match self.recycle_pool_limit_for(key) {
            Some(limit) if limit > 8 => target_limit,
            Some(_) => observed_demand.min(target_limit),
            None => observed_demand,
        };
        self.warm_recycled_node_targets.insert(key, target);
        target
    }

    fn remember_recycled_node_prototype(&mut self, key: TypeId, shell: &dyn Node) {
        if self.recycled_node_prototypes.contains_key(&key) {
            return;
        }
        if let Some(prototype) = shell.rehouse_for_recycle() {
            self.recycled_node_prototypes.insert(key, prototype);
        }
    }

    fn live_warm_recycled_origin_count(&self) -> usize {
        self.physical_warm_recycled_origins
            .iter()
            .zip(self.nodes.iter())
            .filter(|(warm_origin, node)| **warm_origin && node.is_some())
            .count()
            + self
                .high_id_warm_recycled_origins
                .values()
                .filter(|warm_origin| **warm_origin)
                .count()
    }

    fn live_warm_recycled_origin_capacity(&self) -> usize {
        self.physical_warm_recycled_origins.capacity()
            + self.high_id_warm_recycled_origins.capacity()
    }

    fn push_recycled_node(
        &mut self,
        key: TypeId,
        recycle_pool_limit: Option<usize>,
        recycled: RecycledNode,
    ) {
        self.remember_recycle_pool_limit(key, recycle_pool_limit);
        self.remember_recycled_node_prototype(key, recycled.node.as_ref());

        let warm_origin = recycled.warm_origin();
        let pool = if warm_origin {
            self.returning_recycled_nodes.entry(key).or_default()
        } else {
            self.cold_recycled_nodes.entry(key).or_default()
        };
        pool.push(recycled);
        if let Some(limit) = recycle_pool_limit {
            if pool.len() > limit {
                let excess = pool.len() - limit;
                let dropped: Vec<_> = pool.drain(0..excess).collect();
                drop(dropped);
            }
        }
    }

    fn push_warm_recycled_node(
        &mut self,
        key: TypeId,
        recycle_pool_limit: Option<usize>,
        mut recycled: RecycledNode,
    ) {
        self.remember_recycle_pool_limit(key, recycle_pool_limit);

        recycled.set_warm_origin(true);
        let mut dropped = Vec::new();
        let mut remove_pool_entry = false;
        {
            let pool = self.recycled_nodes.entry(key).or_default();
            pool.push(recycled);
            if let Some(limit) = recycle_pool_limit {
                if pool.len() > limit {
                    let excess = pool.len() - limit;
                    dropped = pool.drain(0..excess).collect();
                    remove_pool_entry = pool.is_empty();
                }
            }
        }
        if remove_pool_entry {
            self.recycled_nodes.remove(&key);
        }
        drop(dropped);
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

        self.remember_recycled_node_prototype(key, shell.as_ref());
        let stable_id = self.next_stable_id;
        self.next_stable_id = self.next_stable_id.saturating_add(1);
        self.push_warm_recycled_node(
            key,
            recycle_pool_limit,
            RecycledNode::from_shell(stable_id, shell, true),
        );
    }

    fn take_recycled_node_from_pool(
        pools: &mut HashMap<TypeId, Vec<RecycledNode>>,
        key: TypeId,
    ) -> Option<RecycledNode> {
        let pool = pools.get_mut(&key)?;
        let node = pool.pop();
        if pool.is_empty() {
            pools.remove(&key);
        }
        node
    }

    fn compact_idle_warm_pool(&mut self, key: TypeId) {
        let Some(pool) = self.recycled_nodes.get_mut(&key) else {
            return;
        };
        if pool.capacity() <= pool.len().saturating_mul(4).max(64) {
            return;
        }

        let retained = pool.len();
        let mut compacted = Vec::with_capacity(retained);
        compacted.append(pool);
        let remove_pool_entry = compacted.is_empty();
        *pool = compacted;
        let _ = pool;

        if remove_pool_entry {
            self.recycled_nodes.remove(&key);
        }
    }

    fn trim_idle_warm_pool_to_target(&mut self, key: TypeId, target: usize) {
        let pool_len = self.warm_recycled_pool_len(key);
        if pool_len <= target {
            return;
        }

        let Some(pool) = self.recycled_nodes.get_mut(&key) else {
            return;
        };
        let removable = (pool_len - target).min(pool.len());
        let dropped: Vec<_> = pool.drain(0..removable).collect();
        let remove_pool_entry = pool.is_empty();
        let _ = pool;

        if remove_pool_entry {
            self.recycled_nodes.remove(&key);
        }
        drop(dropped);
    }

    fn replenish_warm_pool_to_target(&mut self, key: TypeId, target: usize) {
        let missing = target.saturating_sub(self.warm_recycled_pool_len(key));
        if missing == 0 {
            return;
        }

        let recycle_pool_limit = self.recycle_pool_limit_for(key);
        let mut shells = Vec::with_capacity(missing);
        if let Some(prototype) = self.recycled_node_prototypes.get(&key) {
            for _ in 0..missing {
                let Some(shell) = prototype.rehouse_for_recycle() else {
                    break;
                };
                shells.push(shell);
            }
        }

        for shell in shells {
            self.seed_recycled_node_shell_impl(key, recycle_pool_limit, shell);
        }
    }

    fn prune_stable_generations(&mut self) {
        let retained_len = self.stable_to_physical.len() + self.total_recycled_node_count();
        if retained_len == self.stable_generations.len() {
            return;
        }

        let mut retained = HashMap::default();
        retained.reserve(retained_len);
        for stable_id in self.stable_to_physical.keys().copied() {
            if let Some(generation) = self.stable_generations.get(&stable_id).copied() {
                retained.insert(stable_id, generation);
            }
        }
        for stable_id in self
            .recycled_nodes
            .values()
            .flat_map(|nodes| nodes.iter().map(RecycledNode::stable_id))
        {
            if let Some(generation) = self.stable_generations.get(&stable_id).copied() {
                retained.insert(stable_id, generation);
            }
        }
        for stable_id in self
            .returning_recycled_nodes
            .values()
            .flat_map(|nodes| nodes.iter().map(RecycledNode::stable_id))
        {
            if let Some(generation) = self.stable_generations.get(&stable_id).copied() {
                retained.insert(stable_id, generation);
            }
        }
        for stable_id in self
            .cold_recycled_nodes
            .values()
            .flat_map(|nodes| nodes.iter().map(RecycledNode::stable_id))
        {
            if let Some(generation) = self.stable_generations.get(&stable_id).copied() {
                retained.insert(stable_id, generation);
            }
        }
        self.stable_generations = retained;
    }

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
        if let Some(physical_id) = self.resolve_node_index(id) {
            let node = self.nodes[physical_id]
                .as_ref()
                .expect("resolved physical node must exist");
            let type_name = std::any::type_name_of_val(&**node);
            output.push_str(&format!("{}[{}] {}\n", indent, id, type_name));

            let children = node.children();
            for child_id in children {
                self.dump_node(output, child_id, depth + 1);
            }
        } else {
            output.push_str(&format!("{}[{}] (missing)\n", indent, id));
        }
    }

    fn resolve_node_index(&self, id: NodeId) -> Option<usize> {
        self.stable_to_physical.get(&id).copied()
    }

    fn get_ref(&self, id: NodeId) -> Result<&dyn Node, NodeError> {
        if let Some(physical_id) = self.resolve_node_index(id) {
            let slot = self
                .nodes
                .get(physical_id)
                .ok_or(NodeError::Missing { id })?
                .as_deref()
                .ok_or(NodeError::Missing { id })?;
            return Ok(slot);
        }

        self.high_id_nodes
            .get(&id)
            .map(|node| node.as_ref())
            .ok_or(NodeError::Missing { id })
    }

    fn collect_node_children_into(
        &self,
        id: NodeId,
        out: &mut SmallVec<[NodeId; 8]>,
    ) -> Result<(), NodeError> {
        self.get_ref(id)?.collect_children_into(out);
        Ok(())
    }

    fn node_parent(&self, id: NodeId) -> Result<Option<NodeId>, NodeError> {
        Ok(self.get_ref(id)?.parent())
    }

    fn collect_owned_children(
        &self,
        node_id: NodeId,
        out: &mut SmallVec<[NodeId; 8]>,
    ) -> Result<(), NodeError> {
        self.collect_node_children_into(node_id, out)?;
        out.retain(|child_id| {
            self.node_parent(*child_id)
                .map(|parent| parent == Some(node_id))
                .unwrap_or(false)
        });
        Ok(())
    }

    fn remove_node_storage(&mut self, node_id: NodeId) -> Result<(), NodeError> {
        if self.high_id_nodes.contains_key(&node_id) {
            if let Some(mut node) = self.high_id_nodes.remove(&node_id) {
                if let Some(key) = node.recycle_key() {
                    let recycle_pool_limit = node.recycle_pool_limit();
                    let warm_origin = self
                        .high_id_warm_recycled_origins
                        .remove(&node_id)
                        .unwrap_or(false);
                    node.prepare_for_recycle();
                    self.push_recycled_node(
                        key,
                        recycle_pool_limit,
                        RecycledNode::new(node_id, node, warm_origin),
                    );
                }
            }
            let generation = self.high_id_generations.entry(node_id).or_insert(0);
            *generation = generation.wrapping_add(1);
            return Ok(());
        }

        let physical_id = self
            .resolve_node_index(node_id)
            .ok_or(NodeError::Missing { id: node_id })?;
        if let Some(mut node) = self.nodes[physical_id].take() {
            if let Some(key) = node.recycle_key() {
                let recycle_pool_limit = node.recycle_pool_limit();
                let warm_origin = self
                    .physical_warm_recycled_origins
                    .get_mut(physical_id)
                    .map(std::mem::take)
                    .unwrap_or(false);
                node.prepare_for_recycle();
                self.push_recycled_node(
                    key,
                    recycle_pool_limit,
                    RecycledNode::new(node_id, node, warm_origin),
                );
            }
        }
        self.physical_stable_ids[physical_id] = Self::INVALID_STABLE_ID;
        self.stable_to_physical.remove(&node_id);
        if let Some(generation) = self.stable_generations.get_mut(&node_id) {
            *generation = generation.wrapping_add(1);
        } else {
            self.stable_generations.insert(node_id, 1);
        }
        self.free_ids.push(Reverse(physical_id));
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
    fn create(&mut self, node: Box<dyn Node>) -> NodeId {
        self.ensure_stable_index_capacity();
        let stable_id = self.next_stable_id;
        self.next_stable_id = self.next_stable_id.saturating_add(1);
        self.stable_generations.insert(stable_id, 0);

        let physical_id = if let Some(Reverse(id)) = self.free_ids.pop() {
            debug_assert!(self.nodes[id].is_none(), "freelist entry {id} is not None");
            self.nodes[id] = Some(node);
            self.physical_stable_ids[id] = Self::pack_stable_id(stable_id);
            self.physical_warm_recycled_origins[id] = false;
            id
        } else {
            self.ensure_dense_node_storage_capacity();
            let id = self.nodes.len();
            self.nodes.push(Some(node));
            self.physical_stable_ids
                .push(Self::pack_stable_id(stable_id));
            self.physical_warm_recycled_origins.push(false);
            id
        };
        self.stable_to_physical.insert(stable_id, physical_id);
        stable_id
    }

    fn node_generation(&self, id: NodeId) -> u32 {
        self.high_id_generations
            .get(&id)
            .copied()
            .or_else(|| self.stable_generations.get(&id).copied())
            .unwrap_or(0)
    }

    fn get_mut(&mut self, id: NodeId) -> Result<&mut dyn Node, NodeError> {
        // Try Vec first (common case for normal nodes), then HashMap for virtual nodes.
        if let Some(physical_id) = self.resolve_node_index(id) {
            let slot = self.nodes[physical_id]
                .as_deref_mut()
                .ok_or(NodeError::Missing { id })?;
            return Ok(slot);
        }
        self.high_id_nodes
            .get_mut(&id)
            .map(|n| n.as_mut())
            .ok_or(NodeError::Missing { id })
    }

    fn remove(&mut self, id: NodeId) -> Result<(), NodeError> {
        self.remove_subtree_postorder(id).map(|_| ())
    }

    fn insert_with_id(&mut self, id: NodeId, node: Box<dyn Node>) -> Result<(), NodeError> {
        // Use HashMap for high IDs (virtual nodes) to avoid Vec capacity overflow
        // Virtual node IDs start at a very high value that can't fit in a Vec
        const HIGH_ID_THRESHOLD: NodeId = 1_000_000_000; // 1 billion

        if id >= HIGH_ID_THRESHOLD {
            if self.high_id_nodes.contains_key(&id) {
                return Err(NodeError::AlreadyExists { id });
            }
            self.high_id_nodes.insert(id, node);
            self.high_id_warm_recycled_origins.insert(id, false);
            self.high_id_generations.entry(id).or_insert(0);
            Ok(())
        } else {
            if self.stable_to_physical.contains_key(&id) {
                return Err(NodeError::AlreadyExists { id });
            }

            let physical_id = if let Some(Reverse(id)) = self.free_ids.pop() {
                self.nodes[id] = Some(node);
                self.physical_stable_ids[id] = Self::pack_stable_id(id);
                self.physical_warm_recycled_origins[id] = false;
                id
            } else {
                self.ensure_dense_node_storage_capacity();
                let id = self.nodes.len();
                self.nodes.push(Some(node));
                self.physical_stable_ids.push(Self::pack_stable_id(id));
                self.physical_warm_recycled_origins.push(false);
                id
            };

            self.next_stable_id = self.next_stable_id.max(id.saturating_add(1));
            self.ensure_stable_index_capacity();
            self.stable_generations.entry(id).or_insert(0);
            self.physical_stable_ids[physical_id] = Self::pack_stable_id(id);
            self.stable_to_physical.insert(id, physical_id);
            Ok(())
        }
    }

    fn compact(&mut self) {
        let live_count = self.nodes.iter().filter(|slot| slot.is_some()).count();
        let tombstone_count = self.nodes.len().saturating_sub(live_count);
        if tombstone_count == 0 {
            return;
        }
        if self.nodes.len() > Self::EAGER_COMPACT_NODE_LEN && tombstone_count < live_count {
            return;
        }
        let rehouse_live_nodes = tombstone_count >= live_count;
        let mut packed_nodes = Vec::with_capacity(live_count);
        let mut packed_physical_stable_ids = Vec::with_capacity(live_count);
        let mut packed_warm_recycled_origins = Vec::with_capacity(live_count);
        let mut stable_to_physical = HashMap::default();
        stable_to_physical.reserve(live_count);

        for physical_id in 0..self.nodes.len() {
            let Some(mut node) = self.nodes[physical_id].take() else {
                continue;
            };
            if rehouse_live_nodes {
                if let Some(rehoused) = node.rehouse_for_live_compaction() {
                    node = rehoused;
                }
            }
            let stable_id = std::mem::replace(
                &mut self.physical_stable_ids[physical_id],
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
            packed_warm_recycled_origins.push(self.physical_warm_recycled_origins[physical_id]);
            stable_to_physical.insert(stable_id, packed_nodes.len() - 1);
        }

        self.nodes = packed_nodes;
        self.physical_stable_ids = packed_physical_stable_ids;
        self.physical_warm_recycled_origins = packed_warm_recycled_origins;
        self.free_ids = BinaryHeap::new();
        self.stable_to_physical = stable_to_physical;
        self.prune_stable_generations();
    }

    fn take_recycled_node(&mut self, key: TypeId) -> Option<RecycledNode> {
        Self::take_recycled_node_from_pool(&mut self.returning_recycled_nodes, key)
            .or_else(|| Self::take_recycled_node_from_pool(&mut self.recycled_nodes, key))
    }

    fn set_recycled_node_origin(&mut self, id: NodeId, warm_origin: bool) {
        if let Some(physical_id) = self.resolve_node_index(id) {
            self.physical_warm_recycled_origins[physical_id] = warm_origin;
        } else if self.high_id_nodes.contains_key(&id) {
            self.high_id_warm_recycled_origins.insert(id, warm_origin);
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
        *self.fresh_recyclable_creations.entry(key).or_insert(0) += 1;
    }

    fn clear_recycled_nodes(&mut self) {
        let returning = std::mem::take(&mut self.returning_recycled_nodes);
        for (key, mut nodes) in returning {
            let pool = self.recycled_nodes.entry(key).or_default();
            pool.append(&mut nodes);
        }

        let fresh_recyclable_creations = std::mem::take(&mut self.fresh_recyclable_creations);
        let cold = std::mem::take(&mut self.cold_recycled_nodes);
        for (key, mut nodes) in cold {
            let needed = fresh_recyclable_creations.get(&key).copied().unwrap_or(0);
            if needed > 0 {
                let remaining_limit = self
                    .recycle_pool_limit_for(key)
                    .unwrap_or(usize::MAX)
                    .saturating_sub(self.warm_recycled_pool_len(key));
                let promote = nodes.len().min(needed).min(remaining_limit);
                let split_at = nodes.len().saturating_sub(promote);
                let promoted = nodes.split_off(split_at);
                for mut recycled in promoted {
                    recycled.set_warm_origin(true);
                    self.recycled_nodes.entry(key).or_default().push(recycled);
                }
            }
        }

        let mut keys: HashSet<TypeId> = HashSet::default();
        keys.extend(self.recycled_nodes.keys().copied());
        keys.extend(self.recycled_node_limits.keys().copied());
        keys.extend(self.warm_recycled_node_targets.keys().copied());
        keys.extend(self.recycled_node_prototypes.keys().copied());
        for key in keys {
            let observed_demand = fresh_recyclable_creations.get(&key).copied().unwrap_or(0);
            let target = self.update_warm_recycled_node_target(key, observed_demand);
            self.replenish_warm_pool_to_target(key, target);
            self.trim_idle_warm_pool_to_target(key, target);
            self.compact_idle_warm_pool(key);
        }
        self.prune_stable_generations();
        // Compact the dense physical storage if it has grown very large relative
        // to the live node count (e.g. after a spike of recyclable nodes that
        // were all removed). This keeps warm_recycled_node_id_capacity bounded.
        self.compact();
    }
}

pub trait ApplierHost {
    fn borrow_dyn(&self) -> RefMut<'_, dyn Applier>;
    /// Compact internal storage after commands have been applied.
    fn compact(&self) {}
}

pub struct ConcreteApplierHost<A: Applier + 'static> {
    inner: RefCell<A>,
}

impl<A: Applier + 'static> ConcreteApplierHost<A> {
    pub fn new(applier: A) -> Self {
        Self {
            inner: RefCell::new(applier),
        }
    }

    pub fn borrow_typed(&self) -> RefMut<'_, A> {
        self.inner.borrow_mut()
    }

    pub fn try_borrow_typed(&self) -> Result<RefMut<'_, A>, std::cell::BorrowMutError> {
        self.inner.try_borrow_mut()
    }

    pub fn into_inner(self) -> A {
        self.inner.into_inner()
    }
}

impl<A: Applier + 'static> ApplierHost for ConcreteApplierHost<A> {
    fn borrow_dyn(&self) -> RefMut<'_, dyn Applier> {
        RefMut::map(self.inner.borrow_mut(), |applier| {
            applier as &mut dyn Applier
        })
    }

    fn compact(&self) {
        self.inner.borrow_mut().compact();
    }
}

pub struct ApplierGuard<'a, A: Applier + 'static> {
    inner: RefMut<'a, A>,
}

impl<'a, A: Applier + 'static> ApplierGuard<'a, A> {
    fn new(inner: RefMut<'a, A>) -> Self {
        Self { inner }
    }
}

impl<'a, A: Applier + 'static> Deref for ApplierGuard<'a, A> {
    type Target = A;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a, A: Applier + 'static> DerefMut for ApplierGuard<'a, A> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

pub struct SlotsHost {
    storage_key: Cell<usize>,
    inner: RefCell<SlotsHostInner>,
}

#[derive(Debug, Default)]
pub(crate) struct SlotPassOutcome {
    pub(crate) compacted: bool,
}

#[derive(Default)]
pub(crate) struct FinishedSlotPass {
    pub(crate) outcome: SlotPassOutcome,
    pub(crate) detached_root_children: Vec<slot::DetachedSubtree>,
}

struct ActivePassState {
    mode: slot::SlotPassMode,
    state: slot::SlotWriteSessionState,
}

struct SlotsHostInner {
    table: SlotTable,
    lifecycle: slot::SlotLifecycleCoordinator,
    runtime_state: Option<Rc<crate::composer::ComposerRuntimeState>>,
    active_pass: Option<ActivePassState>,
}

impl Drop for SlotsHost {
    fn drop(&mut self) {
        let storage_key = self.storage_key.get();
        let inner = self.inner.get_mut();
        if let Some(state) = inner.runtime_state.clone() {
            state.dispose_retained_subtrees_for_host(
                storage_key,
                &mut inner.table,
                &mut inner.lifecycle,
            );
            state.clear_host_storage_key(storage_key);
        }
        inner.lifecycle.dispose_slot_table(&mut inner.table);
    }
}

impl SlotsHost {
    pub(crate) fn storage_key(&self) -> usize {
        self.storage_key.get()
    }

    pub fn new(storage: SlotTable) -> Self {
        let storage_key = storage.storage_id();
        let runtime_state = storage.runtime_state();
        Self {
            storage_key: Cell::new(storage_key),
            inner: RefCell::new(SlotsHostInner {
                table: storage,
                lifecycle: slot::SlotLifecycleCoordinator::default(),
                runtime_state,
                active_pass: None,
            }),
        }
    }

    pub(crate) fn bind_runtime_state(&self, state: &Rc<crate::composer::ComposerRuntimeState>) {
        let mut inner = self.inner.borrow_mut();
        inner.runtime_state = Some(Rc::clone(state));
        inner.table.bind_runtime_state(state);
    }

    pub(crate) fn runtime_state(&self) -> Option<Rc<crate::composer::ComposerRuntimeState>> {
        self.inner.borrow().runtime_state.clone()
    }

    pub(crate) fn borrow(&self) -> Ref<'_, SlotTable> {
        Ref::map(self.inner.borrow(), |inner| &inner.table)
    }

    pub(crate) fn borrow_mut(&self) -> RefMut<'_, SlotTable> {
        RefMut::map(self.inner.borrow_mut(), |inner| &mut inner.table)
    }

    pub fn take(&self) -> SlotTable {
        let inner = self.inner.borrow();
        assert!(
            inner.active_pass.is_none(),
            "cannot take SlotsHost during an active pass"
        );
        drop(inner);
        let mut inner = self.inner.borrow_mut();
        inner.lifecycle.flush_pending_drops();
        let taken = std::mem::take(&mut inner.table);
        self.storage_key.set(inner.table.storage_id());
        inner.runtime_state = inner.table.runtime_state();
        taken
    }

    pub fn reset(&self) {
        let inner = self.inner.borrow();
        assert!(
            inner.active_pass.is_none(),
            "cannot reset SlotsHost during an active pass"
        );
        drop(inner);
        if let Some(state) = self.runtime_state() {
            assert!(
                state.host_retention_is_empty(self),
                "cannot reset SlotsHost while retained subtrees are still owned by the host"
            );
            state.clear_host(self);
        }
        let mut inner = self.inner.borrow_mut();
        let mut lifecycle = std::mem::take(&mut inner.lifecycle);
        lifecycle.dispose_slot_table(&mut inner.table);
        inner.table = SlotTable::default();
        self.storage_key.set(inner.table.storage_id());
        inner.runtime_state = inner.table.runtime_state();
        inner.lifecycle = slot::SlotLifecycleCoordinator::default();
    }

    pub(crate) fn debug_stats(&self) -> SlotTableDebugStats {
        let inner = self.inner.borrow();
        let mut stats = inner.table.debug_stats();
        inner.lifecycle.fill_debug_stats(&mut stats);
        if let Some(state) = inner.runtime_state.clone() {
            state.fill_slot_debug_stats(self, &mut stats);
        }
        stats
    }

    pub(crate) fn debug_snapshot(&self) -> slot::SlotDebugSnapshot {
        let inner = self.inner.borrow();
        let mut snapshot = inner.table.debug_snapshot();
        if let Some(state) = inner.runtime_state.clone() {
            state.fill_slot_debug_snapshot(self, &mut snapshot);
        }
        snapshot
    }

    pub(crate) fn begin_pass(&self, mode: slot::SlotPassMode) {
        let mut inner = self.inner.borrow_mut();
        assert!(
            inner.active_pass.is_none(),
            "slot pass already active for host"
        );
        let mut state = slot::SlotWriteSessionState::default();
        state.reset_for_pass(&inner.table, mode);
        inner.active_pass = Some(ActivePassState { mode, state });
    }

    pub(crate) fn has_active_pass(&self) -> bool {
        self.inner.borrow().active_pass.is_some()
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
            ..
        } = &mut *inner;
        let active_pass = active_pass
            .as_mut()
            .expect("slot write session requires an active pass");
        let active_mode = active_pass.mode;
        let mut session = table.write_session(lifecycle, &mut active_pass.state, active_mode);
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

    pub(crate) fn finish_pass(&self, applier: &mut dyn Applier) -> FinishedSlotPass {
        let mut inner = self.inner.borrow_mut();
        let SlotsHostInner {
            table,
            lifecycle,
            active_pass: active_pass_slot,
            ..
        } = &mut *inner;
        let Some(mut active_pass) = active_pass_slot.take() else {
            return FinishedSlotPass::default();
        };

        #[cfg(debug_assertions)]
        if let Err(err) = active_pass.state.validate(table) {
            panic!("slot writer invariant violation before finalize_pass: {err:?}");
        }

        let detached_root_children = {
            let mut session =
                table.write_session(lifecycle, &mut active_pass.state, active_pass.mode);
            session.finalize_pass(applier)
        };

        FinishedSlotPass {
            outcome: SlotPassOutcome {
                compacted: active_pass.state.request_compaction,
            },
            detached_root_children,
        }
    }

    pub(crate) fn complete_pass_cleanup(&self, compacted: bool) {
        let mut inner = self.inner.borrow_mut();
        let SlotsHostInner {
            table,
            lifecycle,
            runtime_state,
            ..
        } = &mut *inner;
        lifecycle.flush_pending_drops();
        if compacted {
            table.compact_storage();
            lifecycle.compact_storage();
        }
        if let Some(state) = runtime_state.clone() {
            state.compact_table_namespaces_for_host(self, table);
        } else {
            table.compact_payload_anchor_namespace(None);
        }
        table.debug_verify(Some(lifecycle));
        if let Some(state) = runtime_state.clone() {
            state.debug_verify_host(self, table);
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
pub use state::{MutableState, OwnedMutableState, SnapshotStateList, SnapshotStateMap, State};

fn hash_key<K: Hash>(key: &K) -> Key {
    let mut hasher = hash::default::new();
    key.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn explicit_group_key_seed<K: Hash>(
    key: &K,
    caller: &'static std::panic::Location<'static>,
) -> slot_storage::GroupKeySeed {
    let source_key = location_key(caller.file(), caller.line(), caller.column());
    let explicit_key = hash_key(key);
    slot_storage::GroupKeySeed::keyed(source_key, explicit_key)
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/recursive_decrease_increase_test.rs"]
mod recursive_decrease_increase_test;

pub mod collections;
pub mod hash;
