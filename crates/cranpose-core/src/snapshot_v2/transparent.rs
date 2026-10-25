use super::*;

/// A transparent mutable snapshot that allows observer replacement.
///
/// This snapshot type is optimized for cases where observers need to be
/// temporarily added or removed without creating a new snapshot structure.
///
/// # Thread Safety
/// Contains `Cell<T>` and `RefCell<T>` which are not `Send`/`Sync`. This is safe because
/// snapshots are stored in thread-local storage and never shared across threads. The `Rc`
/// is used for cheap cloning within a single thread, not for cross-thread sharing.
pub struct TransparentObserverMutableSnapshot {
    pub(super) state: SnapshotState,
    parent: Option<Weak<Self>>,
    nested_count: Cell<usize>,
    applied: Cell<bool>,
    reusable: Cell<bool>,
}

impl TransparentObserverMutableSnapshot {
    /// Creates a transparent mutable snapshot with the given id and invalid set. The observers are
    /// called for reads and writes inside it and can be replaced later. `parent` is the snapshot it
    /// was taken from, if that snapshot is still alive.
    pub fn new(
        id: SnapshotId,
        invalid: SnapshotIdSet,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
        parent: Option<Weak<Self>>,
    ) -> Rc<Self> {
        Self::new_reusing(None, id, invalid, read_observer, write_observer, parent)
    }

    pub(crate) fn new_reusing(
        recycled: Option<Rc<Self>>,
        id: SnapshotId,
        invalid: SnapshotIdSet,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
        parent: Option<Weak<Self>>,
    ) -> Rc<Self> {
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
            && let Some(target) = Rc::get_mut(&mut recycled)
        {
            *target = fresh;
            recycled
        } else {
            Rc::new(fresh)
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

    pub(crate) fn reads_into(&self, observer: &ReadObserver) -> bool {
        self.state
            .read_observer
            .borrow()
            .as_ref()
            .is_some_and(|installed| Rc::ptr_eq(installed, observer))
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

    /// Returns the outermost parent that is still alive, or this snapshot when it has none.
    pub fn root_transparent_mutable(self: &Rc<Self>) -> Rc<Self> {
        match &self.parent {
            Some(weak) => weak.upgrade().map_or_else(
                || Rc::clone(self),
                |parent| parent.root_transparent_mutable(),
            ),
            None => Rc::clone(self),
        }
    }

    /// Runs `f` with this snapshot as the current snapshot of the thread and returns its result. If
    /// this snapshot is already current, `f` runs directly. The previous current snapshot is
    /// restored when `f` returns.
    pub fn enter<T>(self: &Rc<Self>, f: impl FnOnce() -> T) -> T {
        let prev = current_snapshot();

        if let Some(ref snapshot) = prev
            && snapshot.is_same_transparent(self)
        {
            return f();
        }

        enter_snapshot_scope(AnySnapshot::TransparentMutable(Rc::clone(self)), f)
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

    /// Releases the snapshot. Does nothing if it was already disposed or if nested snapshots taken
    /// from it are still open.
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
    /// time a state object is recorded. Panics if the snapshot was applied.
    pub fn record_write(&self, state: Rc<dyn StateObject>) {
        assert!(!self.applied.get(), "Cannot write to an applied snapshot");
        self.state.record_write(state, self.state.id.get());
    }

    /// Returns `true` once the snapshot was disposed.
    pub fn is_disposed(&self) -> bool {
        self.state.disposed.get()
    }

    /// Marks the snapshot as applied. Returns `Failure` if it was already applied or disposed.
    pub fn apply(&self) -> SnapshotApplyResult {
        if self.state.disposed.get() || self.applied.get() {
            return SnapshotApplyResult::Failure;
        }

        self.applied.set(true);
        SnapshotApplyResult::Success
    }

    /// Takes a transparent mutable snapshot nested in this one. Reads and writes inside it call
    /// both the given observers and this snapshot's own observers.
    pub fn take_nested_mutable_snapshot(
        &self,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
    ) -> Rc<Self> {
        let merged_read =
            merge_read_observers(read_observer, self.state.read_observer.borrow().clone());
        let merged_write =
            merge_write_observers(write_observer, self.state.write_observer.borrow().clone());

        let mut invalid = self.state.invalid.borrow().clone();
        let new_id = self.state.id.get() + 1;
        invalid = invalid.set(new_id);

        Self::new(
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
/// snapshots are stored in thread-local storage and never shared across threads. The `Rc`
/// is used for cheap cloning within a single thread, not for cross-thread sharing.
pub struct TransparentObserverSnapshot {
    pub(super) state: SnapshotState,
    parent: Option<Weak<Self>>,
    reusable: Cell<bool>,
}

impl TransparentObserverSnapshot {
    /// Creates a transparent read-only snapshot with the given id and invalid set. `read_observer`
    /// is called for reads inside it and can be replaced later. `parent` is the snapshot it was
    /// taken from, if that snapshot is still alive.
    pub fn new(
        id: SnapshotId,
        invalid: SnapshotIdSet,
        read_observer: Option<ReadObserver>,
        parent: Option<Weak<Self>>,
    ) -> Rc<Self> {
        Rc::new(Self {
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

    /// Returns the outermost parent that is still alive, or this snapshot when it has none.
    pub fn root_transparent_readonly(self: &Rc<Self>) -> Rc<Self> {
        match &self.parent {
            Some(weak) => weak.upgrade().map_or_else(
                || Rc::clone(self),
                |parent| parent.root_transparent_readonly(),
            ),
            None => Rc::clone(self),
        }
    }

    /// Runs `f` with this snapshot as the current snapshot of the thread and returns its result. If
    /// this snapshot is already current, `f` runs directly. The previous current snapshot is
    /// restored when `f` returns.
    pub fn enter<T>(self: &Rc<Self>, f: impl FnOnce() -> T) -> T {
        let previous = current_snapshot();

        if let Some(ref prev_snapshot) = previous
            && prev_snapshot.is_same_transparent_readonly(self)
        {
            return f();
        }

        enter_snapshot_scope(AnySnapshot::TransparentReadonly(Rc::clone(self)), f)
    }

    /// Takes another transparent read-only snapshot with the same view of state. Reads inside the
    /// new snapshot call both `read_observer` and this snapshot's own read observer.
    pub fn take_nested_snapshot(&self, read_observer: Option<ReadObserver>) -> Rc<Self> {
        let merged_observer =
            merge_read_observers(read_observer, self.state.read_observer.borrow().clone());
        Self::new(
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
}

#[cfg(test)]
#[path = "tests/transparent_tests.rs"]
mod tests;
