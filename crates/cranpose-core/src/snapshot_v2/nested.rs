use super::*;

/// A nested read-only snapshot.
///
/// This is a read-only snapshot that has a parent snapshot. It inherits
/// the parent's invalid set and can be disposed independently.
///
/// # Thread Safety
/// Contains `Cell<T>` and `RefCell<T>` which are not `Send`/`Sync`. This is safe because
/// snapshots are stored in thread-local storage and never shared across threads. The `Arc`
/// is used for cheap cloning within a single thread, not for cross-thread sharing.
pub struct NestedReadonlySnapshot {
    state: SnapshotState,
    parent: Weak<NestedReadonlySnapshot>,
}

impl NestedReadonlySnapshot {
    pub fn new(
        id: SnapshotId,
        invalid: SnapshotIdSet,
        read_observer: Option<ReadObserver>,
        parent: Weak<NestedReadonlySnapshot>,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: SnapshotState::new(id, invalid, read_observer, None, false),
            parent,
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

    pub fn root_nested_readonly(&self) -> Arc<NestedReadonlySnapshot> {
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

    pub fn enter<T>(self: &Arc<Self>, f: impl FnOnce() -> T) -> T {
        enter_snapshot_scope(AnySnapshot::NestedReadonly(self.clone()), f)
    }

    pub fn take_nested_snapshot(
        &self,
        read_observer: Option<ReadObserver>,
    ) -> Arc<NestedReadonlySnapshot> {
        let merged_observer =
            merge_read_observers(read_observer, self.state.read_observer.borrow().clone());

        NestedReadonlySnapshot::new(
            self.state.id.get(),
            self.state.invalid.borrow().clone(),
            merged_observer,
            self.parent.clone(),
        )
    }

    pub fn has_pending_changes(&self) -> bool {
        false
    }

    pub fn dispose(&self) {
        if !self.state.disposed.get() {
            self.state.dispose();
        }
    }

    pub fn record_read(&self, state: &dyn StateObject) {
        self.state.record_read(state);
    }

    pub fn record_write(&self, _state: Arc<dyn StateObject>) {
        panic!("Cannot write to a read-only snapshot");
    }

    pub fn close(&self) {
        self.state.disposed.set(true);
    }

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
/// snapshots are stored in thread-local storage and never shared across threads. The `Arc`
/// is used for cheap cloning within a single thread, not for cross-thread sharing.
pub struct NestedMutableSnapshot {
    state: SnapshotState,
    parent: Weak<MutableSnapshot>,
    nested_count: Cell<usize>,
    applied: Cell<bool>,
    base_parent_id: SnapshotId,
}

impl NestedMutableSnapshot {
    pub fn new(
        id: SnapshotId,
        invalid: SnapshotIdSet,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
        parent: Weak<MutableSnapshot>,
        base_parent_id: SnapshotId,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: SnapshotState::new(id, invalid, read_observer, write_observer, true),
            parent,
            nested_count: Cell::new(0),
            applied: Cell::new(false),
            base_parent_id,
        })
    }

    pub fn snapshot_id(&self) -> SnapshotId {
        self.state.id.get()
    }

    pub fn invalid(&self) -> SnapshotIdSet {
        self.state.invalid.borrow().clone()
    }

    pub fn read_only(&self) -> bool {
        false
    }

    pub(crate) fn set_on_dispose<F>(&self, f: F)
    where
        F: FnOnce() + 'static,
    {
        self.state.set_on_dispose(f);
    }

    pub fn root_mutable(&self) -> Arc<MutableSnapshot> {
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

    pub fn enter<T>(self: &Arc<Self>, f: impl FnOnce() -> T) -> T {
        enter_snapshot_scope(AnySnapshot::NestedMutable(self.clone()), f)
    }

    pub fn take_nested_snapshot(
        &self,
        read_observer: Option<ReadObserver>,
    ) -> Arc<ReadonlySnapshot> {
        let merged_observer =
            merge_read_observers(read_observer, self.state.read_observer.borrow().clone());

        ReadonlySnapshot::new(
            self.state.id.get(),
            self.state.invalid.borrow().clone(),
            merged_observer,
        )
    }

    pub fn has_pending_changes(&self) -> bool {
        !self.state.modified.borrow().is_empty()
    }

    pub fn pending_children(&self) -> Vec<SnapshotId> {
        self.state.pending_children()
    }

    pub fn has_pending_children(&self) -> bool {
        self.state.has_pending_children()
    }

    pub fn dispose(&self) {
        if !self.state.disposed.get() && self.nested_count.get() == 0 {
            self.state.dispose();
        }
    }

    pub fn record_read(&self, state: &dyn StateObject) {
        self.state.record_read(state);
    }

    pub fn record_write(&self, state: Arc<dyn StateObject>) {
        assert!(!self.applied.get(), "Cannot write to an applied snapshot");
        assert!(
            !self.state.disposed.get(),
            "Cannot write to a disposed snapshot"
        );
        self.state.record_write(state, self.state.id.get());
    }

    pub fn close(&self) {
        self.state.disposed.set(true);
    }

    pub fn is_disposed(&self) -> bool {
        self.state.disposed.get()
    }

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

    pub fn take_nested_mutable_snapshot(
        self: &Arc<Self>,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
    ) -> Arc<NestedMutableSnapshot> {
        let root = Arc::downgrade(&self.root_mutable());
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
