use super::*;

/// A nested read-only snapshot.
///
/// This is a read-only snapshot that has a parent snapshot. It inherits
/// the parent's invalid set and can be disposed independently.
///
/// # Thread Safety
/// Contains `Cell<T>` and `RefCell<T>` which are not `Send`/`Sync`. This is safe because
/// snapshots are stored in thread-local storage and never shared across threads. The `Rc`
/// is used for cheap cloning within a single thread, not for cross-thread sharing.
pub struct NestedReadonlySnapshot {
    pub(super) state: SnapshotState,
    parent: Weak<NestedReadonlySnapshot>,
}

impl NestedReadonlySnapshot {
    /// Creates a nested read-only snapshot with the given id and invalid set. `read_observer` is
    /// called for reads inside it. `parent` is the snapshot it was taken from, if that snapshot is
    /// still alive.
    pub fn new(
        id: SnapshotId,
        invalid: SnapshotIdSet,
        read_observer: Option<ReadObserver>,
        parent: Weak<NestedReadonlySnapshot>,
    ) -> Rc<Self> {
        Rc::new(Self {
            state: SnapshotState::new(id, invalid, read_observer, None, false),
            parent,
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

    /// Returns the outermost parent that is still alive, or a copy of this snapshot when it has no
    /// live parent.
    pub fn root_nested_readonly(&self) -> Rc<NestedReadonlySnapshot> {
        if let Some(parent) = self.parent.upgrade() {
            parent.root_nested_readonly()
        } else {
            NestedReadonlySnapshot::new(
                self.state.id.get(),
                self.state.invalid.borrow().clone(),
                self.state.read_observer.borrow().clone(),
                Weak::new(),
            )
        }
    }

    /// Runs `f` with this snapshot as the current snapshot of the thread and returns its result.
    /// The previous current snapshot is restored when `f` returns.
    pub fn enter<T>(self: &Rc<Self>, f: impl FnOnce() -> T) -> T {
        enter_snapshot_scope(AnySnapshot::NestedReadonly(self.clone()), f)
    }

    /// Takes another read-only snapshot with the same view of state. Reads inside the new snapshot
    /// call both `read_observer` and this snapshot's own read observer.
    pub fn take_nested_snapshot(
        &self,
        read_observer: Option<ReadObserver>,
    ) -> Rc<NestedReadonlySnapshot> {
        let merged_observer =
            merge_read_observers(read_observer, self.state.read_observer.borrow().clone());

        NestedReadonlySnapshot::new(
            self.state.id.get(),
            self.state.invalid.borrow().clone(),
            merged_observer,
            self.parent.clone(),
        )
    }

    /// Returns `false`: a read-only snapshot has no writes.
    pub fn has_pending_changes(&self) -> bool {
        false
    }

    /// Releases the snapshot. Does nothing if it was already disposed.
    pub fn dispose(&self) {
        if !self.state.disposed.get() {
            self.state.dispose();
        }
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
}

/// A nested mutable snapshot.
///
/// This is a mutable snapshot that has a parent. Changes made in this
/// snapshot are applied to the parent when `apply()` is called, not
/// to the global snapshot.
///
/// # Thread Safety
/// Contains `Cell<T>` and `RefCell<T>` which are not `Send`/`Sync`. This is safe because
/// snapshots are stored in thread-local storage and never shared across threads. The `Rc`
/// is used for cheap cloning within a single thread, not for cross-thread sharing.
pub struct NestedMutableSnapshot {
    pub(super) state: SnapshotState,
    parent: Weak<MutableSnapshot>,
    nested_count: Cell<usize>,
    applied: Cell<bool>,
    base_parent_id: SnapshotId,
}

impl NestedMutableSnapshot {
    /// Creates a nested mutable snapshot with the given id and invalid set. `parent` receives the
    /// writes on `apply`. `base_parent_id` is the id of the parent when this snapshot was taken.
    pub fn new(
        id: SnapshotId,
        invalid: SnapshotIdSet,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
        parent: Weak<MutableSnapshot>,
        base_parent_id: SnapshotId,
    ) -> Rc<Self> {
        Rc::new(Self {
            state: SnapshotState::new(id, invalid, read_observer, write_observer, true),
            parent,
            nested_count: Cell::new(0),
            applied: Cell::new(false),
            base_parent_id,
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

    /// Returns `false`: this snapshot accepts writes.
    pub fn read_only(&self) -> bool {
        false
    }

    pub(crate) fn set_on_dispose<F>(&self, f: F)
    where
        F: FnOnce() + 'static,
    {
        self.state.set_on_dispose(f);
    }

    /// Returns the root mutable snapshot above this one. If the root is gone, returns a new root
    /// snapshot built from this snapshot's id, invalid set and observers.
    pub fn root_mutable(&self) -> Rc<MutableSnapshot> {
        if let Some(parent) = self.parent.upgrade() {
            parent.root_mutable()
        } else {
            MutableSnapshot::new(
                self.state.id.get(),
                self.state.invalid.borrow().clone(),
                self.state.read_observer.borrow().clone(),
                self.state.write_observer.borrow().clone(),
                self.base_parent_id,
            )
        }
    }

    /// Runs `f` with this snapshot as the current snapshot of the thread and returns its result.
    /// The previous current snapshot is restored when `f` returns.
    pub fn enter<T>(self: &Rc<Self>, f: impl FnOnce() -> T) -> T {
        enter_snapshot_scope(AnySnapshot::NestedMutable(self.clone()), f)
    }

    /// Takes a read-only snapshot of this snapshot's view of state. Reads inside the new snapshot
    /// call both `read_observer` and this snapshot's own read observer.
    pub fn take_nested_snapshot(
        &self,
        read_observer: Option<ReadObserver>,
    ) -> Rc<ReadonlySnapshot> {
        let merged_observer =
            merge_read_observers(read_observer, self.state.read_observer.borrow().clone());

        ReadonlySnapshot::new(
            self.state.id.get(),
            self.state.invalid.borrow().clone(),
            merged_observer,
        )
    }

    /// Returns `true` when this snapshot recorded writes that were not applied yet.
    pub fn has_pending_changes(&self) -> bool {
        !self.state.modified.borrow().is_empty()
    }

    /// Returns the ids of nested mutable snapshots taken from this snapshot and not yet disposed.
    pub fn pending_children(&self) -> Vec<SnapshotId> {
        self.state.pending_children()
    }

    /// Returns `true` when a nested mutable snapshot taken from this snapshot is still open.
    pub fn has_pending_children(&self) -> bool {
        self.state.has_pending_children()
    }

    /// Discards the writes that were not applied and releases the snapshot. Does nothing while
    /// nested snapshots taken from it are still open.
    pub fn dispose(&self) {
        if !self.state.disposed.get() && self.nested_count.get() == 0 {
            self.state.dispose();
        }
    }

    /// Calls the read observer, if any, with `state`.
    pub fn record_read(&self, state: &dyn StateObject) {
        self.state.record_read(state);
    }

    /// Records `state` as written in this snapshot. The write observer, if any, is called the first
    /// time a state object is recorded. Panics if the snapshot was applied or disposed.
    pub fn record_write(&self, state: Rc<dyn StateObject>) {
        assert!(!self.applied.get(), "Cannot write to an applied snapshot");
        assert!(
            !self.state.disposed.get(),
            "Cannot write to a disposed snapshot"
        );
        self.state.record_write(state, self.state.id.get());
    }

    /// Returns `true` once the snapshot was disposed.
    pub fn is_disposed(&self) -> bool {
        self.state.disposed.get()
    }

    /// Moves the writes of this snapshot into its parent. Returns `Failure` if the snapshot was
    /// already applied or disposed, if the parent is gone, or if the parent wrote one of the same
    /// state objects.
    pub fn apply(&self) -> SnapshotApplyResult {
        if self.state.disposed.get() {
            return SnapshotApplyResult::Failure;
        }

        if self.applied.get() {
            return SnapshotApplyResult::Failure;
        }

        if let Some(parent) = self.parent.upgrade() {
            let child_modified = self.state.modified.borrow();
            if child_modified.is_empty() {
                self.applied.set(true);
                self.state.dispose();
                return SnapshotApplyResult::Success;
            }
            if parent.merge_child_modifications(&child_modified).is_err() {
                return SnapshotApplyResult::Failure;
            }

            self.applied.set(true);
            self.state.dispose();
            SnapshotApplyResult::Success
        } else {
            SnapshotApplyResult::Failure
        }
    }

    /// Takes a mutable snapshot nested in this one. Its `apply` moves the writes into this
    /// snapshot.
    pub fn take_nested_mutable_snapshot(
        self: &Rc<Self>,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
    ) -> Rc<NestedMutableSnapshot> {
        let root = Rc::downgrade(&self.root_mutable());
        allocate_nested_mutable_snapshot(self, root, read_observer, write_observer)
    }
}

impl NestedMutableHost for NestedMutableSnapshot {
    fn snapshot_state(&self) -> &SnapshotState {
        &self.state
    }

    fn nested_count(&self) -> &Cell<usize> {
        &self.nested_count
    }
}

#[cfg(test)]
#[path = "tests/nested_tests.rs"]
mod tests;
