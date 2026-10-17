#![deny(missing_docs)]
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]

pub extern crate self as cranpose_core;

mod applier_host;
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
mod effect_key;
mod emit;
pub mod env_flags;
#[cfg(any(feature = "internal", test))]
mod frame_clock;
mod hooks;
mod launched_effect;
mod layout_dirt_roots;
mod node_marks;
/// `Owned`, the handle to a value that a composition remembers.
pub mod owned;
pub mod platform;
mod recompose;
mod retention;
/// The runtime in which a composition runs: state cells, UI tasks, frame callbacks and the queue of
/// invalidated scopes.
pub mod runtime;
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
    BranchGroupGuard, CapturedCompositionContext, ComposableGroup, Composer,
    ScopedBranchGroupGuard, ValueSlotHandle,
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
pub use node_marks::NodeMarks;
pub use owned::Owned;
pub use platform::{Clock, RuntimeScheduler, SchedulerRef, scheduler_ref};
pub use retention::{RetentionBudget, RetentionEvictionPolicy, RetentionMode, RetentionPolicy};
#[doc(hidden)]
pub use runtime::{
    DefaultScheduler, Runtime, RuntimeHandle, StateId, TaskHandle, UiDispatcher,
    current_runtime_handle, label_next_ui_task, schedule_frame, schedule_node_update,
};
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
    any::{Any, TypeId},
    cell::{Cell, Ref, RefCell, RefMut},
    cmp::Reverse,
    collections::BinaryHeap,
    hash::{Hash, Hasher},
    rc::{Rc, Weak},
    sync::OnceLock,
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

#[cfg(any(test, debug_assertions))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LocationKeyDebugInfo {
    file: String,
    line: u32,
    column: u32,
}

#[cfg(any(test, debug_assertions))]
fn register_location_key_debug_info(key: Key, file: &str, line: u32, column: u32) {
    let info = || LocationKeyDebugInfo {
        file: file.to_owned(),
        line,
        column,
    };
    let collision = thread_state::runtimes(|state| {
        let mut registry = state.location_keys.borrow_mut();
        match registry.entry(key) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                thread_state::watch_thread_exit();
                entry.insert(info());
                None
            }
            std::collections::hash_map::Entry::Occupied(entry) => {
                let existing = entry.get();
                (existing.file != file || existing.line != line || existing.column != column)
                    .then(|| (existing.clone(), info()))
            }
        }
    });
    if let Some((existing, incoming)) = collision {
        thread_state::cells(|cells| {
            let count = &cells.location_key_collisions;
            count.set(count.get().saturating_add(1));
        });
        log::error!("location key collision: key={key} first={existing:?} second={incoming:?}");
    }
}

#[cfg(all(debug_assertions, not(test)))]
fn location_key_diagnostics_enabled() -> bool {
    crate::env_flag!("CRANPOSE_LOCATION_KEY_DIAGNOSTICS")
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
pub(crate) fn location_key_debug_collision_count_for_test() -> usize {
    thread_state::cells(|cells| cells.location_key_collisions.get())
}

#[cfg(test)]
pub(crate) fn location_key_debug_info_for_test(key: Key) -> Option<LocationKeyDebugInfo> {
    thread_state::runtimes(|state| state.location_keys.borrow().get(&key).cloned())
}

#[cfg(test)]
pub(crate) fn slot_validation_diagnostics_enabled() -> bool {
    true
}

#[cfg(all(debug_assertions, not(test)))]
pub(crate) fn slot_validation_diagnostics_enabled() -> bool {
    crate::env_flag!("CRANPOSE_VALIDATE_SLOTS")
}

const fn source_location_hash(file: &str, line: u32, column: u32) -> u64 {
    position_location_hash(file_location_hash(file), line, column)
}

const fn file_location_hash(file: &str) -> u64 {
    fnv1a_location_key_bytes(0xcbf2_9ce4_8422_2325u64, file.as_bytes())
}

/// [`file_location_hash`] of a caller's source path, remembered by the
/// path's address: a `&'static str` keeps its bytes for the whole run, and
/// hashing the path on every composable call cost an eighth of composing a
/// feed item.
fn static_file_location_hash(cells: &thread_state::ThreadCells, file: &'static str) -> u64 {
    let address = file.as_ptr() as usize;
    let cached = &cells.file_hashes[(address >> 4) % thread_state::FILE_HASH_SLOTS];
    let (cached_address, cached_len, hash) = cached.get();
    if cached_address == address && cached_len == file.len() {
        return hash;
    }
    let hash = file_location_hash(file);
    cached.set((address, file.len(), hash));
    hash
}

const fn position_location_hash(mut hash: u64, line: u32, column: u32) -> u64 {
    hash = fnv1a_location_key_bytes(hash, &[0xff]);
    hash = fnv1a_location_key_bytes(hash, &line.to_le_bytes());
    hash = fnv1a_location_key_bytes(hash, &[0xfe]);
    hash = fnv1a_location_key_bytes(hash, &column.to_le_bytes());
    hash
}

const fn fnv1a_location_key_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    let mut index = 0;
    while index < bytes.len() {
        hash = (hash ^ bytes[index] as u64).wrapping_mul(0x0000_0100_0000_01b3);
        index += 1;
    }
    hash
}

const fn avalanche_location_key(mut value: u64) -> u64 {
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51_afd7_ed55_8ccd);
    value ^= value >> 33;
    value = value.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    value ^ (value >> 33)
}

#[doc(hidden)]
#[track_caller]
pub fn caller_location_key() -> Key {
    let caller = std::panic::Location::caller();
    let file = caller.file();
    if cfg!(feature = "hot-reload")
        && let Some(key) = hot_call_site_key(file, caller.line(), caller.column())
    {
        return key;
    }
    call_site_key(caller)
}

/// The key of a call site, remembered by its `Location`'s address. Each call
/// site's `Location` is a static, so its address names it for the whole run,
/// and a hit skips hashing the file, line and column on every `remember` and
/// parameter slot. The key is still made from the location's contents, so
/// two statics for one location give the same key.
fn call_site_key(caller: &'static std::panic::Location<'static>) -> Key {
    let address = std::ptr::from_ref(caller).addr();
    thread_state::cells(|cells| {
        let cached = &cells.call_site_keys[(address >> 3) % thread_state::CALL_SITE_KEY_SLOTS];
        let (cached_address, key) = cached.get();
        if cached_address == address {
            return key;
        }
        let file = caller.file();
        let key = registered_location_key(
            static_file_location_hash(cells, file),
            file,
            caller.line(),
            caller.column(),
        );
        cached.set((address, key));
        key
    })
}

#[derive(Clone, Copy)]
pub(crate) struct HotOrigin {
    file: &'static str,
    line: u32,
    end: u32,
    identity: Key,
}

impl HotOrigin {
    pub(crate) const NONE: Self = Self {
        file: "",
        line: 0,
        end: 0,
        identity: 0,
    };
}

/// Restores the previous hot-reload origin when a composable body or closure ends.
#[doc(hidden)]
pub struct HotOriginGuard {
    previous: HotOrigin,
}

impl Drop for HotOriginGuard {
    fn drop(&mut self) {
        let previous = self.previous;
        thread_state::cells(|cells| cells.hot_origin.set(previous));
    }
}

/// Marks the lexically enclosing composable definition for the
/// development-only `hot-reload` expansion. Call sites inside its source,
/// lines `line..=end` of `file`, are keyed by the definition's identity and
/// their line relative to it, so code moved by an edit elsewhere in the file
/// keeps its identity after a hot patch. Other call sites keep absolute keys.
/// Within one build the keys are as distinct as absolute locations.
#[doc(hidden)]
pub fn hot_origin(file: &'static str, line: u32, end: u32, identity: Key) -> HotOriginGuard {
    let next = HotOrigin {
        file,
        line,
        end,
        identity,
    };
    HotOriginGuard {
        previous: thread_state::cells(|cells| cells.hot_origin.replace(next)),
    }
}

fn hot_call_site_key(file: &str, line: u32, column: u32) -> Option<Key> {
    let origin = thread_state::cells(|cells| cells.hot_origin.get());
    if origin.file.is_empty() || !(origin.line..=origin.end).contains(&line) || origin.file != file
    {
        return None;
    }
    let relative = line.wrapping_sub(origin.line);
    let hash = fnv1a_location_key_bytes(origin.identity, &relative.to_le_bytes());
    Some(avalanche_location_key(fnv1a_location_key_bytes(
        hash ^ 0xfc,
        &column.to_le_bytes(),
    )))
}

/// Branch group key for the development-only `hot-reload` expansion: the
/// file and a hash of the guard's structural path, independent of lines.
#[doc(hidden)]
pub const fn hot_branch_key(file: &str, path_hash: u64) -> Key {
    let hash = file_location_hash(file);
    avalanche_location_key(fnv1a_location_key_bytes(
        hash ^ 0xfd,
        &path_hash.to_le_bytes(),
    ))
}

/// Composable definition key for the development-only `hot-reload`
/// expansion: the file, module and function name, independent of lines.
#[doc(hidden)]
pub const fn hot_definition_key(file: &str, module: &str, name: &str) -> Key {
    let mut hash = file_location_hash(file);
    hash = fnv1a_location_key_bytes(hash ^ 0xfe, module.as_bytes());
    avalanche_location_key(fnv1a_location_key_bytes(hash ^ 0xff, name.as_bytes()))
}

#[doc(hidden)]
#[track_caller]
pub fn composable_identity_key(definition: Key) -> Key {
    (definition.wrapping_mul(0x0000_0100_0000_01b3) ^ caller_location_key())
        .wrapping_mul(0x0000_0100_0000_01b3)
}

#[doc(hidden)]
pub fn composable_definition_key(
    file: &str,
    line: u32,
    column: u32,
    marker: std::any::TypeId,
) -> Key {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&marker, &mut hasher);
    location_key(file, line, column) ^ avalanche_location_key(std::hash::Hasher::finish(&hasher))
}

/// The key [`caller_location_key`] gives a call site, as a constant the
/// `#[composable]` macro writes, so the call takes no thread-local lookup.
#[doc(hidden)]
pub const fn const_location_key(file: &str, line: u32, column: u32) -> Key {
    avalanche_location_key(source_location_hash(file, line, column))
}

