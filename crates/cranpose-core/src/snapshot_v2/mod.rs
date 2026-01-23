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
//! managed by the snapshot system.

// All snapshot types use Arc with Cell/RefCell for single-threaded shared ownership.
// This is safe because snapshots are thread-local and never cross thread boundaries.
#![allow(clippy::arc_with_non_send_sync)]

use crate::collections::map::HashMap; // FUTURE(no_std): replace HashMap/HashSet with arena-backed maps.
use crate::collections::map::HashSet;
use crate::snapshot_id_set::{SnapshotId, SnapshotIdSet};
use crate::snapshot_pinning::{self, PinHandle};
use crate::state::{StateObject, StateRecord};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};

mod global;
mod mutable;
mod nested;
mod readonly;
mod runtime;
mod transparent;

#[cfg(test)]
mod integration_tests;

pub use global::{advance_global_snapshot, GlobalSnapshot};
pub use mutable::MutableSnapshot;
pub use nested::{NestedMutableSnapshot, NestedReadonlySnapshot};
pub use readonly::ReadonlySnapshot;
pub use transparent::{TransparentObserverMutableSnapshot, TransparentObserverSnapshot};

pub(crate) use runtime::{allocate_snapshot, close_snapshot, with_runtime};
#[cfg(test)]
pub(crate) use runtime::{reset_runtime_for_tests, TestRuntimeGuard};

/// Observer that is called when a state object is read.
pub type ReadObserver = Arc<dyn Fn(&dyn StateObject) + 'static>;

/// Observer that is called when a state object is written.
pub type WriteObserver = Arc<dyn Fn(&dyn StateObject) + 'static>;

/// Apply observer that is called when a snapshot is applied.
pub type ApplyObserver = Arc<dyn Fn(&[Arc<dyn StateObject>], SnapshotId) + 'static>;

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
        matches!(self, SnapshotApplyResult::Success)
    }

    /// Check if the result is a failure.
    pub fn is_failure(&self) -> bool {
        matches!(self, SnapshotApplyResult::Failure)
    }

    /// Panic if the result is a failure (for use in tests).
    #[track_caller]
    pub fn check(&self) {
        if self.is_failure() {
            panic!("Snapshot apply failed");
        }
    }
}

/// Unique identifier for a state object in the modified set.
pub type StateObjectId = usize;

/// Enum wrapper for all snapshot types.
///
/// This provides a type-safe way to work with different snapshot types
/// without requiring trait objects, which avoids object-safety issues.
#[derive(Clone)]
pub enum AnySnapshot {
    Readonly(Arc<ReadonlySnapshot>),
    Mutable(Arc<MutableSnapshot>),
    NestedReadonly(Arc<NestedReadonlySnapshot>),
    NestedMutable(Arc<NestedMutableSnapshot>),
    Global(Arc<GlobalSnapshot>),
    TransparentMutable(Arc<TransparentObserverMutableSnapshot>),
    TransparentReadonly(Arc<TransparentObserverSnapshot>),
}

impl AnySnapshot {
    /// Get the snapshot ID.
    pub fn snapshot_id(&self) -> SnapshotId {
        match self {
            AnySnapshot::Readonly(s) => s.snapshot_id(),
            AnySnapshot::Mutable(s) => s.snapshot_id(),
            AnySnapshot::NestedReadonly(s) => s.snapshot_id(),
            AnySnapshot::NestedMutable(s) => s.snapshot_id(),
            AnySnapshot::Global(s) => s.snapshot_id(),
            AnySnapshot::TransparentMutable(s) => s.snapshot_id(),
            AnySnapshot::TransparentReadonly(s) => s.snapshot_id(),
        }
    }

    /// Get the set of invalid snapshot IDs.
    pub fn invalid(&self) -> SnapshotIdSet {
        match self {
            AnySnapshot::Readonly(s) => s.invalid(),
            AnySnapshot::Mutable(s) => s.invalid(),
            AnySnapshot::NestedReadonly(s) => s.invalid(),
            AnySnapshot::NestedMutable(s) => s.invalid(),
            AnySnapshot::Global(s) => s.invalid(),
            AnySnapshot::TransparentMutable(s) => s.invalid(),
            AnySnapshot::TransparentReadonly(s) => s.invalid(),
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
            AnySnapshot::Readonly(_) => true,
            AnySnapshot::Mutable(_) => false,
            AnySnapshot::NestedReadonly(_) => true,
            AnySnapshot::NestedMutable(_) => false,
            AnySnapshot::Global(_) => false,
            AnySnapshot::TransparentMutable(_) => false,
            AnySnapshot::TransparentReadonly(_) => true,
        }
    }

    /// Get the root snapshot.
    pub fn root(&self) -> AnySnapshot {
        match self {
            AnySnapshot::Readonly(s) => AnySnapshot::Readonly(s.root_readonly()),
            AnySnapshot::Mutable(s) => AnySnapshot::Mutable(s.root_mutable()),
            AnySnapshot::NestedReadonly(s) => AnySnapshot::NestedReadonly(s.root_nested_readonly()),
            AnySnapshot::NestedMutable(s) => AnySnapshot::Mutable(s.root_mutable()),
            AnySnapshot::Global(s) => AnySnapshot::Global(s.root_global()),
            AnySnapshot::TransparentMutable(s) => {
                AnySnapshot::TransparentMutable(s.root_transparent_mutable())
            }
            AnySnapshot::TransparentReadonly(s) => {
                AnySnapshot::TransparentReadonly(s.root_transparent_readonly())
            }
        }
    }

