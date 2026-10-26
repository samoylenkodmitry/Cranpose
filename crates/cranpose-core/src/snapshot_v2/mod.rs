//! Snapshot system for managing isolated state changes.
//!
//! This module implements Jetpack Compose's snapshot isolation system, allowing
//! state changes to be isolated, composed, and atomically applied.
//!
//! # Snapshot Types
//!
//! - **ReadonlySnapshot**: Immutable view of state at a point in time
//! - **MutableSnapshot**: Allows isolated state mutations
//! - **NestedReadonlySnapshot**: Readonly snapshot nested in a parent
//! - **NestedMutableSnapshot**: Mutable snapshot nested in a parent
//! - **GlobalSnapshot**: Special global mutable snapshot
//! - **TransparentObserverMutableSnapshot**: Optimized for observer chaining
//! - **TransparentObserverSnapshot**: Readonly version of transparent observer
//!
//! # Thread Local Storage
//!
//! The current snapshot is stored in thread-local storage and automatically
//! managed by the snapshot system. Snapshot handles and observers use [`Rc`]
//! and stay on their creating thread. Background work sends results to the UI
//! runtime instead of sharing snapshots across threads.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use crate::{
    collections::map::{HashMap, HashSet},
    snapshot_id_set::{SnapshotId, SnapshotIdSet},
    snapshot_pinning::{self, PinHandle},
    snapshot_weak_set::SnapshotWeakSetDebugStats,
    state::{ObjectId, StateObject, StateRecord},
};

mod global;
mod mutable;
mod nested;
mod readonly;
mod runtime;
mod snapshot_state;
mod transparent;

#[cfg(test)]
#[path = "tests/integration_tests.rs"]
mod integration_tests;

pub use global::{GlobalSnapshot, advance_global_snapshot};
pub use mutable::MutableSnapshot;
pub use nested::{NestedMutableSnapshot, NestedReadonlySnapshot};
pub use readonly::ReadonlySnapshot;
pub(crate) use runtime::{SnapshotRuntime, allocate_snapshot, close_snapshot, with_runtime};
#[cfg(test)]
pub(crate) use runtime::{TestRuntimeGuard, reset_runtime_for_tests};
pub(crate) use snapshot_state::{Pinning, RuntimeTracking, SnapshotState};
pub use transparent::{TransparentObserverMutableSnapshot, TransparentObserverSnapshot};

/// Observer that is called when a state object is read.
pub type ReadObserver = Rc<dyn Fn(&dyn StateObject) + 'static>;

/// Observer that is called when a state object is written.
pub type WriteObserver = Rc<dyn Fn(&dyn StateObject) + 'static>;

/// Apply observer that is called when a snapshot is applied.
pub type ApplyObserver = Rc<dyn Fn(&[Rc<dyn StateObject>], SnapshotId) + 'static>;

/// Result of applying a mutable snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotApplyResult {
    /// The snapshot was applied successfully.
    Success,
    /// The snapshot could not be applied due to conflicts.
    Failure,
}

impl SnapshotApplyResult {
    /// Check if the result is successful.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }

    /// Check if the result is a failure.
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failure)
    }

    /// Panic if the result is a failure (for use in tests).
    #[track_caller]
    pub fn check(&self) {
        assert!(!self.is_failure(), "Snapshot apply failed");
    }
}

/// Enum wrapper for all snapshot types.
///
/// This provides a type-safe way to work with different snapshot types
/// without requiring trait objects, which avoids object-safety issues.
#[derive(Clone)]
pub enum AnySnapshot {
    /// A root read-only snapshot.
    Readonly(Rc<ReadonlySnapshot>),
    /// A root mutable snapshot, applied to the global state.
    Mutable(Rc<MutableSnapshot>),
    /// A read-only snapshot nested in a parent snapshot.
    NestedReadonly(Rc<NestedReadonlySnapshot>),
    /// A mutable snapshot nested in a mutable parent, applied to that parent.
    NestedMutable(Rc<NestedMutableSnapshot>),
    /// The global snapshot of the thread.
    Global(Rc<GlobalSnapshot>),
    /// A mutable snapshot whose observers can be replaced after creation.
    TransparentMutable(Rc<TransparentObserverMutableSnapshot>),
    /// A read-only snapshot whose observer can be replaced after creation.
    TransparentReadonly(Rc<TransparentObserverSnapshot>),
}

