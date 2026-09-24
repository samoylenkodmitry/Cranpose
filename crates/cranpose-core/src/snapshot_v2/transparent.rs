use super::*;

/// A transparent mutable snapshot that allows observer replacement.
///
/// This snapshot type is optimized for cases where observers need to be
/// temporarily added or removed without creating a new snapshot structure.
///
/// # Thread Safety
/// Contains `Cell<T>` and `RefCell<T>` which are not `Send`/`Sync`. This is safe because
/// snapshots are stored in thread-local storage and never shared across threads. The `Arc`
/// is used for cheap cloning within a single thread, not for cross-thread sharing.
pub struct TransparentObserverMutableSnapshot {
    state: SnapshotState,
    parent: Option<Weak<TransparentObserverMutableSnapshot>>,
    nested_count: Cell<usize>,
    applied: Cell<bool>,
    reusable: Cell<bool>,
}

impl TransparentObserverMutableSnapshot {
    pub fn new(
        id: SnapshotId,
        invalid: SnapshotIdSet,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
        parent: Option<Weak<TransparentObserverMutableSnapshot>>,
    ) -> Arc<Self> {
        Self::new_reusing(None, id, invalid, read_observer, write_observer, parent)
    }

    pub(crate) fn new_reusing(
        recycled: Option<Arc<Self>>,
        id: SnapshotId,
        invalid: SnapshotIdSet,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
        parent: Option<Weak<Self>>,
    ) -> Arc<Self> {
        let fresh = Self {
            state: SnapshotState::new_with_pinning(
                id,
                invalid,
                read_observer,
                write_observer,
                false,
                false,
            ),
            parent,
            nested_count: Cell::new(0),
            applied: Cell::new(false),
            reusable: Cell::new(true),
        };
        if let Some(mut recycled) = recycled
            && let Some(target) = Arc::get_mut(&mut recycled)
        {
            *target = fresh;
            recycled
        } else {
            Arc::new(fresh)
        }
    }

    /// Check if this snapshot can be reused for observer changes.
    pub fn can_reuse(&self) -> bool {
        self.reusable.get()
    }

    pub(crate) fn observers(&self) -> (Option<ReadObserver>, Option<WriteObserver>) {
        (
            self.state.read_observer.borrow().clone(),
            self.state.write_observer.borrow().clone(),
        )
    }

    /// Set the read observer (only allowed if reusable).
    pub fn set_read_observer(&self, observer: Option<ReadObserver>) {
        assert!(
            self.can_reuse(),
            "Cannot change observers on non-reusable snapshot"
        );
        *self.state.read_observer.borrow_mut() = observer;
    }

    /// Set the write observer (only allowed if reusable).
    pub fn set_write_observer(&self, observer: Option<WriteObserver>) {
        assert!(
            self.can_reuse(),
            "Cannot change observers on non-reusable snapshot"
        );
        *self.state.write_observer.borrow_mut() = observer;
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

    pub fn root_transparent_mutable(self: &Arc<Self>) -> Arc<Self> {
        match &self.parent {
            Some(weak) => weak
                .upgrade()
                .map_or_else(|| self.clone(), |parent| parent.root_transparent_mutable()),
            None => self.clone(),
        }
    }

    pub fn enter<T>(self: &Arc<Self>, f: impl FnOnce() -> T) -> T {
        let prev = current_snapshot();

        if let Some(ref snapshot) = prev
            && snapshot.is_same_transparent(self)
        {
            return f();
        }

        enter_snapshot_scope(AnySnapshot::TransparentMutable(self.clone()), f)
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
        self.state.record_write(state, self.state.id.get());
    }

    pub fn close(&self) {
        self.state.disposed.set(true);
    }

    pub fn is_disposed(&self) -> bool {
        self.state.disposed.get()
    }

    pub fn apply(&self) -> SnapshotApplyResult {
        if self.state.disposed.get() || self.applied.get() {
            return SnapshotApplyResult::Failure;
        }

        self.applied.set(true);
        SnapshotApplyResult::Success
    }

    pub fn take_nested_mutable_snapshot(
        &self,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
    ) -> Arc<TransparentObserverMutableSnapshot> {
        let merged_read =
            merge_read_observers(read_observer, self.state.read_observer.borrow().clone());
        let merged_write =
            merge_write_observers(write_observer, self.state.write_observer.borrow().clone());

        let mut invalid = self.state.invalid.borrow().clone();
        let new_id = self.state.id.get() + 1;
        invalid = invalid.set(new_id);

        TransparentObserverMutableSnapshot::new(
            new_id,
            invalid,
            merged_read,
            merged_write,
            self.parent.clone(),
        )
    }
}

/// A transparent read-only snapshot.
///
/// Similar to TransparentObserverMutableSnapshot but for read-only snapshots.
///
/// # Thread Safety
/// Contains `Cell<T>` and `RefCell<T>` which are not `Send`/`Sync`. This is safe because
/// snapshots are stored in thread-local storage and never shared across threads. The `Arc`
/// is used for cheap cloning within a single thread, not for cross-thread sharing.
pub struct TransparentObserverSnapshot {
    state: SnapshotState,
    parent: Option<Weak<TransparentObserverSnapshot>>,
    reusable: Cell<bool>,
}

impl TransparentObserverSnapshot {
    pub fn new(
        id: SnapshotId,
        invalid: SnapshotIdSet,
        read_observer: Option<ReadObserver>,
        parent: Option<Weak<TransparentObserverSnapshot>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: SnapshotState::new_with_pinning(id, invalid, read_observer, None, false, false),
            parent,
            reusable: Cell::new(true),
        })
    }

    /// Check if this snapshot can be reused for observer changes.
    pub fn can_reuse(&self) -> bool {
        self.reusable.get()
    }

    /// Set the read observer (only allowed if reusable).
    pub fn set_read_observer(&self, observer: Option<ReadObserver>) {
        assert!(
            self.can_reuse(),
            "Cannot change observers on non-reusable snapshot"
        );
        *self.state.read_observer.borrow_mut() = observer;
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

    pub fn root_transparent_readonly(self: &Arc<Self>) -> Arc<Self> {
        match &self.parent {
            Some(weak) => weak
                .upgrade()
                .map_or_else(|| self.clone(), |parent| parent.root_transparent_readonly()),
            None => self.clone(),
        }
    }

    pub fn enter<T>(self: &Arc<Self>, f: impl FnOnce() -> T) -> T {
        let previous = current_snapshot();

        if let Some(ref prev_snapshot) = previous
            && prev_snapshot.is_same_transparent_readonly(self)
        {
            return f();
        }

        enter_snapshot_scope(AnySnapshot::TransparentReadonly(self.clone()), f)
    }

    pub fn take_nested_snapshot(
        &self,
        read_observer: Option<ReadObserver>,
    ) -> Arc<TransparentObserverSnapshot> {
        let merged_observer =
            merge_read_observers(read_observer, self.state.read_observer.borrow().clone());
        TransparentObserverSnapshot::new(
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
        self.state.dispose();
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

#[cfg(test)]
#[path = "tests/transparent_tests.rs"]
mod tests;