/// [`location_key()`] of the place it is written, worked out at build time
/// instead of hashing the source path on every call.
#[macro_export]
macro_rules! location_key {
    () => {{
        const KEY: $crate::Key = $crate::const_location_key(file!(), line!(), column!());
        $crate::noted_location_key(KEY, file!(), line!(), column!())
    }};
}

/// Returns a constant key the `#[composable]` macro wrote, after recording
/// where it came from for collision diagnostics in debug builds.
#[doc(hidden)]
#[inline(always)]
pub fn noted_location_key(key: Key, file: &str, line: u32, column: u32) -> Key {
    note_location_key(key, file, line, column);
    key
}

#[doc(hidden)]
pub fn cached_composable_definition_key(
    cell: &OnceLock<Key>,
    file: &str,
    line: u32,
    column: u32,
    marker: TypeId,
) -> Key {
    *cell.get_or_init(|| composable_definition_key(file, line, column, marker))
}

/// Returns the key of a source location: a hash of the file path together with the line and column.
/// The same location always gives the same key.
pub fn location_key(file: &str, line: u32, column: u32) -> Key {
    registered_location_key(file_location_hash(file), file, line, column)
}

fn registered_location_key(file_hash: u64, file: &str, line: u32, column: u32) -> Key {
    let key = avalanche_location_key(position_location_hash(file_hash, line, column));
    note_location_key(key, file, line, column);
    key
}

/// Records where `key` came from, for collision diagnostics in debug builds.
#[cfg(any(test, debug_assertions))]
fn note_location_key(key: Key, file: &str, line: u32, column: u32) {
    #[cfg(test)]
    register_location_key_debug_info(key, file, line, column);
    #[cfg(all(debug_assertions, not(test)))]
    if location_key_diagnostics_enabled() {
        register_location_key_debug_info(key, file, line, column);
    }
}

#[cfg(not(any(test, debug_assertions)))]
fn note_location_key(_key: Key, _file: &str, _line: u32, _column: u32) {}

#[doc(hidden)]
#[inline]
pub fn __branch_group_scope_deferred(key: Key) -> Option<ScopedBranchGroupGuard> {
    composer_context::with_current_core(|core| {
        ScopedBranchGroupGuard::open(&core.branch_folds, key)
    })
}

/// Moves a content closure's statement guard to the next statement, keyed
/// `key`: the guard the next statement would open, without looking up the
/// composer again. A guard opened outside composition stays empty.
#[doc(hidden)]
#[inline]
pub fn __branch_group_scope_advance(guard: &mut Option<ScopedBranchGroupGuard>, key: Key) {
    if let Some(guard) = guard {
        guard.advance(key);
    }
}

/// The key of the `branch`th branch guard the `#[composable]` macro wrote at
/// a source position, as a constant.
#[doc(hidden)]
pub const fn branch_location_key(file: &str, line: u32, column: u32, branch: u32) -> Key {
    let mut hash = source_location_hash(file, line, column);
    hash = fnv1a_location_key_bytes(hash, &[0xfd]);
    hash = fnv1a_location_key_bytes(hash, &branch.to_le_bytes());
    avalanche_location_key(hash)
}