/// Enum wrapper for mutable snapshot types.
///
/// This allows `take_mutable_snapshot` to return either a root MutableSnapshot
/// or a NestedMutableSnapshot depending on the current context, matching Kotlin's
/// behavior where `takeMutableSnapshot` creates nested snapshots when inside a
/// mutable snapshot.
#[derive(Clone)]
pub enum AnyMutableSnapshot {
    /// A root mutable snapshot, applied to the global state.
    Root(Rc<MutableSnapshot>),
    /// A mutable snapshot nested in a mutable parent, applied to that parent.
    Nested(Rc<NestedMutableSnapshot>),
}

impl AnyMutableSnapshot {
    /// Get the snapshot ID.
    pub fn snapshot_id(&self) -> SnapshotId {
        match self {
            Self::Root(s) => s.snapshot_id(),
            Self::Nested(s) => s.snapshot_id(),
        }
    }

    /// Get the set of invalid snapshot IDs.
    pub fn invalid(&self) -> SnapshotIdSet {
        match self {
            Self::Root(s) => s.invalid(),
            Self::Nested(s) => s.invalid(),
        }
    }

    /// Enter this snapshot, making it current for the duration of the closure.
    pub fn enter<T>(&self, f: impl FnOnce() -> T) -> T {
        match self {
            Self::Root(s) => s.enter(f),
            Self::Nested(s) => s.enter(f),
        }
    }

    /// Apply the snapshot.
    pub fn apply(&self) -> SnapshotApplyResult {
        match self {
            Self::Root(s) => s.apply(),
            Self::Nested(s) => s.apply(),
        }
    }

    /// Dispose the snapshot.
    pub fn dispose(&self) {
        match self {
            Self::Root(s) => s.dispose(),
            Self::Nested(s) => s.dispose(),
        }
    }
}

impl AnySnapshot {
    /// Get the snapshot ID.
    pub fn snapshot_id(&self) -> SnapshotId {
        match self {
            Self::Readonly(s) => s.snapshot_id(),
            Self::Mutable(s) => s.snapshot_id(),
            Self::NestedReadonly(s) => s.snapshot_id(),
            Self::NestedMutable(s) => s.snapshot_id(),
            Self::Global(s) => s.snapshot_id(),
            Self::TransparentMutable(s) => s.snapshot_id(),
            Self::TransparentReadonly(s) => s.snapshot_id(),
        }
    }

    /// The state every snapshot kind shares: id, invalid set, observers and flags.
    fn state(&self) -> &SnapshotState {
        match self {
            Self::Readonly(s) => &s.state,
            Self::Mutable(s) => &s.state,
            Self::NestedReadonly(s) => &s.state,
            Self::NestedMutable(s) => &s.state,
            Self::Global(s) => &s.state,
            Self::TransparentMutable(s) => &s.state,
            Self::TransparentReadonly(s) => &s.state,
        }
    }

    pub(crate) fn with_invalid<R>(&self, f: impl FnOnce(&SnapshotIdSet) -> R) -> R {
        f(&self.state().invalid.borrow())
    }

    /// Returns the ids of snapshots whose writes this snapshot does not see.
    pub fn invalid(&self) -> SnapshotIdSet {
        match self {
            Self::Readonly(s) => s.invalid(),
            Self::Mutable(s) => s.invalid(),
            Self::NestedReadonly(s) => s.invalid(),
            Self::NestedMutable(s) => s.invalid(),
            Self::Global(s) => s.invalid(),
            Self::TransparentMutable(s) => s.invalid(),
            Self::TransparentReadonly(s) => s.invalid(),
        }
    }