    /// Check if this snapshot refers to the same transparent snapshot.
    pub fn is_same_transparent(&self, other: &Arc<TransparentObserverMutableSnapshot>) -> bool {
        matches!(self, AnySnapshot::TransparentMutable(snapshot) if Arc::ptr_eq(snapshot, other))
    }

    /// Check if this snapshot refers to the same transparent mutable snapshot.
    pub fn is_same_transparent_mutable(
        &self,
        other: &Arc<TransparentObserverMutableSnapshot>,
    ) -> bool {
        self.is_same_transparent(other)
    }

    /// Check if this snapshot refers to the same transparent readonly snapshot.
    pub fn is_same_transparent_readonly(&self, other: &Arc<TransparentObserverSnapshot>) -> bool {
        matches!(self, AnySnapshot::TransparentReadonly(snapshot) if Arc::ptr_eq(snapshot, other))
    }

    /// Enter this snapshot, making it current for the duration of the closure.
    pub fn enter<T>(&self, f: impl FnOnce() -> T) -> T {
        match self {
            AnySnapshot::Readonly(s) => s.enter(f),
            AnySnapshot::Mutable(s) => s.enter(f),
            AnySnapshot::NestedReadonly(s) => s.enter(f),
            AnySnapshot::NestedMutable(s) => s.enter(f),
            AnySnapshot::Global(s) => s.enter(f),
            AnySnapshot::TransparentMutable(s) => s.enter(f),
            AnySnapshot::TransparentReadonly(s) => s.enter(f),
        }
    }

    /// Take a nested read-only snapshot.
    pub fn take_nested_snapshot(&self, read_observer: Option<ReadObserver>) -> AnySnapshot {
        match self {
            AnySnapshot::Readonly(s) => {
                AnySnapshot::Readonly(s.take_nested_snapshot(read_observer))
            }
            AnySnapshot::Mutable(s) => AnySnapshot::Readonly(s.take_nested_snapshot(read_observer)),
            AnySnapshot::NestedReadonly(s) => {
                AnySnapshot::NestedReadonly(s.take_nested_snapshot(read_observer))
            }
            AnySnapshot::NestedMutable(s) => {
                AnySnapshot::Readonly(s.take_nested_snapshot(read_observer))
            }
            AnySnapshot::Global(s) => AnySnapshot::Readonly(s.take_nested_snapshot(read_observer)),
            AnySnapshot::TransparentMutable(s) => {
                AnySnapshot::Readonly(s.take_nested_snapshot(read_observer))
            }
            AnySnapshot::TransparentReadonly(s) => {
                AnySnapshot::TransparentReadonly(s.take_nested_snapshot(read_observer))
            }
        }
    }

    /// Check if there are pending changes.
    pub fn has_pending_changes(&self) -> bool {
        match self {
            AnySnapshot::Readonly(s) => s.has_pending_changes(),
            AnySnapshot::Mutable(s) => s.has_pending_changes(),
            AnySnapshot::NestedReadonly(s) => s.has_pending_changes(),
            AnySnapshot::NestedMutable(s) => s.has_pending_changes(),
            AnySnapshot::Global(s) => s.has_pending_changes(),
            AnySnapshot::TransparentMutable(s) => s.has_pending_changes(),
            AnySnapshot::TransparentReadonly(s) => s.has_pending_changes(),
        }
    }

    /// Dispose of this snapshot.
    pub fn dispose(&self) {
        match self {
            AnySnapshot::Readonly(s) => s.dispose(),
            AnySnapshot::Mutable(s) => s.dispose(),
            AnySnapshot::NestedReadonly(s) => s.dispose(),
            AnySnapshot::NestedMutable(s) => s.dispose(),
            AnySnapshot::Global(s) => s.dispose(),
            AnySnapshot::TransparentMutable(s) => s.dispose(),
            AnySnapshot::TransparentReadonly(s) => s.dispose(),
        }
    }

    /// Check if disposed.
    pub fn is_disposed(&self) -> bool {
        match self {
            AnySnapshot::Readonly(s) => s.is_disposed(),
            AnySnapshot::Mutable(s) => s.is_disposed(),
            AnySnapshot::NestedReadonly(s) => s.is_disposed(),
            AnySnapshot::NestedMutable(s) => s.is_disposed(),
            AnySnapshot::Global(s) => s.is_disposed(),
            AnySnapshot::TransparentMutable(s) => s.is_disposed(),
            AnySnapshot::TransparentReadonly(s) => s.is_disposed(),
        }
    }

    /// Record a read.
    pub fn record_read(&self, state: &dyn StateObject) {
        match self {
            AnySnapshot::Readonly(s) => s.record_read(state),
            AnySnapshot::Mutable(s) => s.record_read(state),
            AnySnapshot::NestedReadonly(s) => s.record_read(state),
            AnySnapshot::NestedMutable(s) => s.record_read(state),
            AnySnapshot::Global(s) => s.record_read(state),
            AnySnapshot::TransparentMutable(s) => s.record_read(state),
            AnySnapshot::TransparentReadonly(s) => s.record_read(state),
        }
    }

    /// Record a write.
    pub fn record_write(&self, state: Arc<dyn StateObject>) {
        match self {
            AnySnapshot::Readonly(s) => s.record_write(state),
            AnySnapshot::Mutable(s) => s.record_write(state),
            AnySnapshot::NestedReadonly(s) => s.record_write(state),
            AnySnapshot::NestedMutable(s) => s.record_write(state),
            AnySnapshot::Global(s) => s.record_write(state),
            AnySnapshot::TransparentMutable(s) => s.record_write(state),
            AnySnapshot::TransparentReadonly(s) => s.record_write(state),
        }
    }