/// The key of the `index`th constant value the `#[composable]` macro keeps
/// at a source position, as a constant.
#[doc(hidden)]
pub const fn constant_value_key(file: &str, line: u32, column: u32, index: u32) -> Key {
    let mut hash = source_location_hash(file, line, column);
    hash = fnv1a_location_key_bytes(hash, &[0xfc]);
    hash = fnv1a_location_key_bytes(hash, &index.to_le_bytes());
    avalanche_location_key(hash)
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
    pub(crate) const INVALID: AnchorId = AnchorId {
        id: 0,
        generation: 0,
    };

    pub(crate) fn new(id: usize) -> Self {
        Self {
            id: crate::slot::checked_usize_to_u32(id, "anchor id"),
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

/// A composable's body. The scope passes itself when it runs, so the
/// callback needs neither a second box nor a reference back to its scope; a
/// capture-free body records how to name its type, to be kept as is.
struct RecomposeCallback {
    body: Box<dyn FnMut(&Composer) + 'static>,
    stateless_body: Option<fn() -> TypeId>,
}

fn scope_activity_epoch() -> u64 {
    thread_state::cells(|cells| cells.scope_activity_epoch.get())
}

fn note_scope_activity_change() {
    thread_state::cells(|cells| {
        let epoch = &cells.scope_activity_epoch;
        epoch.set(epoch.get() + 1);
    });
}

/// Runs `read` on what `cell` holds and puts it back: a scope's links are
/// read this way rather than through a `RefCell`, whose borrow flag would
/// take a word in every scope of a composition. `read` must not reach the
/// same cell again.
fn read_cell<T: Default, R>(cell: &Cell<T>, read: impl FnOnce(&T) -> R) -> R {
    let value = cell.take();
    let result = read(&value);
    cell.set(value);
    result
}

/// Like [`read_cell`], for a change to what `cell` holds.
fn update_cell<T: Default, R>(cell: &Cell<T>, update: impl FnOnce(&mut T) -> R) -> R {
    let mut value = cell.take();
    let result = update(&mut value);
    cell.set(value);
    result
}

/// How a scope is tied to the scope its lifetime follows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OwnerKind {
    /// The scope it composes under, its recomposition ancestor.
    Parent,
    /// For the root of a slot table composed for another scope's content,
    /// that scope, which is no recomposition ancestor.
    Lifetime,
}

/// The scope a scope's lifetime follows, and how it is tied to it.
type OwnerLink = Cell<Option<(OwnerKind, Weak<RecomposeScopeInner>)>>;

/// The node id a scope's `parent_hint` holds for none: ids index the
/// applier's nodes, which never reach it.
const NO_PARENT_HINT: NodeId = NodeId::MAX;

pub(crate) struct RecomposeScopeInner {
    runtime: runtime::ScopeRuntime,
    invalid: Cell<bool>,
    enqueued: Cell<bool>,
    active: Cell<bool>,
    active_epoch: Cell<u64>,
    deactivations: Cell<u64>,
    composed_once: Cell<bool>,
    pending_recompose: Cell<bool>,
    /// The recompose callback is running: it is out of `recompose` meanwhile,
    /// and the scope still recomposes what reads in it.
    recomposing: Cell<bool>,
    force_reuse: Cell<bool>,
    force_recompose: Cell<bool>,
    derivation: Cell<bool>,
    retention_mode: Cell<RetentionMode>,
    /// [`NO_PARENT_HINT`] for none.
    parent_hint: Cell<NodeId>,
    group_anchor: Cell<AnchorId>,
    recompose: Cell<Option<RecomposeCallback>>,
    owner: OwnerLink,
    local_stack: Cell<LocalStackSnapshot>,
    #[cfg(feature = "inspection")]
    source_trace: RefCell<Rc<[source_trace::SourceLocation]>>,
    #[cfg(all(feature = "inspection", debug_assertions))]
    recompositions: std::cell::OnceCell<source_trace::RecompositionCounter>,
    slots_storage_key: Cell<usize>,
    slots_runtime_state: Cell<Option<std::rc::Weak<crate::composer::ComposerRuntimeState>>>,
    /// The states the scope is subscribed to: what its last body read, in
    /// read order.
    state_subscriptions: RefCell<StateIds>,
    /// What the body running now read so far, once it left the order of
    /// `state_subscriptions`.
    run_reads: RefCell<StateIds>,
    /// How many subscriptions the body running now read in order, or
    /// [`DIVERGED`] or [`NOT_READING`].
    read_cursor: Cell<u32>,
    invalidation_sources: Cell<StateIds>,
    unknown_invalidation_source: Cell<bool>,
}

type StateIds = SmallVec<[StateId; 2]>;

/// What [`RecomposeScope::note_read`] made of a read.
pub(crate) enum ScopeRead {
    /// The read repeats the last body's, so the state watches the scope.
    Watched,
    /// The read went into the running body's reads; the state may not watch
    /// the scope yet.
    Collected,
    /// No body of the scope is running.
    NotReading,
}

/// A scope's `read_cursor` while no body of it runs.
const NOT_READING: u32 = u32::MAX;
/// A scope's `read_cursor` once the running body left the order of the reads
/// before it.
const DIVERGED: u32 = u32::MAX - 1;

enum ScopeOwner {
    Root,
    Live(Rc<RecomposeScopeInner>),
    Dropped,
}

impl RecomposeScopeInner {
    fn owner(&self) -> ScopeOwner {
        read_cell(&self.owner, |owner| {
            owner.as_ref().map_or(ScopeOwner::Root, |(_, owner)| {
                owner
                    .upgrade()
                    .map_or(ScopeOwner::Dropped, ScopeOwner::Live)
            })
        })
    }

    fn is_effectively_active(&self, epoch: u64) -> bool {
        if self.active_epoch.get() == epoch {
            return true;
        }
        if !self.active.get() {
            return false;
        }
        let active = match self.owner() {
            ScopeOwner::Root => true,
            ScopeOwner::Dropped => false,
            ScopeOwner::Live(owner) => owner.is_effectively_active(epoch),
        };
        if active {
            self.active_epoch.set(epoch);
        }
        active
    }

    fn new(runtime: runtime::ScopeRuntime) -> Self {
        runtime.increment_live_recompose_scope_count();
        Self {
            runtime,
            invalid: Cell::new(false),
            enqueued: Cell::new(false),
            active: Cell::new(true),
            active_epoch: Cell::new(0),
            deactivations: Cell::new(0),
            composed_once: Cell::new(false),
            pending_recompose: Cell::new(false),
            recomposing: Cell::new(false),
            force_reuse: Cell::new(false),
            force_recompose: Cell::new(false),
            derivation: Cell::new(false),
            retention_mode: Cell::new(RetentionMode::DisposeWhenInactive),
            parent_hint: Cell::new(NO_PARENT_HINT),
            group_anchor: Cell::new(AnchorId::INVALID),
            recompose: Cell::new(None),
            owner: Cell::new(None),
            local_stack: Cell::new(None),
            #[cfg(feature = "inspection")]
            source_trace: RefCell::new(Rc::from([])),
            #[cfg(all(feature = "inspection", debug_assertions))]
            recompositions: std::cell::OnceCell::new(),
            slots_storage_key: Cell::new(0),
            slots_runtime_state: Cell::new(None),
            state_subscriptions: RefCell::new(StateIds::new()),
            run_reads: RefCell::new(StateIds::new()),
            read_cursor: Cell::new(NOT_READING),
            invalidation_sources: Cell::new(StateIds::new()),
            unknown_invalidation_source: Cell::new(false),
        }
    }

    fn id(&self) -> ScopeId {
        std::ptr::from_ref(self).addr()
    }
}

fn push_unique_state_id(ids: &mut StateIds, state_id: StateId) {
    if !ids.contains(&state_id) {
        ids.push(state_id);
    }
}

impl Drop for RecomposeScopeInner {
    fn drop(&mut self) {
        note_scope_activity_change();
        let id = self.id();
        self.runtime.decrement_live_recompose_scope_count();
        let subscriptions = std::mem::take(self.state_subscriptions.get_mut());
        let run_reads = std::mem::take(self.run_reads.get_mut());
        for state_id in subscriptions.into_iter().chain(run_reads) {
            self.runtime.unregister_state_scope(state_id, id);
        }
        #[cfg(debug_assertions)]
        {
            thread_state::try_runtimes(|state| {
                state.scope_labels.borrow_mut().remove(&id);
                state.scope_invalidation_sources.borrow_mut().remove(&id);
            });
        }
        if self.enqueued.replace(false) {
            self.runtime.mark_scope_recomposed();
        }
    }
}

/// Size of the registry of live recompose scopes, for leak checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecomposeScopeRegistryDebugStats {
    /// Scopes registered.
    pub len: usize,
    /// Capacity reserved for the registry.
    pub capacity: usize,
}

/// The unit of recomposition. State read inside a scope invalidates it, and a recomposition runs
/// only the content of the invalid scopes. Clones refer to the same scope.
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
    /// Marks this scope as one that only recomputes a derived state: running
    /// it changes nothing a composition shows unless the value it writes
    /// invalidates the scopes that read it.
    pub(crate) fn mark_derivation(&self) {
        self.inner.derivation.set(true);
    }

    pub(crate) fn is_derivation(&self) -> bool {
        self.inner.derivation.get()
    }

    fn new(runtime: RuntimeHandle) -> Self {
        Self {
            inner: Rc::new(RecomposeScopeInner::new(runtime.scope_runtime())),
        }
    }

    pub(crate) fn downgrade(&self) -> Weak<RecomposeScopeInner> {
        Rc::downgrade(&self.inner)
    }

    pub(crate) fn upgrade(weak: &Weak<RecomposeScopeInner>) -> Option<Self> {
        weak.upgrade().map(|inner| Self { inner })
    }

    /// Returns an id that is unique among the live scopes.
    pub fn id(&self) -> ScopeId {
        self.inner.id()
    }

    /// Returns `true` if state that the scope read has changed since it last recomposed.
    pub fn is_invalid(&self) -> bool {
        self.inner.invalid.get()
    }

    pub(crate) fn is_enqueued(&self) -> bool {
        self.inner.enqueued.get()
    }

    /// Returns `false` between `deactivate` and `reactivate`.
    pub fn is_active(&self) -> bool {
        self.inner.active.get()
    }

    /// Total deactivations along this scope's owner chain. A retained slot
    /// composition records this at compose time; a later mismatch means some
    /// enclosing composition was deactivated (its owned effects cancelled)
    /// since the slot last composed, so the retained content must recompose
    /// once to restart them — no flag on the slot's own scopes carries that
    /// trace, because deactivation walks stop at slot-host boundaries.
    pub fn owner_chain_deactivation_epoch(&self) -> u64 {
        let mut total = self.inner.deactivations.get();
        let mut owner = self.inner.owner();
        while let ScopeOwner::Live(scope) = owner {
            total = total.wrapping_add(scope.deactivations.get());
            owner = scope.owner();
        }
        total
    }

    pub(crate) fn is_effectively_active(&self) -> bool {
        self.inner.is_effectively_active(scope_activity_epoch())
    }

    fn record_state_subscription(&self, state_id: StateId) {
        push_unique_state_id(&mut self.inner.state_subscriptions.borrow_mut(), state_id);
    }

    /// Starts collecting the states a body of the scope reads.
    pub(crate) fn begin_reads(&self) {
        self.inner.read_cursor.set(0);
    }

    /// Notes that the body running now read `state_id`. While the body reads
    /// what the last one read, in the same order, a read only moves the
    /// cursor along the subscriptions, and the state is known to watch the
    /// scope already; the first read off that order copies the reads so far
    /// into `run_reads`.
    #[inline]
    fn note_read(&self, state_id: StateId) -> ScopeRead {
        let cursor = self.inner.read_cursor.get();
        if cursor == NOT_READING {
            return ScopeRead::NotReading;
        }
        if cursor == DIVERGED {
            let mut reads = self.inner.run_reads.borrow_mut();
            if reads.last() != Some(&state_id) {
                reads.push(state_id);
            }
            return ScopeRead::Collected;
        }
        let at = cursor as usize;
        let subscribed = self.inner.state_subscriptions.borrow();
        if subscribed.get(at) == Some(&state_id) {
            self.inner.read_cursor.set(cursor + 1);
            return ScopeRead::Watched;
        }
        if at > 0 && subscribed[at - 1] == state_id {
            return ScopeRead::Watched;
        }
        let mut reads = self.inner.run_reads.borrow_mut();
        reads.clear();
        reads.extend_from_slice(&subscribed[..at]);
        reads.push(state_id);
        self.inner.read_cursor.set(DIVERGED);
        ScopeRead::Collected
    }

    /// Ends collecting reads. A body that ran makes its reads the scope's
    /// subscriptions and drops the states it did not read again, as Compose
    /// does; a skipped or unfinished one keeps the subscriptions it had.
    #[inline]
    pub(crate) fn end_reads(&self, ran: bool) {
        let cursor = self.inner.read_cursor.replace(NOT_READING);
        // A skipped body that read in order keeps the subscriptions as they
        // are.
        if cursor == NOT_READING || (!ran && cursor != DIVERGED) {
            return;
        }
        self.settle_reads(cursor, ran);
    }

    /// The part of [`Self::end_reads`] for a body that ran or left the
    /// order of its reads: `cursor` is where the read cursor stood.
    #[inline(never)]
    fn settle_reads(&self, cursor: u32, ran: bool) {
        let id = self.id();
        let mut subscribed = self.inner.state_subscriptions.borrow_mut();
        if cursor != DIVERGED {
            // The body read a prefix of the subscriptions, in order.
            let read = cursor as usize;
            if ran && read < subscribed.len() {
                for &state_id in &subscribed[read..] {
                    if !subscribed[..read].contains(&state_id) {
                        self.inner.runtime.forget_state_scope(state_id, id);
                    }
                }
                subscribed.truncate(read);
            }
            return;
        }
        let mut reads = self.inner.run_reads.borrow_mut();
        if ran {
            for &state_id in subscribed.iter() {
                if !reads.contains(&state_id) {
                    self.inner.runtime.forget_state_scope(state_id, id);
                }
            }
            std::mem::swap(&mut *reads, &mut *subscribed);
            reads.clear();
        } else {
            for state_id in reads.drain(..) {
                push_unique_state_id(&mut subscribed, state_id);
            }
        }
    }

    fn record_unknown_invalidation_source(&self) {
        self.inner.unknown_invalidation_source.set(true);
        update_cell(&self.inner.invalidation_sources, StateIds::clear);
    }

    fn record_state_invalidation_source(&self, state_id: StateId) {
        if !self.inner.unknown_invalidation_source.get() {
            update_cell(&self.inner.invalidation_sources, |ids| {
                push_unique_state_id(ids, state_id);
            });
        }
    }

    fn enqueue_invalidation(&self) {
        self.inner.invalid.set(true);
        if !self.is_effectively_active() {
            return;
        }
        if !self.inner.enqueued.replace(true) {
            self.inner.runtime.register_invalid_scope(self.downgrade());
        }
    }

    fn invalidate(&self) {
        self.record_unknown_invalidation_source();
        self.enqueue_invalidation();
    }

    pub(crate) fn invalidate_from_state(&self, state_id: StateId) {
        self.record_state_invalidation_source(state_id);
        self.enqueue_invalidation();
    }

    fn mark_recomposed(&self) {
        // An invalidation notes its source and sets `invalid` together, so a
        // scope nothing invalidated has no source to forget.
        if self.inner.invalid.replace(false) {
            self.inner.unknown_invalidation_source.set(false);
            update_cell(&self.inner.invalidation_sources, StateIds::clear);
        }
        self.inner.force_reuse.set(false);
        self.inner.force_recompose.set(false);
        if self.inner.enqueued.replace(false) {
            self.inner.runtime.mark_scope_recomposed();
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

    /// Registers `callback` as the code that recomposes the scope when state
    /// its body read changes. A stateless body the scope reruns already
    /// stays as it is.
    pub(crate) fn set_recompose_callback<F>(&self, callback: F)
    where
        F: FnMut(&Composer) + 'static,
    {
        let body: fn() -> TypeId = TypeId::of::<F>;
        let stateless = std::mem::size_of::<F>() == 0;
        if stateless && self.reruns_stateless(body) {
            return;
        }
        self.set_boxed_recompose(Box::new(callback), stateless.then_some(body));
    }

    #[inline(never)]
    fn set_boxed_recompose(
        &self,
        body: Box<dyn FnMut(&Composer) + 'static>,
        stateless_body: Option<fn() -> TypeId>,
    ) {
        #[cfg(feature = "inspection")]
        self.inner
            .source_trace
            .replace(source_trace::current_source_trace());
        self.inner.recompose.set(Some(RecomposeCallback {
            body,
            stateless_body,
        }));
    }

    /// Whether the scope reruns the stateless body whose type `body` names.
    /// The same function names the same type; a copy of it in another
    /// codegen unit is told by the type itself.
    fn reruns_stateless(&self, body: fn() -> TypeId) -> bool {
        read_cell(&self.inner.recompose, |callback| {
            callback
                .as_ref()
                .and_then(|callback| callback.stateless_body)
                .is_some_and(|current| std::ptr::fn_addr_eq(current, body) || current() == body())
        })
    }

    fn run_recompose(&self, composer: &Composer) -> bool {
        #[cfg(feature = "inspection")]
        let _source_context = source_trace::restore_source_trace(&self.inner.source_trace.borrow());
        let Some(mut callback) = self.inner.recompose.take() else {
            return false;
        };
        self.begin_reads();
        self.inner.recomposing.set(true);
        (callback.body)(composer);
        self.inner.recomposing.set(false);
        self.end_reads(true);
        // The body may have stored a new callback for the scope while it ran.
        update_cell(&self.inner.recompose, |slot| {
            if slot.is_none() {
                *slot = Some(callback);
            }
        });
        true
    }

    fn has_recompose_callback(&self) -> bool {
        self.inner.recomposing.get() || read_cell(&self.inner.recompose, Option::is_some)
    }

    /// How many scopes this one composes under.
    pub(crate) fn depth(&self) -> usize {
        let mut depth = 0;
        let mut current = self.parent_scope();
        while let Some(scope) = current {
            depth += 1;
            current = scope.parent_scope();
        }
        depth
    }

    fn snapshot_locals(&self, stack: &LocalStackSnapshot) {
        update_cell(&self.inner.local_stack, |locals| {
            let unchanged = match (&*locals, stack) {
                (Some(current), Some(stack)) => Rc::ptr_eq(current, stack),
                (current, stack) => current.is_none() && stack.is_none(),
            };
            if !unchanged {
                locals.clone_from(stack);
            }
        });
    }

    fn local_stack(&self) -> LocalStackSnapshot {
        read_cell(&self.inner.local_stack, Clone::clone)
    }

    fn set_parent_hint(&self, parent: Option<NodeId>) {
        self.inner.parent_hint.set(parent.unwrap_or(NO_PARENT_HINT));
    }

    /// Ties the scope to the scope its lifetime follows, or to none.
    fn set_owner(&self, kind: OwnerKind, owner: Option<&RecomposeScope>) {
        update_cell(&self.inner.owner, |link| {
            let current = link
                .as_ref()
                .map(|(kind, owner)| (*kind, Weak::as_ptr(owner)));
            if current != owner.map(|owner| (kind, Rc::as_ptr(&owner.inner))) {
                *link = owner.map(|owner| (kind, owner.downgrade()));
                note_scope_activity_change();
            }
        });
    }

    /// Makes `parent` the scope this one composes under, or makes it a root.
    fn set_parent_scope(&self, parent: Option<&RecomposeScope>) {
        self.set_owner(OwnerKind::Parent, parent);
    }

    fn owner_of_kind(&self, kind: OwnerKind) -> Option<RecomposeScope> {
        read_cell(&self.inner.owner, |owner| {
            owner
                .as_ref()
                .filter(|(owner_kind, _)| *owner_kind == kind)
                .and_then(|(_, owner)| RecomposeScope::upgrade(owner))
        })
    }

    fn parent_scope(&self) -> Option<RecomposeScope> {
        self.owner_of_kind(OwnerKind::Parent)
    }

    /// Makes this scope a root whose lifetime follows `owner`, or a plain
    /// root.
    fn set_lifetime_owner_scope(&self, owner: Option<&RecomposeScope>) {
        self.set_owner(OwnerKind::Lifetime, owner);
    }

    #[cfg(test)]
    fn lifetime_owner_scope(&self) -> Option<RecomposeScope> {
        self.owner_of_kind(OwnerKind::Lifetime)
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
        Some(self.inner.parent_hint.get()).filter(|&hint| hint != NO_PARENT_HINT)
    }

    pub(crate) fn group_anchor(&self) -> AnchorId {
        self.inner.group_anchor.get()
    }

    pub(crate) fn set_group_anchor(&self, anchor: AnchorId) {
        self.inner.group_anchor.set(anchor);
    }

    fn set_slots_host(&self, host: &SlotsHost) {
        let storage_key = host.storage_key();
        update_cell(&self.inner.slots_runtime_state, |runtime_state| {
            if self.inner.slots_storage_key.get() == storage_key
                && runtime_state.as_ref().map(std::rc::Weak::as_ptr) == host.runtime_state_ptr()
            {
                return;
            }
            self.inner.slots_storage_key.set(storage_key);
            *runtime_state = host.runtime_state().map(|state| Rc::downgrade(&state));
        });
    }

    pub(crate) fn slots_storage_key(&self) -> Option<usize> {
        let key = self.inner.slots_storage_key.get();
        (key != 0).then_some(key)
    }

    /// What decides the slot host a recomposition of this scope runs in: its
    /// table's storage key and the runtime state that table belongs to.
    pub(crate) fn slots_host_identity(
        &self,
    ) -> (usize, *const crate::composer::ComposerRuntimeState) {
        let state = read_cell(&self.inner.slots_runtime_state, |state| {
            state
                .as_ref()
                .map_or(std::ptr::null(), std::rc::Weak::as_ptr)
        });
        (self.inner.slots_storage_key.get(), state)
    }

    pub(crate) fn slots_runtime_state(&self) -> Option<Rc<crate::composer::ComposerRuntimeState>> {
        read_cell(&self.inner.slots_runtime_state, |state| {
            state.as_ref().and_then(std::rc::Weak::upgrade)
        })
    }

    /// Marks the scope inactive, as when its content is retained or held for reuse. An inactive
    /// scope is not queued for recomposition until it is reactivated.
    pub fn deactivate(&self) {
        if !self.inner.active.replace(false) {
            return;
        }
        note_scope_activity_change();
        self.inner
            .deactivations
            .set(self.inner.deactivations.get() + 1);
        if self.inner.enqueued.replace(false) {
            self.inner.runtime.mark_scope_recomposed();
        }
    }

    pub(crate) fn defer_until_reactivated(&self) {
        if self.inner.enqueued.replace(false) {
            self.inner.runtime.mark_scope_recomposed();
        }
    }

    /// Marks the scope active again and queues it for recomposition if it was invalidated while
    /// inactive.
    pub fn reactivate(&self) {
        self.inner.active.set(true);
        if self.inner.invalid.get()
            && self.is_effectively_active()
            && !self.inner.enqueued.replace(true)
        {
            self.inner.runtime.register_invalid_scope(self.downgrade());
        }
    }

    /// Makes the next `should_recompose` return `false` even if the scope is invalid. Cancels a
    /// pending `force_recompose`.
    pub fn force_reuse(&self) {
        self.inner.force_reuse.set(true);
        self.inner.force_recompose.set(false);
        self.inner.pending_recompose.set(true);
    }

    pub(crate) fn request_pending_recompose(&self) {
        self.inner.pending_recompose.set(true);
    }

    /// Makes the next `should_recompose` return `true` even if the scope is not invalid. Cancels a
    /// pending `force_reuse`.
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

    /// Returns whether the scope's content has to run now: `true` after `force_recompose` or when
    /// the scope is invalid, and `false` after `force_reuse`. A forced answer is used up by the
    /// call.
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

    /// Returns `true` once the scope has composed its content at least once.
    pub fn has_composed_once(&self) -> bool {
        self.inner.composed_once.get()
    }

    fn mark_composed_once(&self) {
        self.inner.composed_once.set(true);
    }

    fn invalidated_only_by(&self, allowed_sources: &HashSet<StateId>) -> Option<bool> {
        if self.inner.unknown_invalidation_source.get() {
            return None;
        }
        read_cell(&self.inner.invalidation_sources, |sources| {
            (!sources.is_empty()).then(|| {
                sources
                    .iter()
                    .all(|source| allowed_sources.contains(source))
            })
        })
    }

    fn has_unknown_invalidation_source(&self) -> bool {
        self.inner.unknown_invalidation_source.get()
    }
}

#[cfg(test)]
impl RecomposeScope {
    pub(crate) fn new_for_test(runtime: RuntimeHandle) -> Self {
        Self::new(runtime)
    }
}

/// Options that a composable group passes to its recompose scope.
#[derive(Debug, Clone, Copy, Default)]
pub struct RecomposeOptions {
    /// Skips the scope's body on the next composition even if it is invalid.
    pub force_reuse: bool,
    /// Runs the scope's body on the next composition even if it is not invalid.
    pub force_recompose: bool,
    /// What happens to the scope's content when it leaves the composition.
    pub retention: RetentionMode,
}

/// An error from an applier or from applying composition commands to nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeError {
    /// No node has this id.
    Missing {
        /// The id that was looked up.
        id: NodeId,
    },
    /// The node's type differs from the type that the caller requested.
    TypeMismatch {
        /// The id of the node.
        id: NodeId,
        /// The name of the type that the caller requested.
        expected: &'static str,
    },
    /// The node lacks the context that an operation needs.
    MissingContext {
        /// The id of the node.
        id: NodeId,
        /// What the node lacks.
        reason: &'static str,
    },
    /// A node with this id exists already.
    AlreadyExists {
        /// The id that is already in use.
        id: NodeId,
    },
    /// A queued command has no payload or the wrong one.
    MalformedCommandPayload {
        /// The label of the command whose payload is wrong.
        tag: &'static str,
    },
    /// A slot host could not be used for `operation`.
    SlotHostUnavailable {
        /// The operation that needed the host.
        operation: &'static str,
        /// Why the host was unavailable.
        reason: &'static str,
    },
    /// Reconciling a composition did not settle within `limit` iterations.
    RecompositionLimitExceeded {
        /// The loop that reached the limit.
        operation: &'static str,
        /// The most iterations allowed.
        limit: usize,
    },
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
            NodeError::MalformedCommandPayload { tag } => {
                write!(f, "command queue missing or invalid {tag} payload")
            }
            NodeError::SlotHostUnavailable { operation, reason } => {
                write!(f, "{operation} cannot access slot host: {reason}")
            }
            NodeError::RecompositionLimitExceeded { operation, limit } => {
                write!(
                    f,
                    "{operation} exceeded {limit} iterations while reconciling composition"
                )
            }
        }
    }
}