    /// Check if a snapshot ID is valid in this snapshot.
    pub fn is_valid(&self, id: SnapshotId) -> bool {
        let snapshot_id = self.snapshot_id();
        id <= snapshot_id && !self.invalid().get(id)
    }

    /// Check if this is a read-only snapshot.
    pub fn read_only(&self) -> bool {
        match self {
            Self::Readonly(_) => true,
            Self::Mutable(_) => false,
            Self::NestedReadonly(_) => true,
            Self::NestedMutable(_) => false,
            Self::Global(_) => false,
            Self::TransparentMutable(_) => false,
            Self::TransparentReadonly(_) => true,
        }
    }

    /// Get the root snapshot.
    pub fn root(&self) -> Self {
        match self {
            Self::Readonly(s) => Self::Readonly(s.root_readonly()),
            Self::Mutable(s) => Self::Mutable(s.root_mutable()),
            Self::NestedReadonly(s) => Self::NestedReadonly(s.root_nested_readonly()),
            Self::NestedMutable(s) => Self::Mutable(s.root_mutable()),
            Self::Global(_) => Self::Global(GlobalSnapshot::get_or_create()),
            Self::TransparentMutable(s) => Self::TransparentMutable(s.root_transparent_mutable()),
            Self::TransparentReadonly(s) => {
                Self::TransparentReadonly(s.root_transparent_readonly())
            }
        }
    }

    /// Check if this snapshot refers to the same transparent snapshot.
    pub fn is_same_transparent(&self, other: &Rc<TransparentObserverMutableSnapshot>) -> bool {
        matches!(self, Self::TransparentMutable(snapshot) if Rc::ptr_eq(snapshot, other))
    }

    /// Check if this snapshot refers to the same transparent mutable snapshot.
    pub fn is_same_transparent_mutable(
        &self,
        other: &Rc<TransparentObserverMutableSnapshot>,
    ) -> bool {
        self.is_same_transparent(other)
    }

    /// Check if this snapshot refers to the same transparent readonly snapshot.
    pub fn is_same_transparent_readonly(&self, other: &Rc<TransparentObserverSnapshot>) -> bool {
        matches!(self, Self::TransparentReadonly(snapshot) if Rc::ptr_eq(snapshot, other))
    }

    /// Enter this snapshot, making it current for the duration of the closure.
    pub fn enter<T>(&self, f: impl FnOnce() -> T) -> T {
        match self {
            Self::Readonly(s) => s.enter(f),
            Self::Mutable(s) => s.enter(f),
            Self::NestedReadonly(s) => s.enter(f),
            Self::NestedMutable(s) => s.enter(f),
            Self::Global(s) => s.enter(f),
            Self::TransparentMutable(s) => s.enter(f),
            Self::TransparentReadonly(s) => s.enter(f),
        }
    }

    /// Take a nested read-only snapshot.
    pub fn take_nested_snapshot(&self, read_observer: Option<ReadObserver>) -> Self {
        match self {
            Self::Readonly(s) => Self::Readonly(s.take_nested_snapshot(read_observer)),
            Self::Mutable(s) => Self::Readonly(s.take_nested_snapshot(read_observer)),
            Self::NestedReadonly(s) => Self::NestedReadonly(s.take_nested_snapshot(read_observer)),
            Self::NestedMutable(s) => Self::Readonly(s.take_nested_snapshot(read_observer)),
            Self::Global(s) => Self::Readonly(s.take_nested_snapshot(read_observer)),
            Self::TransparentMutable(s) => Self::Readonly(s.take_nested_snapshot(read_observer)),
            Self::TransparentReadonly(s) => {
                Self::TransparentReadonly(s.take_nested_snapshot(read_observer))
            }
        }
    }

    /// Check if there are pending changes.
    pub fn has_pending_changes(&self) -> bool {
        match self {
            Self::Readonly(s) => s.has_pending_changes(),
            Self::Mutable(s) => s.has_pending_changes(),
            Self::NestedReadonly(s) => s.has_pending_changes(),
            Self::NestedMutable(s) => s.has_pending_changes(),
            Self::Global(s) => s.has_pending_changes(),
            Self::TransparentMutable(s) => s.has_pending_changes(),
            Self::TransparentReadonly(s) => s.has_pending_changes(),
        }
    }

