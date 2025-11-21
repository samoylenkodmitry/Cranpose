//! Global runtime state for snapshot v2.
//!
//! This module implements a Rust translation of the core global state
//! management that backs Jetpack Compose's snapshot system. The goal is
//! to faithfully mirror the Kotlin implementation's behaviour for
//! snapshot identifier allocation, open snapshot tracking, and global
//! snapshot bookkeeping.
//!
//! At this stage the runtime focuses on:
//! - Tracking the set of currently open snapshot IDs (used to seed the
//!   `invalid` set for new snapshots).
//! - Allocating monotonically increasing snapshot identifiers.
//! - Recording the global snapshot identifier.
//!
//! Additional responsibilities such as pinning, double-index heaps, or
//! observer dispatch will be translated in follow-up changes.

use super::*;
use std::cell::Cell;
use std::sync::{LazyLock, Mutex};

/// Snapshot identifiers less than or equal to this value are considered
/// pre-existing. This mirrors `Snapshot.PreexistingSnapshotId` in the
/// Kotlin runtime.
const PREEXISTING_SNAPSHOT_ID: SnapshotId = 1;

/// Initial snapshot identifier assigned to the global snapshot. The Kotlin
/// runtime reserves snapshot id `0`, seeds `nextSnapshotId` with
/// `PreexistingSnapshotId + 1`, and then immediately allocates the global
/// snapshot. We replicate that ordering here.
const INITIAL_GLOBAL_SNAPSHOT_ID: SnapshotId = PREEXISTING_SNAPSHOT_ID + 1;

/// Global runtime singleton, guarded by a mutex so we can mutate state safely.
static SNAPSHOT_RUNTIME: LazyLock<Mutex<SnapshotRuntime>> =
    LazyLock::new(|| Mutex::new(SnapshotRuntime::new()));

thread_local! {
    static RUNTIME_LOCK_DEPTH: Cell<usize> = const { Cell::new(0) };
}

struct RuntimeLockGuard;

impl RuntimeLockGuard {
    fn enter() -> Self {
        RUNTIME_LOCK_DEPTH.with(|cell| cell.set(cell.get() + 1));
        Self
    }
}

impl Drop for RuntimeLockGuard {
    fn drop(&mut self) {
        RUNTIME_LOCK_DEPTH.with(|cell| {
            let depth = cell.get();
            debug_assert!(depth > 0, "runtime lock depth underflow");
            cell.set(depth.saturating_sub(1));
        });
    }
}

/// Helper for temporarily mutating the runtime state.
///
/// This mirrors the `sync { ... }` helper in Kotlin by ensuring exclusive
/// access to the global snapshot bookkeeping while the provided closure runs.
pub(crate) fn with_runtime<T>(f: impl FnOnce(&mut SnapshotRuntime) -> T) -> T {
    let mut guard = SNAPSHOT_RUNTIME.lock().unwrap_or_else(|poisoned| {
        // If the mutex was poisoned by a panic in another test, we can still
        // use the data. This allows tests to continue even after a panic.
        poisoned.into_inner()
    });
    let _scope = RuntimeLockGuard::enter();
    f(&mut guard)
}

#[cfg(test)]
pub(crate) fn runtime_lock_depth() -> usize {
    RUNTIME_LOCK_DEPTH.with(|cell| cell.get())
}

/// Allocate a new snapshot identifier and return it along with the
/// `invalid` set that should seed the snapshot.
pub(crate) fn allocate_snapshot() -> (SnapshotId, SnapshotIdSet) {
    with_runtime(|runtime| runtime.allocate_snapshot())
}

/// Mark a snapshot identifier as closed.
pub(crate) fn close_snapshot(id: SnapshotId) {
    with_runtime(|runtime| runtime.close_snapshot(id))
}

/// Allocate a fresh record identifier that does not correspond to an open snapshot.
pub(crate) fn allocate_record_id() -> SnapshotId {
    with_runtime(|runtime| runtime.allocate_record_id())
}

/// Get the next snapshot ID that will be allocated.
///
/// This does not increment the counter and is used for cleanup operations.
pub(crate) fn peek_next_snapshot_id() -> SnapshotId {
    with_runtime(|runtime| runtime.peek_next_snapshot_id())
}

/// Advance the global snapshot identifier and update the open set.
///
/// Returns the updated open snapshot set after the transition so callers can
/// refresh any cached invalid views.
pub(crate) fn advance_global_snapshot(new_id: SnapshotId) -> SnapshotIdSet {
    with_runtime(|runtime| runtime.advance_global_snapshot(new_id))
}

/// Snapshot of the currently open snapshot ids.
pub(crate) fn open_snapshots() -> SnapshotIdSet {
    with_runtime(|runtime| runtime.open_snapshots())
}

/// Reset runtime state for deterministic testing.
#[cfg(test)]
pub(crate) struct TestRuntimeGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
static TEST_RUNTIME_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[cfg(test)]
pub(crate) fn reset_runtime_for_tests() -> TestRuntimeGuard {
    // Handle poison errors - if a previous test panicked, we can still proceed
    let guard = TEST_RUNTIME_LOCK.lock().unwrap_or_else(|poisoned| {
        // Clear the poison by taking ownership of the guard
        poisoned.into_inner()
    });
    with_runtime(|runtime| runtime.reset_for_tests());
    super::clear_last_writes();
    super::global::clear_global_snapshot_for_tests();
    TestRuntimeGuard { _lock: guard }
}