impl std::error::Error for NodeError {}

pub use subcompose::{
    ContentTypeReusePolicy, DefaultSlotReusePolicy, SlotId, SlotReusePolicy, SubcomposeState,
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
#[expect(non_snake_case)]
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

fn key_scoped<K: Hash, R>(
    key: &K,
    caller: &'static std::panic::Location<'static>,
    content: impl FnOnce() -> R,
) -> R {
    let seed = explicit_group_key_seed(key, caller);
    with_current_composer(|composer| composer.with_group_seed(seed, |_| content()))
}

/// Runs `content` in a group identified by `key`, so state and nodes follow the key when siblings
/// move. Like `key` for a single key and no return value.
#[track_caller]
pub fn with_key<K: Hash>(key: &K, content: impl FnOnce()) {
    key_scoped(key, std::panic::Location::caller(), content);
}

/// Scopes composition identity to `keys` for the duration of `content`.
///
/// Mirrors Jetpack Compose's `key(vararg keys) { content }`. A call keeps its
/// identity by call-site position alone by default, so a slot survives a
/// recomposition even when what a caller passes in changes; wrapping the
/// call in `key(keys, || ...)` folds `keys` into that identity instead, so
/// changing `keys` discards the previous state and starts fresh, and two
/// `key` calls at the same call site with different `keys` never share
/// state. Pass a tuple to key on more than one value, e.g.
/// `key((a, b), || ...)`.
#[track_caller]
pub fn key<K: Hash, R>(keys: K, content: impl FnOnce() -> R) -> R {
    key_scoped(&keys, std::panic::Location::caller(), content)
}

/// Composes `content` under an identity that is `key` alone, the same at
/// every call site, so the subtree can be emitted from any parent and keep
/// its remembered values, its running effects and its nodes.
///
/// Move content by emitting it under a different parent, in the same pass
/// or a later one: when the old parent stops emitting it, the subtree is
/// retained under `key`; when a parent emits it, the retained subtree is
/// taken back. A site that asks for content still attached elsewhere
/// composes nothing and is recomposed once the content is retained, so the
/// order in which the two parents recompose does not matter.
///
/// Content is shown at most once. Two live sites with the same key leave
/// the later one empty. Retained content is kept
/// until a parent takes it back or [`forget_movable`] releases it, so an
/// item the app closes for good must be forgotten or its state stays in
/// memory. A subtree may cross into and out of a `SubcomposeLayout` slot,
/// whose content lives in a slot table of its own: the table that held it
/// gives up its anchors and the one taking it over issues new ones.
#[track_caller]
pub fn movable<K: Hash>(key: K, content: impl FnOnce()) {
    let id = hash_key(&key);
    with_current_composer(|composer| composer.with_movable_group(id, |_| content()));
}

/// Content written once that keeps its identity wherever it is shown.
///
/// [`movable`] names content by a key, which means writing the content out
/// at every parent that might show it and trusting the keys to match. When
/// the parents are alternatives -- a pane docked in a stack or pulled out
/// into a window of its own, a tab in a strip or torn into its own window --
/// that is the same body written twice, and two bodies that must be kept in
/// step by hand are two bodies that drift.
///
/// This hands back the content as a value instead. Write the body once,
/// remember it, and call [`MovableContent::show`] from whichever parent is
/// showing it this pass:
///
/// ```ignore
/// let pane = rememberMovableContentOf(move || PaneBody(state));
/// if docked {
///     Box(Modifier::empty().offset(0.0, y), BoxSpec::default(), || pane.show());
/// } else {
///     Box(Modifier::empty().window(config), BoxSpec::default(), || pane.show());
/// }
/// ```
///
/// The identity comes from the `remember`, so two of these at one call site
/// -- one per row of a list -- are two pieces of content, and the same one
/// survives recomposition. Everything [`movable`] says about ordering,
/// showing content at most once, and releasing it still holds; release this
/// one with [`MovableContent::forget`].
///
/// Content that has to be recognised from somewhere this value cannot reach
/// -- another composable that shows the same pane in another arrangement --
/// wants [`movableContentOf`] instead, which takes the identity as a key.
///
/// Mirrors Jetpack Compose's `movableContentOf`.
#[expect(non_snake_case)]
#[track_caller]
pub fn rememberMovableContentOf(content: impl Fn() + 'static) -> MovableContent {
    let runtime = with_current_composer(composer::Composer::runtime_handle);
    let id = remember(|| runtime.next_movable_content_id()).with(|id| *id);
    MovableContent {
        id,
        content: Rc::new(content),
    }
}

/// Movable content whose identity is `key`, the same at every call site.
///
/// The keyed twin of [`rememberMovableContentOf`], and the same identity
/// [`movable`] uses: content shown by this value and content shown by
/// `movable` under the same key is one piece of content. Reach for this when
/// the parents that can show it are in different composables, so no single
/// value can be handed to all of them.
#[expect(non_snake_case)]
pub fn movableContentOf<K: Hash>(key: K, content: impl Fn() + 'static) -> MovableContent {
    MovableContent {
        id: hash_key(&key),
        content: Rc::new(content),
    }
}

/// Movable content held as a value. See [`rememberMovableContentOf`].
#[derive(Clone)]
pub struct MovableContent {
    id: Key,
    content: Rc<dyn Fn()>,
}

impl MovableContent {
    /// Shows the content here. Calling this from a different parent than
    /// last pass moves the content, with its remembered values, its running
    /// effects and its nodes.
    pub fn show(&self) {
        let content = Rc::clone(&self.content);
        let id = self.id;
        with_current_composer(|composer| composer.with_movable_group(id, |_| content()));
    }

    /// Releases the content's state once no parent is showing it, for
    /// content the application has closed for good. See [`forget_movable`].
    pub fn forget(&self) {
        forget_movable_id(self.id);
    }
}

impl std::fmt::Debug for MovableContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MovableContent")
            .field("id", &self.id)
            .finish()
    }
}