    /// Dispose of this snapshot.
    pub fn dispose(&self) {
        match self {
            Self::Readonly(s) => s.dispose(),
            Self::Mutable(s) => s.dispose(),
            Self::NestedReadonly(s) => s.dispose(),
            Self::NestedMutable(s) => s.dispose(),
            Self::Global(s) => s.dispose(),
            Self::TransparentMutable(s) => s.dispose(),
            Self::TransparentReadonly(s) => s.dispose(),
        }
    }

    /// Check if disposed.
    pub fn is_disposed(&self) -> bool {
        match self {
            Self::Readonly(s) => s.is_disposed(),
            Self::Mutable(s) => s.is_disposed(),
            Self::NestedReadonly(s) => s.is_disposed(),
            Self::NestedMutable(s) => s.is_disposed(),
            Self::Global(s) => s.is_disposed(),
            Self::TransparentMutable(s) => s.is_disposed(),
            Self::TransparentReadonly(s) => s.is_disposed(),
        }
    }

    /// Record a read.
    pub fn record_read(&self, state: &dyn StateObject) {
        match self {
            Self::Readonly(s) => s.record_read(state),
            Self::Mutable(s) => s.record_read(state),
            Self::NestedReadonly(s) => s.record_read(state),
            Self::NestedMutable(s) => s.record_read(state),
            Self::Global(s) => s.record_read(state),
            Self::TransparentMutable(s) => s.record_read(state),
            Self::TransparentReadonly(s) => s.record_read(state),
        }
    }

    /// Record a write.
    pub fn record_write(&self, state: Rc<dyn StateObject>) {
        match self {
            Self::Readonly(s) => s.record_write(state),
            Self::Mutable(s) => s.record_write(state),
            Self::NestedReadonly(s) => s.record_write(state),
            Self::NestedMutable(s) => s.record_write(state),
            Self::Global(s) => s.record_write(state),
            Self::TransparentMutable(s) => s.record_write(state),
            Self::TransparentReadonly(s) => s.record_write(state),
        }
    }

    /// Apply changes (only valid for mutable snapshots).
    pub fn apply(&self) -> SnapshotApplyResult {
        match self {
            Self::Mutable(s) => s.apply(),
            Self::NestedMutable(s) => s.apply(),
            Self::Global(s) => s.apply(),
            Self::TransparentMutable(s) => s.apply(),
            _ => panic!("Cannot apply a read-only snapshot"),
        }
    }

    /// Take a nested mutable snapshot (only valid for mutable snapshots).
    pub fn take_nested_mutable_snapshot(
        &self,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
    ) -> Self {
        match self {
            Self::Mutable(s) => {
                Self::NestedMutable(s.take_nested_mutable_snapshot(read_observer, write_observer))
            }
            Self::NestedMutable(s) => {
                Self::NestedMutable(s.take_nested_mutable_snapshot(read_observer, write_observer))
            }
            Self::Global(s) => {
                Self::Mutable(s.take_nested_mutable_snapshot(read_observer, write_observer))
            }
            Self::TransparentMutable(s) => Self::TransparentMutable(
                s.take_nested_mutable_snapshot(read_observer, write_observer),
            ),
            _ => panic!("Cannot take nested mutable snapshot from read-only snapshot"),
        }
    }
}

/// Get the current snapshot, or None if not in a snapshot context.
pub fn current_snapshot() -> Option<AnySnapshot> {
    crate::thread_state::try_snapshots(|state| state.current.borrow().clone()).flatten()
}

pub(crate) fn current_snapshot_reads_into(observer: &ReadObserver) -> bool {
    crate::thread_state::try_snapshots(|state| match &*state.current.borrow() {
        Some(AnySnapshot::TransparentMutable(snapshot)) => {
            snapshot.can_reuse() && snapshot.reads_into(observer)
        }
        _ => false,
    })
    .unwrap_or(false)
}