/// Encapsulates global bookkeeping required by the snapshot runtime.
#[derive(Debug)]
pub(crate) struct SnapshotRuntime {
    /// The next snapshot id to hand out. Always strictly greater than any id
    /// that has been issued so far.
    next_snapshot_id: SnapshotId,
    /// Set of snapshots that are currently open. New snapshots treat these as
    /// invalid so they will not observe mutations performed by still-open
    /// writers.
    open_snapshots: SnapshotIdSet,
    /// The logical id of the global snapshot.
    global_snapshot_id: SnapshotId,
}

impl SnapshotRuntime {
    fn new() -> Self {
        let mut open = SnapshotIdSet::new();
        open = open.set(INITIAL_GLOBAL_SNAPSHOT_ID);
        Self {
            next_snapshot_id: INITIAL_GLOBAL_SNAPSHOT_ID + 1,
            open_snapshots: open,
            global_snapshot_id: INITIAL_GLOBAL_SNAPSHOT_ID,
        }
    }

    /// Returns the id assigned to the global snapshot.
    pub(crate) fn global_snapshot_id(&self) -> SnapshotId {
        self.global_snapshot_id
    }

    /// Returns the set of currently open snapshots.
    pub(crate) fn open_snapshots(&self) -> SnapshotIdSet {
        self.open_snapshots.clone()
    }

    /// Update the global snapshot id, adjusting the open set accordingly.
    pub(crate) fn advance_global_snapshot(&mut self, new_id: SnapshotId) -> SnapshotIdSet {
        let old_id = self.global_snapshot_id;
        if new_id <= old_id {
            let mut open = SnapshotIdSet::new();
            open = open.set(new_id);
            self.open_snapshots = open;
            self.global_snapshot_id = new_id;
            return self.open_snapshots.clone();
        }

        self.open_snapshots = self.open_snapshots.clear(old_id);
        self.open_snapshots = self.open_snapshots.set(new_id);
        self.global_snapshot_id = new_id;
        self.open_snapshots.clone()
    }

    /// Allocate a new snapshot identifier and mark it open.
    ///
    /// The returned tuple mirrors the information produced by the Kotlin
    /// runtime during `takeNewSnapshot`:
    /// - The freshly allocated snapshot id.
    /// - The `invalid` set to seed into the new snapshot (i.e. the open set
    ///   prior to inserting the newly allocated id).
    pub(crate) fn allocate_snapshot(&mut self) -> (SnapshotId, SnapshotIdSet) {
        let invalid = self.open_snapshots.clone();
        let id = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        self.open_snapshots = self.open_snapshots.set(id);
        (id, invalid)
    }

    /// Marks the given snapshot id as no longer open.
    pub(crate) fn close_snapshot(&mut self, id: SnapshotId) {
        self.open_snapshots = self.open_snapshots.clear(id);
    }

    pub(crate) fn allocate_record_id(&mut self) -> SnapshotId {
        let id = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        id
    }

    /// Get the next snapshot ID that will be allocated without incrementing the counter.
    ///
    /// This is used for cleanup operations to determine the reuse limit.
    /// Mirrors Kotlin's `nextSnapshotId` field access.
    pub(crate) fn peek_next_snapshot_id(&self) -> SnapshotId {
        self.next_snapshot_id
    }

    /// Reset the runtime to a clean state. This is primarily intended for
    /// tests so they can make deterministic assertions about snapshot ids.
    #[cfg(test)]
    pub(crate) fn reset_for_tests(&mut self) {
        *self = SnapshotRuntime::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state_marks_global_snapshot_open() {
        let _guard = reset_runtime_for_tests();
        with_runtime(|runtime| {
            assert_eq!(runtime.global_snapshot_id(), INITIAL_GLOBAL_SNAPSHOT_ID);
            assert!(runtime.open_snapshots().get(INITIAL_GLOBAL_SNAPSHOT_ID));
        });
    }

    #[test]
    fn test_allocate_snapshot_marks_it_open() {
        let _guard = reset_runtime_for_tests();
        let (id, invalid) = allocate_snapshot();
        assert!(invalid.get(INITIAL_GLOBAL_SNAPSHOT_ID));
        assert!(!invalid.get(id));
        with_runtime(|runtime| {
            assert!(runtime.open_snapshots().get(id));
        });
    }

    #[test]
    fn test_close_snapshot_clears_open_flag() {
        let _guard = reset_runtime_for_tests();
        let (id, _) = allocate_snapshot();
        with_runtime(|runtime| {
            assert!(runtime.open_snapshots().get(id));
        });
        close_snapshot(id);
        with_runtime(|runtime| {
            assert!(!runtime.open_snapshots().get(id));
        });
    }

    #[test]
    fn test_advance_global_snapshot_updates_open_set() {
        let _guard = reset_runtime_for_tests();
        let new_id = INITIAL_GLOBAL_SNAPSHOT_ID + 1;
        let open = advance_global_snapshot(new_id);
        assert!(open.get(new_id));
        assert!(!open.get(INITIAL_GLOBAL_SNAPSHOT_ID));
        with_runtime(|runtime| {
            assert_eq!(runtime.global_snapshot_id(), new_id);
            assert!(runtime.open_snapshots().get(new_id));
        });
    }
}