impl PartialEq for MovableContent {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && Rc::ptr_eq(&self.content, &other.content)
    }
}

/// Releases the state of [`movable`] content with identity `key` that no
/// parent is showing. Call it when the item is closed for good, from an
/// event handler or inside composition. Content a parent is still showing
/// is unaffected. Needs a runtime: an active composition or one created on
/// this thread.
pub fn forget_movable<K: Hash>(key: K) {
    let id = hash_key(&key);
    forget_movable_id(id);
}

fn forget_movable_id(id: Key) {
    let runtime = composer_context::try_with_composer(composer::Composer::runtime_handle)
        .or_else(runtime::current_runtime_handle);
    match runtime {
        Some(runtime) => runtime.forget_movable(id),
        None => log::error!("forget_movable called without an active runtime"),
    }
}

#[derive(Default)]
struct DisposableEffectState {
    key: Option<effect_key::EffectKey>,
    cleanup: Option<Box<dyn FnOnce()>>,
}

impl DisposableEffectState {
    fn should_run<K: PartialEq + 'static>(&self, keys: &K) -> bool {
        self.key.as_ref().is_none_or(|current| !current.holds(keys))
    }

    fn set_key(&mut self, key: effect_key::EffectKey) {
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

/// The value a [`DisposableEffect`] body receives. Its `on_dispose` method makes the result that
/// the body returns.
#[derive(Clone, Copy, Debug, Default)]
pub struct DisposableEffectScope;

/// What a [`DisposableEffect`] body returns: the cleanup to run when the effect is disposed. Make
/// it with `DisposableEffectScope::on_dispose`.
#[derive(Default)]
pub struct DisposableEffectResult {
    cleanup: Option<Box<dyn FnOnce()>>,
}

impl DisposableEffectScope {
    /// Returns a result that runs `cleanup` when the effect leaves the composition or its keys
    /// change.
    pub fn on_dispose(&self, cleanup: impl FnOnce() + 'static) -> DisposableEffectResult {
        DisposableEffectResult::new(cleanup)
    }
}

impl DisposableEffectResult {
    /// Creates a result that runs `cleanup` when the effect is disposed.
    pub fn new(cleanup: impl FnOnce() + 'static) -> Self {
        Self {
            cleanup: Some(Box::new(cleanup)),
        }
    }

    fn into_cleanup(self) -> Option<Box<dyn FnOnce()>> {
        self.cleanup
    }
}

/// Queues `effect` to run each time this call site is composed, after the node changes of the pass
/// are applied. Compose's `SideEffect`.
#[expect(non_snake_case)]
pub fn SideEffect(effect: impl FnOnce() + 'static) {
    with_current_composer(|composer| composer.register_side_effect(effect));
}

/// Implements [`DisposableEffect`] for a call site that has already computed its group key. Call
/// `DisposableEffect` instead.
#[doc(hidden)]
pub fn __disposable_effect_impl<K, F>(group_key: Key, keys: K, effect: F)
where
    K: PartialEq + 'static,
    F: FnOnce(DisposableEffectScope) -> DisposableEffectResult + 'static,
{
    with_current_composer(|composer| {
        let state = composer.remember_effect::<DisposableEffectState>(group_key);
        if state.with(|state| state.should_run(&keys)) {
            let key = effect_key::EffectKey::new(keys);
            state.update(|state| {
                state.run_cleanup();
                state.set_key(key);
            });
            let mut effect_opt = Some(effect);
            composer.register_side_effect(move || {
                if let Some(effect) = effect_opt.take() {
                    let result = effect(DisposableEffectScope);
                    state.update(|state| state.set_cleanup(result.into_cleanup()));
                }
            });
        }
    });
}

/// Runs `effect` when this call site first enters composition, and again
/// whenever `keys` no longer equals the value it ran with last time; the
/// previous run's [`DisposableEffectScope::on_dispose`] cleanup, if any,
/// runs first, and also when the call site leaves composition entirely.
///
/// `keys` may be a tuple to depend on more than one value, matching Jetpack
/// Compose's `DisposableEffect(vararg keys)`.
#[expect(non_snake_case)]
#[track_caller]
pub fn DisposableEffect<K, F>(keys: K, effect: F)
where
    K: PartialEq + 'static,
    F: FnOnce(DisposableEffectScope) -> DisposableEffectResult + 'static,
{
    __disposable_effect_impl(crate::caller_location_key(), keys, effect);
}

/// Clones values into new bindings and evaluates a body with those bindings in scope, which saves
/// writing out the clones before a `move` closure. Each alias before the semicolon is either a
/// name, which clones the variable of that name, or `name = expression`, which clones the value of
/// the expression. The body follows the semicolon.
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

/// Runs `f` on the node `id` as an `N`, using the current composer. Returns an error if the node
/// does not exist or is not an `N`. Must be called during composition.
pub fn with_node_mut<N: Node + 'static, R>(
    id: NodeId,
    f: impl FnOnce(&mut N) -> R,
) -> Result<R, NodeError> {
    with_current_composer(|composer| composer.with_node_mut(id, f))
}