pub(crate) fn set_current_snapshot(snapshot: Option<AnySnapshot>) {
    crate::thread_state::try_snapshots(|state| {
        *state.current.borrow_mut() = snapshot;
    });
}

struct CurrentSnapshotGuard {
    previous: Option<AnySnapshot>,
}

impl CurrentSnapshotGuard {
    fn enter(snapshot: AnySnapshot) -> Self {
        let previous =
            crate::thread_state::try_snapshots(|state| state.current.replace(Some(snapshot)))
                .flatten();
        Self { previous }
    }
}

impl Drop for CurrentSnapshotGuard {
    fn drop(&mut self) {
        set_current_snapshot(self.previous.take());
    }
}

pub(crate) fn enter_snapshot_scope<T>(snapshot: AnySnapshot, f: impl FnOnce() -> T) -> T {
    let _guard = CurrentSnapshotGuard::enter(snapshot);
    f()
}

/// Creates a mutable snapshot, matching Kotlin's `Snapshot.takeMutableSnapshot` semantics.
///
/// If called while inside a MutableSnapshot, creates a nested snapshot that will
/// apply to the parent when `apply()` is called. This ensures proper isolation
/// between nested operations (like event handlers during animations).
///
/// If called while inside a GlobalSnapshot or no snapshot, creates a root
/// mutable snapshot that applies to the global state.
pub fn take_mutable_snapshot(
    read_observer: Option<ReadObserver>,
    write_observer: Option<WriteObserver>,
) -> AnyMutableSnapshot {
    match current_snapshot() {
        Some(AnySnapshot::Mutable(parent)) => AnyMutableSnapshot::Nested(
            parent.take_nested_mutable_snapshot(read_observer, write_observer),
        ),
        Some(AnySnapshot::NestedMutable(parent)) => AnyMutableSnapshot::Nested(
            parent.take_nested_mutable_snapshot(read_observer, write_observer),
        ),
        _ => AnyMutableSnapshot::Root(
            GlobalSnapshot::get_or_create()
                .take_nested_mutable_snapshot(read_observer, write_observer),
        ),
    }
}

/// Take a transparent observer mutable snapshot with optional observers.
///
/// This type of snapshot is used for read observation during composition,
/// matching Kotlin's Snapshot.observeInternal behavior. It allows writes
/// to happen during observation.
///
/// Transparent snapshots DO NOT allocate new IDs - they delegate to the
/// current/global snapshot, making them "transparent" to the snapshot system.
pub fn take_transparent_observer_mutable_snapshot(
    read_observer: Option<ReadObserver>,
    write_observer: Option<WriteObserver>,
) -> Rc<TransparentObserverMutableSnapshot> {
    take_transparent_observer_mutable_snapshot_reusing(read_observer, write_observer, None)
}

pub(crate) fn take_transparent_observer_mutable_snapshot_reusing(
    read_observer: Option<ReadObserver>,
    write_observer: Option<WriteObserver>,
    recycled: Option<Rc<TransparentObserverMutableSnapshot>>,
) -> Rc<TransparentObserverMutableSnapshot> {
    let parent = current_snapshot();
    match parent {
        Some(AnySnapshot::TransparentMutable(transparent)) if transparent.can_reuse() => {
            let (parent_read, parent_write) = transparent.observers();
            if already_observes(&read_observer, &parent_read)
                && already_observes(&write_observer, &parent_write)
            {
                return transparent;
            }
            TransparentObserverMutableSnapshot::new_reusing(
                recycled,
                transparent.snapshot_id(),
                transparent.invalid(),
                merge_read_observers(read_observer, parent_read),
                merge_write_observers(write_observer, parent_write),
                Some(Rc::downgrade(&transparent)),
            )
        }
        _ => {
            let current = current_snapshot()
                .unwrap_or_else(|| AnySnapshot::Global(GlobalSnapshot::get_or_create()));
            let id = current.snapshot_id();
            let invalid = current.invalid();
            TransparentObserverMutableSnapshot::new_reusing(
                recycled,
                id,
                invalid,
                read_observer,
                write_observer,
                None,
            )
        }
    }
}

