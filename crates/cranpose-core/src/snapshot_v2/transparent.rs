//! Transparent observer snapshot implementations.
//!
//! These snapshots are optimized for observer chaining, allowing observers
//! to be temporarily changed without allocating new snapshot structures.

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
#[allow(clippy::arc_with_non_send_sync)]
pub struct TransparentObserverMutableSnapshot {
    state: SnapshotState,
    parent: Option<Weak<TransparentObserverMutableSnapshot>>,
    nested_count: Cell<usize>,
    applied: Cell<bool>,
    /// Whether this snapshot can be reused for observer changes
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
        Arc::new(Self {
            // Transparent snapshots don't allocate new IDs, so they shouldn't pin
            // to prevent garbage collection of old records
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
        })
    }

    /// Check if this snapshot can be reused for observer changes.
    pub fn can_reuse(&self) -> bool {
        self.reusable.get()
    }

    /// Set the read observer (only allowed if reusable).
    pub fn set_read_observer(&self, observer: Option<ReadObserver>) {
        if !self.can_reuse() {
            panic!("Cannot change observers on non-reusable snapshot");
        }
        *self.state.read_observer.borrow_mut() = observer;
    }

    /// Set the write observer (only allowed if reusable).
    pub fn set_write_observer(&self, observer: Option<WriteObserver>) {
        if !self.can_reuse() {
            panic!("Cannot change observers on non-reusable snapshot");
        }
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
                .map(|parent| parent.root_transparent_mutable())
                .unwrap_or_else(|| self.clone()),
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
        if self.applied.get() {
            panic!("Cannot write to an applied snapshot");
        }
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
#[allow(clippy::arc_with_non_send_sync)]
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
            // Transparent snapshots don't allocate new IDs, so they shouldn't pin
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
        if !self.can_reuse() {
            panic!("Cannot change observers on non-reusable snapshot");
        }
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
                .map(|parent| parent.root_transparent_readonly())
                .unwrap_or_else(|| self.clone()),
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
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::{
        snapshot_v2::runtime::TestRuntimeGuard,
        state::{ObjectId, PREEXISTING_SNAPSHOT_ID, StateObject, StateRecord},
    };

    fn reset_runtime() -> TestRuntimeGuard {
        reset_runtime_for_tests()
    }

    fn mock_state_record() -> Rc<StateRecord> {
        StateRecord::new(PREEXISTING_SNAPSHOT_ID, (), None)
    }

    struct MockState(usize);

    impl StateObject for MockState {
        fn object_id(&self) -> ObjectId {
            ObjectId(self.0)
        }

        fn first_record(&self) -> Rc<StateRecord> {
            mock_state_record()
        }

        fn try_readable_record(
            &self,
            snapshot_id: SnapshotId,
            invalid: &SnapshotIdSet,
        ) -> Option<Rc<StateRecord>> {
            Some(self.readable_record(snapshot_id, invalid))
        }

        fn readable_record(
            &self,
            _snapshot_id: SnapshotId,
            _invalid: &SnapshotIdSet,
        ) -> Rc<StateRecord> {
            mock_state_record()
        }

        fn prepend_state_record(&self, _record: Rc<StateRecord>) {}

        fn promote_record(&self, _child_id: SnapshotId) -> Result<(), &'static str> {
            Ok(())
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_transparent_observer_mutable_snapshot() {
        let _guard = reset_runtime();
        let snapshot =
            TransparentObserverMutableSnapshot::new(1, SnapshotIdSet::new(), None, None, None);

        assert_eq!(snapshot.snapshot_id(), 1);
        assert!(!snapshot.read_only());
        assert!(snapshot.can_reuse());
    }

    #[test]
    fn test_transparent_observer_mutable_apply() {
        let _guard = reset_runtime();
        let snapshot =
            TransparentObserverMutableSnapshot::new(1, SnapshotIdSet::new(), None, None, None);

        let result = snapshot.apply();
        assert!(result.is_success());
    }

    #[test]
    fn test_transparent_observer_snapshot() {
        let _guard = reset_runtime();
        let snapshot = TransparentObserverSnapshot::new(1, SnapshotIdSet::new(), None, None);

        assert_eq!(snapshot.snapshot_id(), 1);
        assert!(snapshot.read_only());
        assert!(snapshot.can_reuse());
    }

    #[test]
    #[should_panic(expected = "Cannot write to a read-only snapshot")]
    fn test_transparent_observer_snapshot_write_panics() {
        let _guard = reset_runtime();

        let snapshot = TransparentObserverSnapshot::new(1, SnapshotIdSet::new(), None, None);

        let mock_state = Arc::new(MockState(0));
        snapshot.record_write(mock_state);
    }

    #[test]
    fn transparent_mutable_set_read_observer_replaces_observer() {
        let _guard = reset_runtime();
        let initial_reads = Rc::new(Cell::new(0));
        let replacement_reads = Rc::new(Cell::new(0));
        let snapshot = TransparentObserverMutableSnapshot::new(
            1,
            SnapshotIdSet::new(),
            Some(Arc::new({
                let initial_reads = Rc::clone(&initial_reads);
                move |_| initial_reads.set(initial_reads.get() + 1)
            })),
            None,
            None,
        );

        snapshot.set_read_observer(Some(Arc::new({
            let replacement_reads = Rc::clone(&replacement_reads);
            move |_| replacement_reads.set(replacement_reads.get() + 1)
        })));
        snapshot.record_read(&MockState(1));

        assert_eq!(initial_reads.get(), 0);
        assert_eq!(replacement_reads.get(), 1);
    }

    #[test]
    fn transparent_mutable_set_write_observer_replaces_observer() {
        let _guard = reset_runtime();
        let initial_writes = Rc::new(Cell::new(0));
        let replacement_writes = Rc::new(Cell::new(0));
        let snapshot = TransparentObserverMutableSnapshot::new(
            1,
            SnapshotIdSet::new(),
            None,
            Some(Arc::new({
                let initial_writes = Rc::clone(&initial_writes);
                move |_| initial_writes.set(initial_writes.get() + 1)
            })),
            None,
        );

        snapshot.set_write_observer(Some(Arc::new({
            let replacement_writes = Rc::clone(&replacement_writes);
            move |_| replacement_writes.set(replacement_writes.get() + 1)
        })));
        snapshot.record_write(Arc::new(MockState(2)));

        assert_eq!(initial_writes.get(), 0);
        assert_eq!(replacement_writes.get(), 1);
    }

    #[test]
    fn transparent_mutable_nested_snapshot_inherits_replaced_observers() {
        let _guard = reset_runtime();
        let parent_reads = Rc::new(Cell::new(0));
        let parent_writes = Rc::new(Cell::new(0));
        let snapshot =
            TransparentObserverMutableSnapshot::new(1, SnapshotIdSet::new(), None, None, None);
        snapshot.set_read_observer(Some(Arc::new({
            let parent_reads = Rc::clone(&parent_reads);
            move |_| parent_reads.set(parent_reads.get() + 1)
        })));
        snapshot.set_write_observer(Some(Arc::new({
            let parent_writes = Rc::clone(&parent_writes);
            move |_| parent_writes.set(parent_writes.get() + 1)
        })));

        let nested = snapshot.take_nested_mutable_snapshot(None, None);
        nested.record_read(&MockState(3));
        nested.record_write(Arc::new(MockState(4)));

        assert_eq!(parent_reads.get(), 1);
        assert_eq!(parent_writes.get(), 1);
    }

    #[test]
    fn transparent_readonly_set_read_observer_replaces_observer() {
        let _guard = reset_runtime();
        let initial_reads = Rc::new(Cell::new(0));
        let replacement_reads = Rc::new(Cell::new(0));
        let snapshot = TransparentObserverSnapshot::new(
            1,
            SnapshotIdSet::new(),
            Some(Arc::new({
                let initial_reads = Rc::clone(&initial_reads);
                move |_| initial_reads.set(initial_reads.get() + 1)
            })),
            None,
        );

        snapshot.set_read_observer(Some(Arc::new({
            let replacement_reads = Rc::clone(&replacement_reads);
            move |_| replacement_reads.set(replacement_reads.get() + 1)
        })));
        snapshot.record_read(&MockState(5));

        assert_eq!(initial_reads.get(), 0);
        assert_eq!(replacement_reads.get(), 1);
    }

    #[test]
    fn test_transparent_observer_mutable_nested() {
        let _guard = reset_runtime();
        let parent =
            TransparentObserverMutableSnapshot::new(1, SnapshotIdSet::new(), None, None, None);

        let nested = parent.take_nested_mutable_snapshot(None, None);
        assert!(nested.snapshot_id() > parent.snapshot_id());
    }

    #[test]
    fn test_transparent_observer_snapshot_nested() {
        let _guard = reset_runtime();
        let parent = TransparentObserverSnapshot::new(1, SnapshotIdSet::new(), None, None);

        let nested = parent.take_nested_snapshot(None);
        assert_eq!(nested.snapshot_id(), parent.snapshot_id());
        assert!(nested.read_only());
    }
}
