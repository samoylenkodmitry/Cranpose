use super::*;

/// A read-only snapshot of state at a specific point in time.
///
/// This snapshot cannot be used to modify state. Any attempts to write
/// to state objects while this snapshot is active will fail.
///
/// # Thread Safety
/// Contains `Cell<T>` and `RefCell<T>` which are not `Send`/`Sync`. This is safe because
/// snapshots are stored in thread-local storage and never shared across threads. The `Arc`
/// is used for cheap cloning within a single thread, not for cross-thread sharing.
pub struct ReadonlySnapshot {
    state: SnapshotState,
}

impl ReadonlySnapshot {
    /// Create a new read-only snapshot.
    pub fn new(
        id: SnapshotId,
        invalid: SnapshotIdSet,
        read_observer: Option<ReadObserver>,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: SnapshotState::new(id, invalid, read_observer, None, false),
        })
    }

    pub fn snapshot_id(&self) -> SnapshotId {
        self.state.id.get()
    }

    pub fn invalid(&self) -> SnapshotIdSet {
        self.state.invalid.borrow().clone()
    }

    pub fn read_only(&self) -> bool {
        true
    }

    pub fn root_readonly(&self) -> Arc<Self> {
        ReadonlySnapshot::new(
            self.state.id.get(),
            self.state.invalid.borrow().clone(),
            self.state.read_observer.borrow().clone(),
        )
    }

    pub fn enter<T>(&self, f: impl FnOnce() -> T) -> T {
        enter_snapshot_scope(AnySnapshot::Readonly(self.root_readonly()), f)
    }

    pub fn take_nested_snapshot(&self, read_observer: Option<ReadObserver>) -> Arc<Self> {
        let merged_observer =
            merge_read_observers(read_observer, self.state.read_observer.borrow().clone());
        ReadonlySnapshot::new(
            self.state.id.get(),
            self.state.invalid.borrow().clone(),
            merged_observer,
        )
    }

    pub fn has_pending_changes(&self) -> bool {
        false
    }

    pub fn dispose(&self) {
        self.state.dispose();
    }

    pub fn record_read(&self, state: &dyn StateObject) {
        self.state.record_read(state);
    }

    pub fn record_write(&self, _state: Arc<dyn StateObject>) {
        panic!("Cannot write to a read-only snapshot");
    }

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