    /// Apply changes (only valid for mutable snapshots).
    pub fn apply(&self) -> SnapshotApplyResult {
        match self {
            AnySnapshot::Mutable(s) => s.apply(),
            AnySnapshot::NestedMutable(s) => s.apply(),
            AnySnapshot::Global(s) => s.apply(),
            AnySnapshot::TransparentMutable(s) => s.apply(),
            _ => panic!("Cannot apply a read-only snapshot"),
        }
    }

    /// Take a nested mutable snapshot (only valid for mutable snapshots).
    pub fn take_nested_mutable_snapshot(
        &self,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
    ) -> AnySnapshot {
        match self {
            AnySnapshot::Mutable(s) => AnySnapshot::NestedMutable(
                s.take_nested_mutable_snapshot(read_observer, write_observer),
            ),
            AnySnapshot::NestedMutable(s) => AnySnapshot::NestedMutable(
                s.take_nested_mutable_snapshot(read_observer, write_observer),
            ),
            AnySnapshot::Global(s) => {
                AnySnapshot::Mutable(s.take_nested_mutable_snapshot(read_observer, write_observer))
            }
            AnySnapshot::TransparentMutable(s) => AnySnapshot::TransparentMutable(
                s.take_nested_mutable_snapshot(read_observer, write_observer),
            ),
            _ => panic!("Cannot take nested mutable snapshot from read-only snapshot"),
        }
    }
}

thread_local! {
    // Thread-local storage for the current snapshot.
    static CURRENT_SNAPSHOT: RefCell<Option<AnySnapshot>> = const { RefCell::new(None) };
}

/// Get the current snapshot, or None if not in a snapshot context.
pub fn current_snapshot() -> Option<AnySnapshot> {
    CURRENT_SNAPSHOT
        .try_with(|cell| cell.borrow().clone())
        .unwrap_or(None)
}

/// Set the current snapshot (internal use only).
pub(crate) fn set_current_snapshot(snapshot: Option<AnySnapshot>) {
    let _ = CURRENT_SNAPSHOT.try_with(|cell| {
        *cell.borrow_mut() = snapshot;
    });
}

/// Convenience helper that mirrors the legacy `take_mutable_snapshot` API.
///
/// Returns a mutable snapshot rooted at the global snapshot with the provided
/// read/write observers installed.
pub fn take_mutable_snapshot(
    read_observer: Option<ReadObserver>,
    write_observer: Option<WriteObserver>,
) -> Arc<MutableSnapshot> {
    GlobalSnapshot::get_or_create().take_nested_mutable_snapshot(read_observer, write_observer)
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
) -> Arc<TransparentObserverMutableSnapshot> {
    let parent = current_snapshot();
    match parent {
        Some(AnySnapshot::TransparentMutable(transparent)) if transparent.can_reuse() => {
            // Reuse the existing transparent snapshot
            transparent
        }
        _ => {
            // Create a new transparent snapshot using the current snapshot's ID
            // Transparent snapshots do NOT allocate new IDs!
            let current = current_snapshot()
                .unwrap_or_else(|| AnySnapshot::Global(GlobalSnapshot::get_or_create()));
            let id = current.snapshot_id();
            let invalid = current.invalid();
            TransparentObserverMutableSnapshot::new(
                id,
                invalid,
                read_observer,
                write_observer,
                None,
            )
        }
    }
}

/// Allocate a new record identifier that is distinct from any active snapshot id.
pub fn allocate_record_id() -> SnapshotId {
    runtime::allocate_record_id()
}

/// Get the next snapshot ID that will be allocated without incrementing the counter.
///
/// This is used for cleanup operations to determine the reuse limit.
/// Mirrors Kotlin's `nextSnapshotId` field access.
pub(crate) fn peek_next_snapshot_id() -> SnapshotId {
    runtime::peek_next_snapshot_id()
}

/// Global counter for unique observer IDs.
static NEXT_OBSERVER_ID: AtomicUsize = AtomicUsize::new(1);

thread_local! {
    // Global map of apply observers indexed by unique ID.
    static APPLY_OBSERVERS: RefCell<HashMap<usize, ApplyObserver>> = RefCell::new(HashMap::default());
}

thread_local! {
    // Thread-local last-writer registry used for conflict detection in v2.
    //
    // Maps a state object id to the snapshot id of the most recent successful apply
    // that modified the object. This is a simplified conflict tracking mechanism
    // for Phase 2.1 before full record-chain merging is implemented.
    //
    // Thread-local ensures test isolation - each test thread has its own registry.
    static LAST_WRITES: RefCell<HashMap<StateObjectId, SnapshotId>> = RefCell::new(HashMap::default());
}

thread_local! {
    // Thread-local weak set of state objects with multiple records for periodic garbage collection.
    // Mirrors Kotlin's `extraStateObjects` WeakSet.
    static EXTRA_STATE_OBJECTS: RefCell<crate::snapshot_weak_set::SnapshotWeakSet> = RefCell::new(crate::snapshot_weak_set::SnapshotWeakSet::new());
}

const UNUSED_RECORD_CLEANUP_INTERVAL: SnapshotId = 2;
const UNUSED_RECORD_CLEANUP_BUSY_INTERVAL: SnapshotId = 1;
const UNUSED_RECORD_CLEANUP_MIN_SIZE: usize = 64;