fn already_observes(requested: &Option<ReadObserver>, installed: &Option<ReadObserver>) -> bool {
    match (requested, installed) {
        (None, _) => true,
        (Some(requested), Some(installed)) => Rc::ptr_eq(requested, installed),
        (Some(_), None) => false,
    }
}

/// Allocate a new record identifier that is distinct from any active snapshot id.
pub fn allocate_record_id() -> SnapshotId {
    runtime::allocate_record_id()
}

pub(crate) fn peek_next_snapshot_id() -> SnapshotId {
    runtime::peek_next_snapshot_id()
}

#[derive(Clone)]
pub(crate) struct ObserverId(Rc<()>);

impl ObserverId {
    fn new() -> Self {
        Self(Rc::new(()))
    }
}

impl PartialEq for ObserverId {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ObserverId {}

/// The apply observers of a thread in the order they registered, and the
/// list a notification runs, kept until an observer registers or leaves.
pub(crate) struct ApplyObservers {
    entries: Vec<(ObserverId, ApplyObserver)>,
    running: Option<Rc<[ApplyObserver]>>,
}

impl ApplyObservers {
    pub(crate) const fn new() -> Self {
        Self {
            entries: Vec::new(),
            running: None,
        }
    }

    fn insert(&mut self, id: ObserverId, observer: ApplyObserver) {
        self.entries.push((id, observer));
        self.running = None;
    }

    fn remove(&mut self, id: &ObserverId) {
        if let Some(at) = self.entries.iter().position(|(entry, _)| entry == id) {
            self.entries.remove(at);
            self.running = None;
        }
    }

    /// The observers a notification runs, apart from the registry: an
    /// observer may register or drop observers while it runs.
    fn running(&mut self) -> Rc<[ApplyObserver]> {
        let entries = &self.entries;
        Rc::clone(self.running.get_or_insert_with(|| {
            entries
                .iter()
                .map(|(_, observer)| Rc::clone(observer))
                .collect()
        }))
    }
}

const UNUSED_RECORD_CLEANUP_INTERVAL: usize = 2;
const UNUSED_RECORD_CLEANUP_BUSY_INTERVAL: usize = 1;
const UNUSED_RECORD_CLEANUP_MIN_SIZE: usize = 64;

/// Size figures of the snapshot system's bookkeeping on the current thread, from
/// `debug_snapshot_v2_stats`. `*_len` is the number of entries and `*_cap` the entries that fit
/// before the storage grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SnapshotV2DebugStats {
    /// Registered apply observers.
    pub apply_observers_len: usize,
    /// Apply observers the observer storage holds before it grows.
    pub apply_observers_cap: usize,
    /// State objects with a recorded last-write snapshot id.
    pub last_writes_len: usize,
    /// Entries the last-write record holds before it grows.
    pub last_writes_cap: usize,
    /// State objects tracked for later cleanup of unused records.
    pub extra_state_objects_len: usize,
    /// Entries the tracked state object set holds before it grows.
    pub extra_state_objects_cap: usize,
    /// Id of the snapshot at which unused records were last cleaned up.
    pub last_unused_record_cleanup: SnapshotId,
}

/// Returns the size figures of the snapshot system's bookkeeping on the current thread.
pub fn debug_snapshot_v2_stats() -> SnapshotV2DebugStats {
    crate::thread_state::snapshots(|state| {
        let observers = state.apply_observers.borrow();
        let writes = state.last_writes.borrow();
        let SnapshotWeakSetDebugStats {
            len: extra_state_objects_len,
            capacity: extra_state_objects_cap,
        } = state.extra_state_objects.borrow().debug_stats();
        SnapshotV2DebugStats {
            apply_observers_len: observers.entries.len(),
            apply_observers_cap: observers.entries.capacity(),
            last_writes_len: writes.len(),
            last_writes_cap: writes.capacity(),
            extra_state_objects_len,
            extra_state_objects_cap,
            last_unused_record_cleanup: state.last_unused_record_cleanup.get(),
        }
    })
}