/// Makes the node `id` the parent of the nodes emitted next, using the current composer. Needs a
/// matching `pop_parent`. Must be called during composition.
pub fn push_parent(id: NodeId) {
    with_current_composer(|composer| composer.push_parent(id));
}

/// Closes the parent that the matching `push_parent` opened, using the current composer. Must be
/// called during composition.
pub fn pop_parent() {
    with_current_composer(composer::Composer::pop_parent);
}

/// A node of the tree that a composition builds. An applier stores the nodes by id. Every method
/// has a default, so a simple node implements none of them.
pub trait Node: Any {
    /// Called once the node has its id, when the composition applies the node's creation.
    fn mount(&mut self) {}
    /// Called just before the node is removed from the applier.
    fn unmount(&mut self) {}
    /// Adds `child` to this node's child list, returning whether the list
    /// actually changed. A node that already holds the child returns `false`:
    /// callers record a structural change from this answer, and a structural
    /// change re-lowers the whole subtree, so claiming one for a no-op costs a
    /// full rebuild per call.
    fn insert_child(&mut self, _child: NodeId) -> bool {
        false
    }
    /// Removes `child` from this node's child list, returning whether the list
    /// actually changed. Only this node can answer that: a caller inspecting
    /// the child's parent pointer gets it wrong when the child was reparented
    /// while still listed here.
    fn remove_child(&mut self, _child: NodeId) -> bool {
        false
    }
    /// Moves the child at the first index to the second index in the child list.
    fn move_child(&mut self, _from: usize, _to: usize) {}
    /// Replaces the child list with `children`, in order.
    fn update_children(&mut self, _children: &[NodeId]) {}
    /// Copies child IDs into the provided scratch buffer without allocating a
    /// fresh container for every traversal. A node without children leaves
    /// the buffer empty.
    fn collect_children_into(&self, out: &mut SmallVec<[NodeId; 8]>) {
        out.clear();
    }
    /// Copies the ids of the children this node owns, in order, into `out`. A node whose
    /// `collect_children_into` leaves some children out overrides this to list them all. The
    /// default calls `collect_children_into`.
    fn collect_owned_children_into(&self, out: &mut SmallVec<[NodeId; 8]>) {
        self.collect_children_into(out);
    }
    /// Finds a child in the order returned by [`Node::collect_owned_children_into`].
    /// Nodes with slice-backed child storage should override this to avoid
    /// materializing the whole list for a single lookup.
    ///
    /// ```
    /// use cranpose_core::{Node, NodeId};
    ///
    /// fn child_is_first(parent: &dyn Node, child: NodeId) -> bool {
    ///     parent.owned_child_index(child) == Some(0)
    /// }
    /// ```
    fn owned_child_index(&self, child: NodeId) -> Option<usize> {
        let mut children = SmallVec::<[NodeId; 8]>::new();
        self.collect_owned_children_into(&mut children);
        children.iter().position(|&id| id == child)
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
    /// Mark that a node below this one needs layout, or measure when
    /// `measure` is set: a layout pass has to visit this node to reach it,
    /// while this node's own measurement may still hold. A node that does not
    /// track the difference marks itself dirty.
    fn mark_descendant_needs_layout(&self, measure: bool) {
        if measure {
            self.mark_needs_measure();
        } else {
            self.mark_needs_layout();
        }
    }
    /// Check if a node below this one needs layout or measure.
    fn descendant_needs_layout(&self) -> bool {
        false
    }
    /// Check if a node below this one needs measure.
    fn descendant_needs_measure(&self) -> bool {
        false
    }
    /// Check if a layout pass has to visit this node: the node or a node
    /// below it needs layout or measure.
    fn layout_dirty(&self) -> bool {
        self.needs_measure() || self.needs_layout() || self.descendant_needs_layout()
    }
    /// Whether the node lays out nothing itself and its parent lays out its
    /// children, so a change of its own is a change of its parent's.
    fn is_virtual(&self) -> bool {
        false
    }
    /// Mark this node as needing semantics recomputation.
    fn mark_needs_semantics(&self) {}
    /// Mark that the semantics tree has to read this node again while its
    /// own semantics are unchanged: a node below it changed its semantics,
    /// or its own placement or children changed.
    fn mark_descendant_needs_semantics(&self) {
        self.mark_needs_semantics();
    }
    /// Check if this node or a node below it needs semantics recomputation.
    fn needs_semantics(&self) -> bool {
        false
    }
    /// Set parent reference for dirty flag bubbling ONLY.
    /// This is a minimal version of on_attached_to_parent that doesn't trigger
    /// registry updates or other side effects. Used during measurement when we
    /// need to establish parent connections for bubble_measure_dirty without
    /// causing the full attachment lifecycle.
    ///
    /// An existing parent is preserved until the structural attach operation
    /// removes the node from its previous container.
    fn set_parent_for_bubbling(&mut self, parent: NodeId) {
        if self.parent().is_none() {
            self.on_attached_to_parent(parent);
        }
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
/// 3. Stops at the root (node with no parent)
///
/// # Performance
/// This function is O(height): a scoped layout repass can leave a dirty node
/// under a clean ancestor, so the walk does not stop at a dirty ancestor.
///
/// # Usage
/// - Call from composer mutations (insert/remove/move) during apply phase
/// - Call from applier-level operations that modify the tree structure
pub fn bubble_layout_dirty(applier: &mut dyn Applier, node_id: NodeId) {
    DirtyBubble::LAYOUT.apply(applier, node_id);
}

/// Unified API for bubbling measure dirty flags from a node to the root (Applier context).
///
/// Call this when a node's size may have changed (children added/removed, modifier changed).
/// This ensures that measure_layout will increment the cache epoch and re-measure the subtree.
///
/// # Behavior
/// 1. Marks the starting node as needing measure
/// 2. Walks up the parent chain, marking each ancestor
/// 3. Stops at the root (node with no parent)
pub fn bubble_measure_dirty(applier: &mut dyn Applier, node_id: NodeId) {
    DirtyBubble::MEASURE.apply(applier, node_id);
}

/// Unified API for bubbling semantics dirty flags from a node to the root (Applier context).
///
/// This mirrors [`bubble_layout_dirty`] but toggles semantics-specific dirty
/// flags instead of layout ones, allowing semantics updates to propagate during
/// the apply phase without forcing layout work.
pub fn bubble_semantics_dirty(applier: &mut dyn Applier, node_id: NodeId) {
    DirtyBubble::SEMANTICS.apply(applier, node_id);
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
/// 3. Stops at the root (node with no parent)
///
/// # Performance
/// This function is O(height): a scoped layout repass can leave a dirty node
/// under a clean ancestor, so the walk does not stop at a dirty ancestor.
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

fn bubble_layout_dirty_composer<N: Node + 'static>(mut node_id: NodeId) {
    let _ = with_node_mut(node_id, |node: &mut N| {
        node.mark_needs_layout();
    });
    with_current_composer(|composer| composer.borrow_applier().note_layout_dirt(node_id));

    while let Ok(Some(pid)) = with_node_mut(node_id, |node: &mut N| node.parent()) {
        let parent_id = pid;

        let advanced = with_node_mut(parent_id, |node: &mut N| {
            node.mark_descendant_needs_layout(false);
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

fn bubble_semantics_dirty_composer<N: Node + 'static>(mut node_id: NodeId) {
    let _ = with_node_mut(node_id, |node: &mut N| {
        node.mark_needs_semantics();
    });

    while let Ok(Some(pid)) = with_node_mut(node_id, |node: &mut N| node.parent()) {
        let parent_id = pid;

        let advanced = with_node_mut(parent_id, |node: &mut N| {
            if !node.needs_semantics() {
                node.mark_descendant_needs_semantics();
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
    /// Returns the node as `Any`, for downcasting to its concrete type.
    pub fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A removed node kept in a pool with its old id, so that a later node of the same type can reuse
/// it.
pub struct RecycledNode {
    stable_id: NodeId,
    node: Box<dyn Node>,
    warm_origin: bool,
}

impl RecycledNode {
    /// A removed node as a pool shell: the fresh box it rehouses into, or
    /// the node itself cleared in place when it offers none. A rehoused
    /// node is dropped as it is, so it is not cleared first.
    fn new(stable_id: NodeId, mut node: Box<dyn Node>, warm_origin: bool) -> Self {
        let node = node.rehouse_for_recycle().unwrap_or_else(|| {
            node.prepare_for_recycle();
            node
        });
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

    /// Returns the id the node had before it was recycled.
    pub fn stable_id(&self) -> NodeId {
        self.stable_id
    }

    fn warm_origin(&self) -> bool {
        self.warm_origin
    }

    fn set_warm_origin(&mut self, warm_origin: bool) {
        self.warm_origin = warm_origin;
    }

    /// Returns the pooled node, so that it can be reset before reuse.
    pub fn node_mut(&mut self) -> &mut dyn Node {
        self.node.as_mut()
    }

    /// Splits the shell into its stable id and its node, with a flag that tells whether it came
    /// from the warm recycle path.
    pub fn into_parts(self) -> (NodeId, Box<dyn Node>, bool) {
        (self.stable_id, self.node, self.warm_origin)
    }
}

/// The result of putting a recycled node back into an applier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecycledNodeInsertion {
    /// The id the node has now.
    pub id: NodeId,
    /// Whether the node got its old stable id back.
    pub stable_id_reused: bool,
    /// The error that forced a new id, or `None` if the old id was reused.
    pub fallback_error: Option<NodeError>,
}

impl RecycledNodeInsertion {
    fn reused(stable_id: NodeId) -> Self {
        Self {
            id: stable_id,
            stable_id_reused: true,
            fallback_error: None,
        }
    }

    fn fresh(id: NodeId, fallback_error: Option<NodeError>) -> Self {
        Self {
            id,
            stable_id_reused: false,
            fallback_error,
        }
    }
}

/// Stores the nodes that a composition builds and applies changes to them. The UI layer supplies an
/// applier; [`MemoryApplier`] is the stock one.
pub trait Applier: Any {
    /// Stores `node` and returns its new id.
    fn create(&mut self, node: Box<dyn Node>) -> NodeId;
    /// Returns the node `id` for change, or an error if there is none.
    fn get_mut(&mut self, id: NodeId) -> Result<&mut dyn Node, NodeError>;
    /// Removes the node `id` and the nodes below it.
    fn remove(&mut self, id: NodeId) -> Result<(), NodeError>;

    /// Opens a batch of commands, in which no layout pass runs: within it a
    /// dirty bubble may stop at an ancestor an earlier one passed
    /// ([`Self::note_bubble_pass`]). Batches nest.
    fn begin_bubble_batch(&mut self) {}

    /// Notes that a dirty bubble marked `id` as needing measure or layout,
    /// or carried the dirt of `id` to its ancestors, for a layout pass that
    /// starts from the dirty nodes. An applier that keeps no notes ignores
    /// it.
    fn note_layout_dirt(&mut self, _id: NodeId) {}

    /// Closes the batch [`Self::begin_bubble_batch`] opened.
    fn end_bubble_batch(&mut self) {}

    /// Forgets what the open batch's bubbles passed: a node changed its
    /// parent, or other code ran, so their marks may no longer reach the root.
    fn forget_bubble_passes(&mut self) {}

    /// Notes that a dirty bubble carrying `flags` passed the ancestor `id`.
    /// `true` when one carrying all of them passed it earlier in the open
    /// batch: that one marked every ancestor above `id`, so this one may
    /// stop. An applier that keeps no notes answers `false`.
    fn note_bubble_pass(&mut self, _id: NodeId, _flags: u8) -> bool {
        false
    }

    /// The node `id` for a dirty bubble to mark, or `None` when it is
    /// missing or, with `note`, when [`Self::note_bubble_pass`] says a
    /// bubble carrying `flags` passed it earlier in the open batch.
    fn bubble_node(&mut self, id: NodeId, flags: u8, note: bool) -> Option<&mut dyn Node> {
        if note && self.note_bubble_pass(id, flags) {
            return None;
        }
        self.get_mut(id).ok()
    }

    /// Records that `parent_id`'s child list changed structurally this frame
    /// (insert, remove, move, or reparent). Incremental scene consumers drain
    /// the recorded parents and re-patch those subtrees so removed nodes are
    /// evicted from a persistent render graph even when the frame's scene
    /// update is otherwise scoped to unrelated dirty nodes.
    fn record_structural_change(&mut self, _parent_id: NodeId) {}

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

    /// Reinserts a recycled node at its retained stable ID, or creates a fresh ID if that
    /// retained ID is no longer available.
    fn insert_recycled_node_or_create(
        &mut self,
        stable_id: NodeId,
        node: Box<dyn Node>,
    ) -> RecycledNodeInsertion {
        let id = self.create(node);
        RecycledNodeInsertion::fresh(id, Some(NodeError::AlreadyExists { id: stable_id }))
    }

    /// Returns the applier as `Any`, for downcasting to its concrete type.
    fn as_any(&self) -> &dyn Any
    where
        Self: Sized,
    {
        self
    }

    /// Returns the applier as `Any`, for downcasting to its concrete type.
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
    /// its parent.
    fn walk(self, applier: &mut dyn Applier, first: Option<NodeId>, mut start: bool) {
        let flags =
            u8::from(self.layout) | u8::from(self.measure) << 1 | u8::from(self.semantics) << 2;
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
            .saturating_mul(std::mem::size_of::<BubbleDirtyCommand>())
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
    high_id_nodes: HashMap<NodeId, Box<dyn Node>>,
    high_id_warm_recycled_origins: HashMap<NodeId, bool>,
    high_id_generations: HashMap<NodeId, u32>,
    next_stable_id: NodeId,
    layout_runtime: Option<RuntimeHandle>,
    slots: Rc<RefCell<SlotTable>>,
    recycled_nodes: HashMap<TypeId, Vec<RecycledNode>>,
    returning_recycled_nodes: HashMap<TypeId, Vec<RecycledNode>>,
    cold_recycled_nodes: HashMap<TypeId, Vec<RecycledNode>>,
    recycled_node_limits: HashMap<TypeId, usize>,
    warm_recycled_node_targets: HashMap<TypeId, usize>,
    fresh_recyclable_creations: HashMap<TypeId, usize>,
    recycled_node_prototypes: HashMap<TypeId, Box<dyn Node>>,
    structural_change_parents: Vec<NodeId>,
    virtual_node_ids: HashSet<NodeId>,
    bubble_passes: bubble_passes::BubblePasses,
    layout_dirt_roots: layout_dirt_roots::LayoutDirtRoots,
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

    /// The slot table that composition during layout writes to, shared with
    /// the layout pass that lends this applier.
    pub fn slots_handle(&self) -> Rc<RefCell<SlotTable>> {
        Rc::clone(&self.state.slots)
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
        self.state.recycled_nodes.get(&key).map_or(0, Vec::len)
            + self
                .state
                .returning_recycled_nodes
                .get(&key)
                .map_or(0, Vec::len)
            + self.state.cold_recycled_nodes.get(&key).map_or(0, Vec::len)
    }

    /// Returns the sizes of the applier's tables, for leak and memory checks.
    pub fn debug_stats(&self) -> MemoryApplierDebugStats {
        let mut recycled_keys: HashSet<TypeId> = HashSet::default();
        recycled_keys.extend(self.state.recycled_nodes.keys().copied());
        recycled_keys.extend(self.state.returning_recycled_nodes.keys().copied());
        recycled_keys.extend(self.state.cold_recycled_nodes.keys().copied());

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
            high_id_nodes_len: self.state.high_id_nodes.len(),
            high_id_nodes_cap: self.state.high_id_nodes.capacity(),
            high_id_generations_len: self.state.high_id_generations.len(),
            high_id_generations_cap: self.state.high_id_generations.capacity(),
            recycled_type_count: recycled_keys.len(),
            recycled_type_cap: self.state.recycled_nodes.capacity()
                + self.state.returning_recycled_nodes.capacity()
                + self.state.cold_recycled_nodes.capacity(),
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
        for node in self.state.high_id_nodes.values_mut() {
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
            .map(|node| std::mem::size_of_val(&**node) + node.debug_heap_bytes())
            .sum::<usize>();
        let high_id_nodes = self
            .state
            .high_id_nodes
            .values()
            .map(|node| std::mem::size_of_val(&**node) + node.debug_heap_bytes())
            .sum::<usize>();
        dense_nodes + high_id_nodes
    }

    /// Returns the heap bytes that the pooled recycled nodes take, including what they own.
    pub fn debug_recycled_node_heap_bytes(&self) -> usize {
        let pool_bytes = |pools: &HashMap<TypeId, Vec<RecycledNode>>| {
            pools
                .values()
                .flat_map(|nodes| nodes.iter())
                .map(|node| std::mem::size_of_val(&*node.node) + node.node.debug_heap_bytes())
                .sum::<usize>()
        };

        pool_bytes(&self.state.recycled_nodes)
            + pool_bytes(&self.state.returning_recycled_nodes)
            + pool_bytes(&self.state.cold_recycled_nodes)
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

    fn pool_node_count(pools: &HashMap<TypeId, Vec<RecycledNode>>) -> usize {
        pools.values().map(Vec::len).sum()
    }

    fn pool_node_capacity(pools: &HashMap<TypeId, Vec<RecycledNode>>) -> usize {
        pools.values().map(Vec::capacity).sum()
    }

    fn total_recycled_node_count(&self) -> usize {
        Self::pool_node_count(&self.state.recycled_nodes)
            + Self::pool_node_count(&self.state.returning_recycled_nodes)
            + Self::pool_node_count(&self.state.cold_recycled_nodes)
    }

    fn total_recycled_node_capacity(&self) -> usize {
        Self::pool_node_capacity(&self.state.recycled_nodes)
            + Self::pool_node_capacity(&self.state.returning_recycled_nodes)
            + Self::pool_node_capacity(&self.state.cold_recycled_nodes)
    }

    fn total_warm_recycled_node_id_count(&self) -> usize {
        self.live_warm_recycled_origin_count()
            + Self::pool_node_count(&self.state.recycled_nodes)
            + Self::pool_node_count(&self.state.returning_recycled_nodes)
    }

    fn total_warm_recycled_node_id_capacity(&self) -> usize {
        self.live_warm_recycled_origin_capacity()
            + Self::pool_node_capacity(&self.state.recycled_nodes)
            + Self::pool_node_capacity(&self.state.returning_recycled_nodes)
    }

    fn remember_recycle_pool_limit(&mut self, key: TypeId, recycle_pool_limit: Option<usize>) {
        if let Some(limit) = recycle_pool_limit {
            self.state.recycled_node_limits.insert(key, limit);
        } else {
            self.state.recycled_node_limits.remove(&key);
        }
    }

    fn recycle_pool_limit_for(&self, key: TypeId) -> Option<usize> {
        self.state.recycled_node_limits.get(&key).copied()
    }

    fn warm_recycled_pool_len(&self, key: TypeId) -> usize {
        self.state.recycled_nodes.get(&key).map_or(0, Vec::len)
    }

    fn warm_recycled_node_target(&self, key: TypeId) -> usize {
        self.state
            .warm_recycled_node_targets
            .get(&key)
            .copied()
            .unwrap_or(0)
    }

    fn warm_recycled_node_target_limit(&self, key: TypeId) -> usize {
        let Some(limit) = self.recycle_pool_limit_for(key) else {
            return usize::MAX;
        };
        if limit <= 8 { limit } else { limit / 4 }
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
        self.state.warm_recycled_node_targets.insert(key, target);
        target
    }

    fn remember_recycled_node_prototype(&mut self, key: TypeId, shell: &dyn Node) {
        if self.state.recycled_node_prototypes.contains_key(&key) {
            return;
        }
        if let Some(prototype) = shell.rehouse_for_recycle() {
            self.state.recycled_node_prototypes.insert(key, prototype);
        }
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
                .high_id_warm_recycled_origins
                .values()
                .filter(|warm_origin| **warm_origin)
                .count()
    }

    fn live_warm_recycled_origin_capacity(&self) -> usize {
        self.state.physical_warm_recycled_origins.capacity()
            + self.state.high_id_warm_recycled_origins.capacity()
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
            self.state.returning_recycled_nodes.entry(key).or_default()
        } else {
            self.state.cold_recycled_nodes.entry(key).or_default()
        };
        pool.push(recycled);
        if let Some(limit) = recycle_pool_limit
            && pool.len() > limit
        {
            let excess = pool.len() - limit;
            let dropped: Vec<_> = pool.drain(0..excess).collect();
            drop(dropped);
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
            let pool = self.state.recycled_nodes.entry(key).or_default();
            pool.push(recycled);
            if let Some(limit) = recycle_pool_limit
                && pool.len() > limit
            {
                let excess = pool.len() - limit;
                dropped = pool.drain(0..excess).collect();
                remove_pool_entry = pool.is_empty();
            }
        }
        if remove_pool_entry {
            self.state.recycled_nodes.remove(&key);
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
        let stable_id = self.state.next_stable_id;
        self.state.next_stable_id = self.state.next_stable_id.saturating_add(1);
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
        let Some(pool) = self.state.recycled_nodes.get_mut(&key) else {
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
            self.state.recycled_nodes.remove(&key);
        }
    }

    fn trim_idle_warm_pool_to_target(&mut self, key: TypeId, target: usize) {
        let pool_len = self.warm_recycled_pool_len(key);
        if pool_len <= target {
            return;
        }

        let Some(pool) = self.state.recycled_nodes.get_mut(&key) else {
            return;
        };
        let removable = (pool_len - target).min(pool.len());
        let dropped: Vec<_> = pool.drain(0..removable).collect();
        let remove_pool_entry = pool.is_empty();
        let _ = pool;

        if remove_pool_entry {
            self.state.recycled_nodes.remove(&key);
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
        if let Some(prototype) = self.state.recycled_node_prototypes.get(&key) {
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
        let retained_len = self.state.stable_index.live() + self.total_recycled_node_count();
        if retained_len == self.state.stable_index.occupied() {
            return;
        }

        let recycled: HashSet<NodeId> = [
            &self.state.recycled_nodes,
            &self.state.returning_recycled_nodes,
            &self.state.cold_recycled_nodes,
        ]
        .into_iter()
        .flat_map(|pools| pools.values().flatten().map(RecycledNode::stable_id))
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

    fn resolve_node_index(&self, id: NodeId) -> Option<usize> {
        self.state.stable_index.slot(id)
    }

    fn contains_node_id(&self, id: NodeId) -> bool {
        self.resolve_node_index(id).is_some() || self.state.high_id_nodes.contains_key(&id)
    }

    fn insert_high_id_node(&mut self, stable_id: NodeId, node: Box<dyn Node>, warm_origin: bool) {
        self.state.high_id_nodes.insert(stable_id, node);
        self.state
            .high_id_warm_recycled_origins
            .insert(stable_id, warm_origin);
        self.state.high_id_generations.entry(stable_id).or_insert(0);
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
    pub fn get_ref(&self, id: NodeId) -> Result<&dyn Node, NodeError> {
        if let Some(physical_id) = self.resolve_node_index(id) {
            let slot = self
                .state
                .nodes
                .get(physical_id)
                .ok_or(NodeError::Missing { id })?
                .as_deref()
                .ok_or(NodeError::Missing { id })?;
            return Ok(slot);
        }

        self.state
            .high_id_nodes
            .get(&id)
            .map(AsRef::as_ref)
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
        if self.state.high_id_nodes.contains_key(&node_id) {
            if let Some(node) = self.state.high_id_nodes.remove(&node_id)
                && let Some(key) = node.recycle_key()
            {
                let recycle_pool_limit = node.recycle_pool_limit();
                let warm_origin = self
                    .state
                    .high_id_warm_recycled_origins
                    .remove(&node_id)
                    .unwrap_or(false);
                self.push_recycled_node(
                    key,
                    recycle_pool_limit,
                    RecycledNode::new(node_id, node, warm_origin),
                );
            }
            let generation = self.state.high_id_generations.entry(node_id).or_insert(0);
            *generation = generation.wrapping_add(1);
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
                .high_id_generations
                .get(&id)
                .copied()
                .unwrap_or(0);
        }
        self.state.stable_index.generation(id)
    }

    fn get_mut(&mut self, id: NodeId) -> Result<&mut dyn Node, NodeError> {
        if let Some(physical_id) = self.resolve_node_index(id) {
            let slot = self
                .state
                .nodes
                .get_mut(physical_id)
                .ok_or(NodeError::Missing { id })?
                .as_deref_mut()
                .ok_or(NodeError::Missing { id })?;
            return Ok(slot);
        }
        self.state
            .high_id_nodes
            .get_mut(&id)
            .map(std::convert::AsMut::as_mut)
            .ok_or(NodeError::Missing { id })
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
                .high_id_nodes
                .get_mut(&id)
                .map(std::convert::AsMut::as_mut);
        };
        if note && self.state.bubble_passes.note(index, flags) {
            return None;
        }
        self.state.nodes.get_mut(index)?.as_deref_mut()
    }

    fn insert_with_id(&mut self, id: NodeId, node: Box<dyn Node>) -> Result<(), NodeError> {
        if self.contains_node_id(id) {
            return Err(NodeError::AlreadyExists { id });
        }
        self.insert_available_with_id(id, node);
        self.state.virtual_node_ids.insert(id);
        Ok(())
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
        Self::take_recycled_node_from_pool(&mut self.state.returning_recycled_nodes, key)
            .or_else(|| Self::take_recycled_node_from_pool(&mut self.state.recycled_nodes, key))
    }

    fn set_recycled_node_origin(&mut self, id: NodeId, warm_origin: bool) {
        if let Some(physical_id) = self.resolve_node_index(id) {
            self.state.physical_warm_recycled_origins[physical_id] = warm_origin;
        } else if self.state.high_id_nodes.contains_key(&id) {
            self.state
                .high_id_warm_recycled_origins
                .insert(id, warm_origin);
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
        *self
            .state
            .fresh_recyclable_creations
            .entry(key)
            .or_insert(0) += 1;
    }

    fn clear_recycled_nodes(&mut self) {
        let returning = std::mem::take(&mut self.state.returning_recycled_nodes);
        for (key, mut nodes) in returning {
            let pool = self.state.recycled_nodes.entry(key).or_default();
            pool.append(&mut nodes);
        }

        let fresh_recyclable_creations = std::mem::take(&mut self.state.fresh_recyclable_creations);
        let cold = std::mem::take(&mut self.state.cold_recycled_nodes);
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
                    self.state
                        .recycled_nodes
                        .entry(key)
                        .or_default()
                        .push(recycled);
                }
            }
        }

        let mut keys: HashSet<TypeId> = HashSet::default();
        keys.extend(self.state.recycled_nodes.keys().copied());
        keys.extend(self.state.recycled_node_limits.keys().copied());
        keys.extend(self.state.warm_recycled_node_targets.keys().copied());
        keys.extend(self.state.recycled_node_prototypes.keys().copied());
        for key in keys {
            let observed_demand = fresh_recyclable_creations.get(&key).copied().unwrap_or(0);
            let target = self.update_warm_recycled_node_target(key, observed_demand);
            self.replenish_warm_pool_to_target(key, target);
            self.trim_idle_warm_pool_to_target(key, target);
            self.compact_idle_warm_pool(key);
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
    nested_hosts: Vec<std::rc::Weak<SlotsHost>>,
    lifecycle: slot::SlotLifecycleCoordinator,
    runtime_state: Option<Rc<crate::composer::ComposerRuntimeState>>,
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
    pub fn note_nested_host(&self, nested: &Rc<SlotsHost>) {
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
            let nested: Vec<Rc<SlotsHost>> = inner
                .nested_hosts
                .iter()
                .filter_map(std::rc::Weak::upgrade)
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

    pub(crate) fn bind_runtime_state(&self, state: &Rc<crate::composer::ComposerRuntimeState>) {
        let mut inner = self.inner.borrow_mut();
        inner.runtime_state = Some(Rc::clone(state));
    }

    pub(crate) fn rebind_orphaned_runtime_state(
        &self,
        state: &Rc<crate::composer::ComposerRuntimeState>,
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

    pub(crate) fn runtime_state(&self) -> Option<Rc<crate::composer::ComposerRuntimeState>> {
        self.inner.borrow().runtime_state.clone()
    }

    fn runtime_state_ptr(&self) -> Option<*const crate::composer::ComposerRuntimeState> {
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
    pub(crate) fn debug_snapshot(&self) -> slot::SlotDebugSnapshot {
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

pub(crate) fn note_structural(reason: &str, parent_id: NodeId, child_id: NodeId) {
    if env_flag!("CRANPOSE_STRUCTURAL_DIAG") {
        eprintln!("[structural] {reason} parent={parent_id} child={child_id}");
    }
}

pub(crate) fn note_structural_move(parent_id: NodeId, from_index: usize, to_index: usize) {
    if env_flag!("CRANPOSE_STRUCTURAL_DIAG") {
        eprintln!("[structural] move parent={parent_id} from={from_index} to={to_index}");
    }
}