thread_local! {
    static LAST_UNUSED_RECORD_CLEANUP: Cell<SnapshotId> = const { Cell::new(0) };
}

/// Register an apply observer.
///
/// Returns a handle that will automatically unregister the observer when dropped.
pub fn register_apply_observer(observer: ApplyObserver) -> ObserverHandle {
    let id = NEXT_OBSERVER_ID.fetch_add(1, Ordering::SeqCst);
    APPLY_OBSERVERS.with(|cell| {
        cell.borrow_mut().insert(id, observer);
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
    id: usize,
}

enum ObserverKind {
    Apply,
}

impl Drop for ObserverHandle {
    fn drop(&mut self) {
        match self.kind {
            ObserverKind::Apply => {
                APPLY_OBSERVERS.with(|cell| {
                    cell.borrow_mut().remove(&self.id);
                });
            }
        }
    }
}

/// Notify apply observers that a snapshot was applied.
pub(crate) fn notify_apply_observers(modified: &[Arc<dyn StateObject>], snapshot_id: SnapshotId) {
    // Copy observers so callbacks run outside the borrow
    APPLY_OBSERVERS.with(|cell| {
        let observers: Vec<ApplyObserver> = cell.borrow().values().cloned().collect();
        for observer in observers.into_iter() {
            observer(modified, snapshot_id);
        }
    });
}

/// Get the last successful writer snapshot id for a given object id.
#[allow(dead_code)]
pub(crate) fn get_last_write(id: StateObjectId) -> Option<SnapshotId> {
    LAST_WRITES.with(|cell| cell.borrow().get(&id).copied())
}

/// Record the last successful writer snapshot id for a given object id.
pub(crate) fn set_last_write(id: StateObjectId, snapshot_id: SnapshotId) {
    LAST_WRITES.with(|cell| {
        cell.borrow_mut().insert(id, snapshot_id);
    });
}

/// Clear all last write records (for testing).
#[cfg(test)]
pub(crate) fn clear_last_writes() {
    LAST_WRITES.with(|cell| {
        cell.borrow_mut().clear();
    });
}

/// Check and overwrite unused records for all tracked state objects.
///
/// Mirrors Kotlin's `checkAndOverwriteUnusedRecordsLocked()`. This method:
/// 1. Iterates through all state objects in `EXTRA_STATE_OBJECTS`
/// 2. Calls `overwrite_unused_records()` on each
/// 3. Removes states that no longer need tracking (down to 1 or fewer records)
/// 4. Automatically cleans up dead weak references
pub(crate) fn check_and_overwrite_unused_records_locked() {
    EXTRA_STATE_OBJECTS.with(|cell| {
        cell.borrow_mut().remove_if(|state| {
            // Returns true to keep, false to remove
            state.overwrite_unused_records()
        });
    });
}

pub(crate) fn maybe_check_and_overwrite_unused_records_locked(current_snapshot_id: SnapshotId) {
    let should_run = EXTRA_STATE_OBJECTS.with(|cell| {
        let set = cell.borrow();
        if set.is_empty() {
            return false;
        }
        let last_cleanup = LAST_UNUSED_RECORD_CLEANUP.with(|last| last.get());
        let interval = if set.len() >= UNUSED_RECORD_CLEANUP_MIN_SIZE {
            UNUSED_RECORD_CLEANUP_BUSY_INTERVAL
        } else {
            UNUSED_RECORD_CLEANUP_INTERVAL
        };
        current_snapshot_id.saturating_sub(last_cleanup) >= interval
    });

    if should_run {
        LAST_UNUSED_RECORD_CLEANUP.with(|cell| cell.set(current_snapshot_id));
        check_and_overwrite_unused_records_locked();
    }
}

/// Process a state object for unused record cleanup, tracking it if needed.
///
/// Mirrors Kotlin's `processForUnusedRecordsLocked()`. After a state is modified:
/// 1. Calls `overwrite_unused_records()` to clean up old records
/// 2. If the state has multiple records, adds it to `EXTRA_STATE_OBJECTS` for future cleanup
#[allow(dead_code)]
pub(crate) fn process_for_unused_records_locked(state: &Arc<dyn crate::state::StateObject>) {
    if state.overwrite_unused_records() {
        // State has multiple records - track it for future cleanup
        EXTRA_STATE_OBJECTS.with(|cell| {
            cell.borrow_mut().add_trait_object(state);
        });
    }
}

#[cfg(test)]
pub(crate) fn clear_unused_record_cleanup_for_tests() {
    LAST_UNUSED_RECORD_CLEANUP.with(|cell| cell.set(0));
}

pub(crate) fn optimistic_merges(
    current_snapshot_id: SnapshotId,
    base_parent_id: SnapshotId,
    modified_objects: &[(StateObjectId, Arc<dyn StateObject>, SnapshotId)],
    invalid_snapshots: &SnapshotIdSet,
) -> Option<HashMap<usize, Rc<StateRecord>>> {
    if modified_objects.is_empty() {
        return None;
    }

    let mut result: Option<HashMap<usize, Rc<StateRecord>>> = None;

    for (_, state, writer_id) in modified_objects.iter() {
        let head = state.first_record();

        let current = match crate::state::readable_record_for(
            &head,
            current_snapshot_id,
            invalid_snapshots,
        ) {
            Some(record) => record,
            None => continue,
        };

        let (previous_opt, found_base) = mutable::find_previous_record(&head, base_parent_id);
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

/// Merge two read observers into one.
///
/// # Thread Safety
/// The resulting Arc-wrapped closure may capture non-Send closures. This is safe
/// because observers are only invoked on the UI thread where they were created.
#[allow(clippy::arc_with_non_send_sync)]
pub fn merge_read_observers(
    a: Option<ReadObserver>,
    b: Option<ReadObserver>,
) -> Option<ReadObserver> {
    match (a, b) {
        (None, None) => None,
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (Some(a), Some(b)) => Some(Arc::new(move |state: &dyn StateObject| {
            a(state);
            b(state);
        })),
    }
}

/// Merge two write observers into one.
///
/// # Thread Safety
/// The resulting Arc-wrapped closure may capture non-Send closures. This is safe
/// because observers are only invoked on the UI thread where they were created.
#[allow(clippy::arc_with_non_send_sync)]
pub fn merge_write_observers(
    a: Option<WriteObserver>,
    b: Option<WriteObserver>,
) -> Option<WriteObserver> {
    match (a, b) {
        (None, None) => None,
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (Some(a), Some(b)) => Some(Arc::new(move |state: &dyn StateObject| {
            a(state);
            b(state);
        })),
    }
}

/// Shared state for all snapshots.
pub(crate) struct SnapshotState {
    /// The snapshot ID.
    pub(crate) id: Cell<SnapshotId>,
    /// Set of invalid snapshot IDs.
    pub(crate) invalid: RefCell<SnapshotIdSet>,
    /// Pin handle to keep this snapshot alive.
    pub(crate) pin_handle: Cell<PinHandle>,
    /// Whether this snapshot has been disposed.
    pub(crate) disposed: Cell<bool>,
    /// Read observer, if any.
    pub(crate) read_observer: Option<ReadObserver>,
    /// Write observer, if any.
    pub(crate) write_observer: Option<WriteObserver>,
    /// Modified state objects.
    #[allow(clippy::type_complexity)]
    // HashMap value is (Arc, SnapshotId) - reasonable for tracking state
    pub(crate) modified: RefCell<HashMap<StateObjectId, (Arc<dyn StateObject>, SnapshotId)>>,
    /// Optional callback invoked once when disposed.
    on_dispose: RefCell<Option<Box<dyn FnOnce()>>>,
    /// Whether this snapshot's lifecycle is tracked in the global runtime.
    runtime_tracked: bool,
    /// Set of child snapshot ids that are still pending.
    pending_children: RefCell<HashSet<SnapshotId>>,
}

impl SnapshotState {
    pub(crate) fn new(
        id: SnapshotId,
        invalid: SnapshotIdSet,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
        runtime_tracked: bool,
    ) -> Self {
        Self::new_with_pinning(
            id,
            invalid,
            read_observer,
            write_observer,
            runtime_tracked,
            true,
        )
    }

    /// Create a new SnapshotState with optional pinning control.
    ///
    /// Transparent snapshots should pass `should_pin: false` since they don't allocate
    /// new IDs and shouldn't prevent garbage collection of old records.
    pub(crate) fn new_with_pinning(
        id: SnapshotId,
        invalid: SnapshotIdSet,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
        runtime_tracked: bool,
        should_pin: bool,
    ) -> Self {
        let pin_handle = if should_pin {
            snapshot_pinning::track_pinning(id, &invalid)
        } else {
            snapshot_pinning::PinHandle::INVALID
        };
        Self {
            id: Cell::new(id),
            invalid: RefCell::new(invalid),
            pin_handle: Cell::new(pin_handle),
            disposed: Cell::new(false),
            read_observer,
            write_observer,
            modified: RefCell::new(HashMap::default()),
            on_dispose: RefCell::new(None),
            runtime_tracked,
            pending_children: RefCell::new(HashSet::default()),
        }
    }

    pub(crate) fn record_read(&self, state: &dyn StateObject) {
        if let Some(ref observer) = self.read_observer {
            observer(state);
        }
    }

    pub(crate) fn record_write(&self, state: Arc<dyn StateObject>, writer_id: SnapshotId) {
        // Get the unique ID for this state object
        let state_id = state.object_id().as_usize();

        let mut modified = self.modified.borrow_mut();

        // Only call observer on first write
        match modified.entry(state_id) {
            std::collections::hash_map::Entry::Vacant(e) => {
                if let Some(ref observer) = self.write_observer {
                    observer(&*state);
                }
                // Store the Arc and writer id in the modified set
                e.insert((state, writer_id));
            }
            std::collections::hash_map::Entry::Occupied(mut e) => {
                // Update the writer id to reflect the most recent writer for this state.
                e.insert((state, writer_id));
            }
        }
    }

    pub(crate) fn dispose(&self) {
        if !self.disposed.replace(true) {
            let pin_handle = self.pin_handle.get();
            snapshot_pinning::release_pinning(pin_handle);
            if let Some(cb) = self.on_dispose.borrow_mut().take() {
                cb();
            }
            if self.runtime_tracked {
                close_snapshot(self.id.get());
            }
        }
    }

    pub(crate) fn add_pending_child(&self, id: SnapshotId) {
        self.pending_children.borrow_mut().insert(id);
    }

    pub(crate) fn remove_pending_child(&self, id: SnapshotId) {
        self.pending_children.borrow_mut().remove(&id);
    }

    pub(crate) fn has_pending_children(&self) -> bool {
        !self.pending_children.borrow().is_empty()
    }

    pub(crate) fn pending_children(&self) -> Vec<SnapshotId> {
        self.pending_children.borrow().iter().copied().collect()
    }

    pub(crate) fn set_on_dispose<F>(&self, f: F)
    where
        F: FnOnce() + 'static,
    {
        *self.on_dispose.borrow_mut() = Some(Box::new(f));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_result_is_success() {
        assert!(SnapshotApplyResult::Success.is_success());
        assert!(!SnapshotApplyResult::Failure.is_success());
    }

    #[test]
    fn test_apply_result_is_failure() {
        assert!(!SnapshotApplyResult::Success.is_failure());
        assert!(SnapshotApplyResult::Failure.is_failure());
    }

    #[test]
    fn test_apply_result_check_success() {
        SnapshotApplyResult::Success.check(); // Should not panic
    }

    #[test]
    #[should_panic(expected = "Snapshot apply failed")]
    fn test_apply_result_check_failure() {
        SnapshotApplyResult::Failure.check(); // Should panic
    }

    #[test]
    fn test_merge_read_observers_both_none() {
        let result = merge_read_observers(None, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_merge_read_observers_one_some() {
        let observer = Arc::new(|_: &dyn StateObject| {});
        let result = merge_read_observers(Some(observer.clone()), None);
        assert!(result.is_some());

        let result = merge_read_observers(None, Some(observer));
        assert!(result.is_some());
    }

    #[test]
    fn test_merge_write_observers_both_none() {
        let result = merge_write_observers(None, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_merge_write_observers_one_some() {
        let observer = Arc::new(|_: &dyn StateObject| {});
        let result = merge_write_observers(Some(observer.clone()), None);
        assert!(result.is_some());

        let result = merge_write_observers(None, Some(observer));
        assert!(result.is_some());
    }

    #[test]
    fn test_current_snapshot_none_initially() {
        set_current_snapshot(None);
        assert!(current_snapshot().is_none());
    }

    // Test helper: Simple state object for testing
    struct TestStateObject {
        id: usize,
    }

    impl TestStateObject {
        fn new(id: usize) -> Arc<Self> {
            Arc::new(Self { id })
        }
    }

    impl StateObject for TestStateObject {
        fn object_id(&self) -> crate::state::ObjectId {
            crate::state::ObjectId(self.id)
        }

        fn first_record(&self) -> Rc<crate::state::StateRecord> {
            unimplemented!("Not needed for observer tests")
        }

        fn readable_record(
            &self,
            _snapshot_id: SnapshotId,
            _invalid: &SnapshotIdSet,
        ) -> Rc<crate::state::StateRecord> {
            unimplemented!("Not needed for observer tests")
        }

        fn prepend_state_record(&self, _record: Rc<crate::state::StateRecord>) {
            unimplemented!("Not needed for observer tests")
        }

        fn promote_record(&self, _child_id: SnapshotId) -> Result<(), &'static str> {
            unimplemented!("Not needed for observer tests")
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_apply_observer_receives_correct_modified_objects() {
        use std::sync::Mutex;

        // Setup: Track what the observer receives
        let received_count = Arc::new(Mutex::new(0));
        let received_snapshot_id = Arc::new(Mutex::new(0));

        let received_count_clone = received_count.clone();
        let received_snapshot_id_clone = received_snapshot_id.clone();

        // Register observer
        let _handle = register_apply_observer(Arc::new(move |modified, snapshot_id| {
            *received_snapshot_id_clone.lock().unwrap() = snapshot_id;
            *received_count_clone.lock().unwrap() = modified.len();
        }));

        // Create test objects
        let obj1: Arc<dyn StateObject> = TestStateObject::new(42);
        let obj2: Arc<dyn StateObject> = TestStateObject::new(99);
        let modified = vec![obj1, obj2];

        // Notify observers
        notify_apply_observers(&modified, 123);

        // Verify
        assert_eq!(*received_snapshot_id.lock().unwrap(), 123);
        assert_eq!(*received_count.lock().unwrap(), 2);
    }

    #[test]
    fn test_apply_observer_receives_correct_snapshot_id() {
        use std::sync::Mutex;

        let received_id = Arc::new(Mutex::new(0));
        let received_id_clone = received_id.clone();

        let _handle = register_apply_observer(Arc::new(move |_, snapshot_id| {
            *received_id_clone.lock().unwrap() = snapshot_id;
        }));

        // Notify with specific snapshot ID
        notify_apply_observers(&[], 456);

        assert_eq!(*received_id.lock().unwrap(), 456);
    }

    #[test]
    fn test_multiple_apply_observers_all_called() {
        use std::sync::Mutex;

        let call_count1 = Arc::new(Mutex::new(0));
        let call_count2 = Arc::new(Mutex::new(0));
        let call_count3 = Arc::new(Mutex::new(0));

        let call_count1_clone = call_count1.clone();
        let call_count2_clone = call_count2.clone();
        let call_count3_clone = call_count3.clone();

        // Register three observers
        let _handle1 = register_apply_observer(Arc::new(move |_, _| {
            *call_count1_clone.lock().unwrap() += 1;
        }));

        let _handle2 = register_apply_observer(Arc::new(move |_, _| {
            *call_count2_clone.lock().unwrap() += 1;
        }));

        let _handle3 = register_apply_observer(Arc::new(move |_, _| {
            *call_count3_clone.lock().unwrap() += 1;
        }));

        // Notify observers
        notify_apply_observers(&[], 1);

        // All three should have been called
        assert_eq!(*call_count1.lock().unwrap(), 1);
        assert_eq!(*call_count2.lock().unwrap(), 1);
        assert_eq!(*call_count3.lock().unwrap(), 1);

        // Notify again
        notify_apply_observers(&[], 2);

        // All should have been called twice
        assert_eq!(*call_count1.lock().unwrap(), 2);
        assert_eq!(*call_count2.lock().unwrap(), 2);
        assert_eq!(*call_count3.lock().unwrap(), 2);
    }

    #[test]
    fn test_apply_observer_not_called_for_empty_modifications() {
        use std::sync::Mutex;

        let call_count = Arc::new(Mutex::new(0));
        let call_count_clone = call_count.clone();

        let _handle = register_apply_observer(Arc::new(move |modified, _| {
            // Observer should still be called, but with empty array
            *call_count_clone.lock().unwrap() += 1;
            assert_eq!(modified.len(), 0);
        }));

        // Notify with no modifications
        notify_apply_observers(&[], 1);

        // Observer should have been called
        assert_eq!(*call_count.lock().unwrap(), 1);
    }

    #[test]
    fn test_observer_handle_drop_removes_correct_observer() {
        use std::sync::Mutex;

        // Register three observers that track their IDs
        let calls = Arc::new(Mutex::new(Vec::new()));

        let calls1 = calls.clone();
        let handle1 = register_apply_observer(Arc::new(move |_, _| {
            calls1.lock().unwrap().push(1);
        }));

        let calls2 = calls.clone();
        let handle2 = register_apply_observer(Arc::new(move |_, _| {
            calls2.lock().unwrap().push(2);
        }));

        let calls3 = calls.clone();
        let handle3 = register_apply_observer(Arc::new(move |_, _| {
            calls3.lock().unwrap().push(3);
        }));

        // All three should be called
        notify_apply_observers(&[], 1);
        let result = calls.lock().unwrap().clone();
        assert_eq!(result.len(), 3);
        assert!(result.contains(&1));
        assert!(result.contains(&2));
        assert!(result.contains(&3));
        calls.lock().unwrap().clear();

        // Drop handle2 (middle one)
        drop(handle2);

        // Only 1 and 3 should be called now
        notify_apply_observers(&[], 2);
        let result = calls.lock().unwrap().clone();
        assert_eq!(result.len(), 2);
        assert!(result.contains(&1));
        assert!(result.contains(&3));
        assert!(!result.contains(&2));
        calls.lock().unwrap().clear();

        // Drop handle1
        drop(handle1);

        // Only 3 should be called
        notify_apply_observers(&[], 3);
        let result = calls.lock().unwrap().clone();
        assert_eq!(result.len(), 1);
        assert!(result.contains(&3));
        calls.lock().unwrap().clear();

        // Drop handle3
        drop(handle3);

        // None should be called
        notify_apply_observers(&[], 4);
        assert_eq!(calls.lock().unwrap().len(), 0);
    }

    #[test]
    fn test_observer_handle_drop_in_different_orders() {
        use std::sync::Mutex;

        // Test 1: Drop in reverse order (3, 2, 1)
        {
            let calls = Arc::new(Mutex::new(Vec::new()));

            let calls1 = calls.clone();
            let h1 = register_apply_observer(Arc::new(move |_, _| {
                calls1.lock().unwrap().push(1);
            }));

            let calls2 = calls.clone();
            let h2 = register_apply_observer(Arc::new(move |_, _| {
                calls2.lock().unwrap().push(2);
            }));

            let calls3 = calls.clone();
            let h3 = register_apply_observer(Arc::new(move |_, _| {
                calls3.lock().unwrap().push(3);
            }));

            drop(h3);
            notify_apply_observers(&[], 1);
            let result = calls.lock().unwrap().clone();
            assert!(result.contains(&1) && result.contains(&2) && !result.contains(&3));
            calls.lock().unwrap().clear();

            drop(h2);
            notify_apply_observers(&[], 2);
            let result = calls.lock().unwrap().clone();
            assert_eq!(result.len(), 1);
            assert!(result.contains(&1));
            calls.lock().unwrap().clear();

            drop(h1);
            notify_apply_observers(&[], 3);
            assert_eq!(calls.lock().unwrap().len(), 0);
        }

        // Test 2: Drop in forward order (1, 2, 3)
        {
            let calls = Arc::new(Mutex::new(Vec::new()));

            let calls1 = calls.clone();
            let h1 = register_apply_observer(Arc::new(move |_, _| {
                calls1.lock().unwrap().push(1);
            }));

            let calls2 = calls.clone();
            let h2 = register_apply_observer(Arc::new(move |_, _| {
                calls2.lock().unwrap().push(2);
            }));

            let calls3 = calls.clone();
            let h3 = register_apply_observer(Arc::new(move |_, _| {
                calls3.lock().unwrap().push(3);
            }));

            drop(h1);
            notify_apply_observers(&[], 1);
            let result = calls.lock().unwrap().clone();
            assert!(!result.contains(&1) && result.contains(&2) && result.contains(&3));
            calls.lock().unwrap().clear();

            drop(h2);
            notify_apply_observers(&[], 2);
            let result = calls.lock().unwrap().clone();
            assert_eq!(result.len(), 1);
            assert!(result.contains(&3));
            calls.lock().unwrap().clear();

            drop(h3);
            notify_apply_observers(&[], 3);
            assert_eq!(calls.lock().unwrap().len(), 0);
        }
    }

    #[test]
    fn test_remaining_observers_still_work_after_drop() {
        use std::sync::Mutex;

        let calls = Arc::new(Mutex::new(Vec::new()));

        let calls1 = calls.clone();
        let handle1 = register_apply_observer(Arc::new(move |_, snapshot_id| {
            calls1.lock().unwrap().push((1, snapshot_id));
        }));

        let calls2 = calls.clone();
        let handle2 = register_apply_observer(Arc::new(move |_, snapshot_id| {
            calls2.lock().unwrap().push((2, snapshot_id));
        }));

        // Both work
        notify_apply_observers(&[], 100);
        assert_eq!(calls.lock().unwrap().len(), 2);
        calls.lock().unwrap().clear();

        // Drop handle1
        drop(handle1);

        // handle2 still works with new snapshot ID
        notify_apply_observers(&[], 200);
        assert_eq!(*calls.lock().unwrap(), vec![(2, 200)]);
        calls.lock().unwrap().clear();

        // Register new observer after dropping handle1
        let calls3 = calls.clone();
        let _handle3 = register_apply_observer(Arc::new(move |_, snapshot_id| {
            calls3.lock().unwrap().push((3, snapshot_id));
        }));

        // Both handle2 and handle3 work
        notify_apply_observers(&[], 300);
        let result = calls.lock().unwrap().clone();
        assert_eq!(result.len(), 2);
        assert!(result.contains(&(2, 300)));
        assert!(result.contains(&(3, 300)));

        drop(handle2);
    }

    #[test]
    fn test_observer_ids_are_unique() {
        use std::sync::Mutex;

        let ids = Arc::new(Mutex::new(std::collections::HashSet::new()));

        let mut handles = Vec::new();

        // Register 100 observers and track their IDs through side channel
        // Since we can't directly access the ID from the handle, we'll verify
        // uniqueness by ensuring all observers get called
        for i in 0..100 {
            let ids_clone = ids.clone();
            let handle = register_apply_observer(Arc::new(move |_, _| {
                ids_clone.lock().unwrap().insert(i);
            }));
            handles.push(handle);
        }

        // Notify once - all 100 should be called
        notify_apply_observers(&[], 1);
        assert_eq!(ids.lock().unwrap().len(), 100);

        // Drop every other handle
        for i in (0..100).step_by(2) {
            handles.remove(i / 2);
        }

        // Clear and notify again - only 50 should be called
        ids.lock().unwrap().clear();
        notify_apply_observers(&[], 2);
        assert_eq!(ids.lock().unwrap().len(), 50);
    }

    #[test]
    fn test_state_object_storage_in_modified_set() {
        use crate::state::StateObject;
        use std::cell::Cell;

        // Mock StateObject for testing
        #[allow(dead_code)]
        struct TestState {
            value: Cell<i32>,
        }

        impl StateObject for TestState {
            fn object_id(&self) -> crate::state::ObjectId {
                crate::state::ObjectId(12345)
            }

            fn first_record(&self) -> Rc<crate::state::StateRecord> {
                unimplemented!("Not needed for this test")
            }

            fn readable_record(
                &self,
                _snapshot_id: SnapshotId,
                _invalid: &SnapshotIdSet,
            ) -> Rc<crate::state::StateRecord> {
                unimplemented!("Not needed for this test")
            }

            fn prepend_state_record(&self, _record: Rc<crate::state::StateRecord>) {
                unimplemented!("Not needed for this test")
            }

            fn promote_record(&self, _child_id: SnapshotId) -> Result<(), &'static str> {
                unimplemented!("Not needed for this test")
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let state = SnapshotState::new(1, SnapshotIdSet::new(), None, None, false);

        // Create Arc to state object
        let state_obj = Arc::new(TestState {
            value: Cell::new(42),
        }) as Arc<dyn StateObject>;

        // Record write should store the Arc
        state.record_write(state_obj.clone(), 1);

        // Verify it was stored in the modified set
        let modified = state.modified.borrow();
        assert_eq!(modified.len(), 1);
        assert!(modified.contains_key(&12345));

        // Verify the Arc is the same object
        let (stored, writer_id) = modified.get(&12345).unwrap();
        assert_eq!(stored.object_id().as_usize(), 12345);
        assert_eq!(*writer_id, 1);
    }

    #[test]
    fn test_multiple_writes_to_same_state_object() {
        use crate::state::StateObject;
        use std::cell::Cell;

        #[allow(dead_code)]
        struct TestState {
            value: Cell<i32>,
        }

        impl StateObject for TestState {
            fn object_id(&self) -> crate::state::ObjectId {
                crate::state::ObjectId(99999)
            }

            fn first_record(&self) -> Rc<crate::state::StateRecord> {
                unimplemented!()
            }

            fn readable_record(
                &self,
                _snapshot_id: SnapshotId,
                _invalid: &SnapshotIdSet,
            ) -> Rc<crate::state::StateRecord> {
                unimplemented!()
            }

            fn prepend_state_record(&self, _record: Rc<crate::state::StateRecord>) {
                unimplemented!()
            }

            fn promote_record(&self, _child_id: SnapshotId) -> Result<(), &'static str> {
                unimplemented!()
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let state = SnapshotState::new(1, SnapshotIdSet::new(), None, None, false);
        let state_obj = Arc::new(TestState {
            value: Cell::new(100),
        }) as Arc<dyn StateObject>;

        // First write
        state.record_write(state_obj.clone(), 1);
        assert_eq!(state.modified.borrow().len(), 1);

        // Second write to same object should not add a new entry but updates writer id
        state.record_write(state_obj.clone(), 2);
        let modified = state.modified.borrow();
        assert_eq!(modified.len(), 1);
        assert!(modified.contains_key(&99999));
        let (_, writer_id) = modified.get(&99999).unwrap();
        assert_eq!(*writer_id, 2);
    }
}