/// Register an apply observer.
///
/// Returns a handle that will automatically unregister the observer when dropped.
pub fn register_apply_observer(observer: ApplyObserver) -> ObserverHandle {
    crate::thread_state::watch_thread_exit();
    let id = ObserverId::new();
    crate::thread_state::snapshots(|state| {
        state
            .apply_observers
            .borrow_mut()
            .insert(id.clone(), observer);
    });
    ObserverHandle {
        kind: ObserverKind::Apply,
        id,
    }
}

/// Handle for unregistering observers.
///
/// When dropped, automatically removes the associated observer.
pub struct ObserverHandle {
    kind: ObserverKind,
    id: ObserverId,
}

enum ObserverKind {
    Apply,
}

impl Drop for ObserverHandle {
    fn drop(&mut self) {
        match self.kind {
            ObserverKind::Apply => {
                // A handle dropped while the thread tears its locals down,
                // as a coroutine's state can be, finds the registry gone
                // and has nothing left to remove.
                crate::thread_state::try_snapshots(|state| {
                    state.apply_observers.borrow_mut().remove(&self.id);
                });
            }
        }
    }
}

pub(crate) fn notify_apply_observers(modified: &[Rc<dyn StateObject>], snapshot_id: SnapshotId) {
    let observers =
        crate::thread_state::snapshots(|state| state.apply_observers.borrow_mut().running());
    for observer in observers.iter() {
        observer(modified, snapshot_id);
    }
}

pub(crate) fn set_last_write(id: ObjectId, snapshot_id: SnapshotId) {
    crate::thread_state::snapshots(|state| {
        state.last_writes.borrow_mut().insert(id, snapshot_id);
    });
}

#[cfg(test)]
pub(crate) fn clear_last_writes() {
    crate::thread_state::snapshots(|state| state.last_writes.borrow_mut().clear());
}

pub(crate) fn check_and_overwrite_unused_records_locked() {
    let _retiring = super::state::RetiredValuesScope::enter();
    crate::thread_state::snapshots(|state| {
        state
            .extra_state_objects
            .borrow_mut()
            .remove_if(StateObject::overwrite_unused_records);
    });
}

pub(crate) fn maybe_check_and_overwrite_unused_records_locked(current_snapshot_id: SnapshotId) {
    let should_run = crate::thread_state::snapshots(|state| {
        let set = state.extra_state_objects.borrow();
        if set.is_empty() {
            return false;
        }
        let interval = if set.len() >= UNUSED_RECORD_CLEANUP_MIN_SIZE {
            UNUSED_RECORD_CLEANUP_BUSY_INTERVAL
        } else {
            UNUSED_RECORD_CLEANUP_INTERVAL
        };
        let due = current_snapshot_id.since(state.last_unused_record_cleanup.get()) >= interval;
        if due {
            state.last_unused_record_cleanup.set(current_snapshot_id);
        }
        due
    });

    if should_run {
        check_and_overwrite_unused_records_locked();
    }
}

#[cfg(test)]
pub(crate) fn clear_unused_record_cleanup_for_tests() {
    crate::thread_state::snapshots(|state| state.last_unused_record_cleanup.set(SnapshotId(0)));
}

pub(crate) fn optimistic_merges(
    current_snapshot_id: SnapshotId,
    base_parent_id: SnapshotId,
    modified_objects: &[(ObjectId, Rc<dyn StateObject>, SnapshotId)],
    invalid_snapshots: &SnapshotIdSet,
    applying_invalid: &SnapshotIdSet,
) -> Option<HashMap<usize, Rc<StateRecord>>> {
    if modified_objects.is_empty() {
        return None;
    }

    let mut result: Option<HashMap<usize, Rc<StateRecord>>> = None;

    for (_, state, writer_id) in modified_objects {
        let head = state.first_record();

        let Some(current) =
            crate::state::readable_record_for(&head, current_snapshot_id, invalid_snapshots)
        else {
            continue;
        };

        let (previous_opt, found_base) =
            mutable::find_previous_record(&head, base_parent_id, applying_invalid);
        let previous = previous_opt?;

        if !found_base || previous.snapshot_id() == crate::state::PREEXISTING_SNAPSHOT_ID {
            continue;
        }

        if Rc::ptr_eq(&current, &previous) {
            continue;
        }

        let applied = mutable::find_record_by_id(&head, *writer_id)?;

        let merged = state.merge_records(
            Rc::clone(&previous),
            Rc::clone(&current),
            Rc::clone(&applied),
        )?;

        result
            .get_or_insert_with(HashMap::default)
            .insert(Rc::as_ptr(&current) as usize, merged);
    }

    result
}

