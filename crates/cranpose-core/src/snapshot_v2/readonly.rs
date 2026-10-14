use super::*;

/// A read-only snapshot of state at a specific point in time.
///
/// This snapshot cannot be used to modify state. Any attempts to write
/// to state objects while this snapshot is active will fail.
///
/// # Thread Safety
/// Contains `Cell<T>` and `RefCell<T>` which are not `Send`/`Sync`. This is safe because
/// snapshots are stored in thread-local storage and never shared across threads. The `Rc`
/// is used for cheap cloning within a single thread, not for cross-thread sharing.
pub struct ReadonlySnapshot {
    pub(super) state: SnapshotState,
}

impl ReadonlySnapshot {
    /// Create a new read-only snapshot.
    pub fn new(
        id: SnapshotId,
        invalid: SnapshotIdSet,
        read_observer: Option<ReadObserver>,
    ) -> Rc<Self> {
        Rc::new(Self {
            state: SnapshotState::new(id, invalid, read_observer, None, false),
        })
    }

    /// Returns the id of this snapshot. Ids grow with every snapshot taken.
    pub fn snapshot_id(&self) -> SnapshotId {
        self.state.id.get()
    }

    /// Returns the ids of snapshots whose writes this snapshot does not see.
    pub fn invalid(&self) -> SnapshotIdSet {
        self.state.invalid.borrow().clone()
    }

    /// Returns `true`: this snapshot rejects writes.
    pub fn read_only(&self) -> bool {
        true
    }

    /// Returns a new read-only snapshot with the same id, invalid set and read observer.
    pub fn root_readonly(&self) -> Rc<Self> {
        ReadonlySnapshot::new(
            self.state.id.get(),
            self.state.invalid.borrow().clone(),
            self.state.read_observer.borrow().clone(),
        )
    }

    /// Runs `f` with this snapshot as the current snapshot of the thread and returns its result.
    /// The previous current snapshot is restored when `f` returns.
    pub fn enter<T>(&self, f: impl FnOnce() -> T) -> T {
        enter_snapshot_scope(AnySnapshot::Readonly(self.root_readonly()), f)
    }

    /// Takes another read-only snapshot with the same view of state. Reads inside the new snapshot
    /// call both `read_observer` and this snapshot's own read observer.
    pub fn take_nested_snapshot(&self, read_observer: Option<ReadObserver>) -> Rc<Self> {
        let merged_observer =
            merge_read_observers(read_observer, self.state.read_observer.borrow().clone());
        ReadonlySnapshot::new(
            self.state.id.get(),
            self.state.invalid.borrow().clone(),
            merged_observer,
        )
    }

    /// Returns `false`: a read-only snapshot has no writes.
    pub fn has_pending_changes(&self) -> bool {
        false
    }

    /// Releases the snapshot. Call it once the snapshot is no longer needed.
    pub fn dispose(&self) {
        self.state.dispose();
    }

    /// Calls the read observer, if any, with `state`.
    pub fn record_read(&self, state: &dyn StateObject) {
        self.state.record_read(state);
    }

    /// Always panics: a read-only snapshot rejects writes.
    pub fn record_write(&self, _state: Rc<dyn StateObject>) {
        panic!("Cannot write to a read-only snapshot");
    }

    /// Returns `true` once the snapshot was disposed.
    pub fn is_disposed(&self) -> bool {
        self.state.disposed.get()
    }

    pub(crate) fn set_on_dispose<F>(&self, f: F)
    where
        F: FnOnce() + 'static,
    {
        self.state.set_on_dispose(f);
    }
}

#[cfg(test)]
#[path = "tests/readonly_tests.rs"]
mod tests;