fn merge_observers(a: Option<ReadObserver>, b: Option<ReadObserver>) -> Option<ReadObserver> {
    match (a, b) {
        (None, None) => None,
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (Some(a), Some(b)) => Some(Rc::new(move |state: &dyn StateObject| {
            a(state);
            b(state);
        })),
    }
}

/// Merge two read observers into one.
///
/// # Thread Safety
/// The resulting Rc-wrapped closure may capture non-Send closures. This is safe
/// because observers are only invoked on the UI thread where they were created.
pub fn merge_read_observers(
    a: Option<ReadObserver>,
    b: Option<ReadObserver>,
) -> Option<ReadObserver> {
    merge_observers(a, b)
}

/// Merge two write observers into one.
///
/// # Thread Safety
/// The resulting Rc-wrapped closure may capture non-Send closures. This is safe
/// because observers are only invoked on the UI thread where they were created.
pub fn merge_write_observers(
    a: Option<WriteObserver>,
    b: Option<WriteObserver>,
) -> Option<WriteObserver> {
    merge_observers(a, b)
}

pub(crate) trait NestedMutableHost {
    fn snapshot_state(&self) -> &SnapshotState;
    fn nested_count(&self) -> &Cell<usize>;
}

pub(crate) fn clear_nested_child_on_dispose<P>(
    parent: &Rc<P>,
    child_id: SnapshotId,
) -> impl FnOnce() + 'static
where
    P: NestedMutableHost + 'static,
{
    let weak = Rc::downgrade(parent);
    move || {
        if let Some(parent) = weak.upgrade() {
            let nested_count = parent.nested_count();
            if nested_count.get() > 0 {
                nested_count.set(nested_count.get().saturating_sub(1));
            }
            let state = parent.snapshot_state();
            let new_invalid = state.invalid.borrow().clone().clear(child_id);
            state.invalid.replace(new_invalid);
            state.remove_pending_child(child_id);
        }
    }
}

pub(crate) fn allocate_nested_mutable_snapshot<P>(
    parent: &Rc<P>,
    root: Weak<MutableSnapshot>,
    read_observer: Option<ReadObserver>,
    write_observer: Option<WriteObserver>,
) -> Rc<NestedMutableSnapshot>
where
    P: NestedMutableHost + 'static,
{
    let state = parent.snapshot_state();
    let merged_read = merge_read_observers(read_observer, state.read_observer.borrow().clone());
    let merged_write = merge_write_observers(write_observer, state.write_observer.borrow().clone());

    let parent_id = state.id.get();
    let current_invalid = state.invalid.borrow().clone();

    let (new_id, _runtime_invalid) = allocate_snapshot();

    let parent_invalid_with_child = current_invalid.set(new_id);
    state.invalid.replace(parent_invalid_with_child);

    let invalid = current_invalid.add_range(parent_id.next(), new_id);

    let nested = NestedMutableSnapshot::new(
        new_id,
        invalid,
        merged_read,
        merged_write,
        root,
        state.id.get(),
    );

    let nested_count = parent.nested_count();
    nested_count.set(nested_count.get() + 1);
    state.add_pending_child(new_id);

    nested.set_on_dispose(clear_nested_child_on_dispose(parent, new_id));

    nested
}

#[cfg(test)]
#[path = "tests/snapshot_v2_tests.rs"]
mod tests;
