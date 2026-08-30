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
#[allow(clippy::arc_with_non_send_sync)]
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
#[allow(clippy::arc_with_non_send_sync)]
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
        if self.applied.get() {
            panic!("Cannot write to an applied snapshot");
        }
        if self.state.disposed.get() {
            panic!("Cannot write to a disposed snapshot");
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
        let merged_read =
            merge_read_observers(read_observer, self.state.read_observer.borrow().clone());
        let merged_write =
            merge_write_observers(write_observer, self.state.write_observer.borrow().clone());

        let parent_id = self.state.id.get();
        let current_invalid = self.state.invalid.borrow().clone();

        let (new_id, _runtime_invalid) = allocate_snapshot();

        let parent_invalid_with_child = current_invalid.set(new_id);
        self.state.invalid.replace(parent_invalid_with_child);

        let invalid = current_invalid.add_range(parent_id + 1, new_id);

        let self_weak = Arc::downgrade(&self.root_mutable());

        let nested = NestedMutableSnapshot::new(
            new_id,
            invalid,
            merged_read,
            merged_write,
            self_weak,
            self.state.id.get(),
        );

        self.nested_count.set(self.nested_count.get() + 1);
        self.state.add_pending_child(new_id);

        let parent_self_weak = Arc::downgrade(self);
        nested.set_on_dispose({
            let child_id = new_id;
            move || {
                if let Some(parent) = parent_self_weak.upgrade() {
                    if parent.nested_count.get() > 0 {
                        parent
                            .nested_count
                            .set(parent.nested_count.get().saturating_sub(1));
                    }
                    let mut invalid = parent.state.invalid.borrow_mut();
                    let new_set = invalid.clone().clear(child_id);
                    *invalid = new_set;
                    parent.state.remove_pending_child(child_id);
                }
            }
        });

        nested
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::snapshot_v2::runtime::TestRuntimeGuard;

    fn reset_runtime() -> TestRuntimeGuard {
        reset_runtime_for_tests()
    }

    fn mock_state_record() -> Rc<crate::state::StateRecord> {
        crate::state::StateRecord::new(crate::state::PREEXISTING_SNAPSHOT_ID, (), None)
    }

    #[test]
    fn test_nested_readonly_snapshot() {
        let _guard = reset_runtime();
        let parent = NestedReadonlySnapshot::new(1, SnapshotIdSet::new(), None, Weak::new());
        let parent_weak = Arc::downgrade(&parent);

        let nested = NestedReadonlySnapshot::new(1, SnapshotIdSet::new(), None, parent_weak);

        assert_eq!(nested.snapshot_id(), 1);
        assert!(nested.read_only());
        assert!(!nested.is_disposed());
    }

    #[test]
    fn test_nested_readonly_snapshot_root() {
        let _guard = reset_runtime();
        let parent = NestedReadonlySnapshot::new(1, SnapshotIdSet::new(), None, Weak::new());
        let parent_weak = Arc::downgrade(&parent);

        let nested = NestedReadonlySnapshot::new(1, SnapshotIdSet::new(), None, parent_weak);

        let root = nested.root_nested_readonly();
        assert_eq!(root.snapshot_id(), 1);
    }

    #[test]
    fn test_nested_readonly_dispose() {
        let _guard = reset_runtime();
        let parent = NestedReadonlySnapshot::new(1, SnapshotIdSet::new(), None, Weak::new());
        let parent_weak = Arc::downgrade(&parent);

        let nested = NestedReadonlySnapshot::new(1, SnapshotIdSet::new(), None, parent_weak);

        nested.dispose();
        assert!(nested.is_disposed());
    }

    #[test]
    fn test_nested_mutable_snapshot() {
        let _guard = reset_runtime();
        let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
        let parent_weak = Arc::downgrade(&parent);

        let nested =
            NestedMutableSnapshot::new(2, SnapshotIdSet::new().set(1), None, None, parent_weak, 1);

        assert_eq!(nested.snapshot_id(), 2);
        assert!(!nested.read_only());
        assert!(!nested.is_disposed());
    }

    #[test]
    fn test_nested_mutable_apply() {
        let _guard = reset_runtime();
        let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
        let parent_weak = Arc::downgrade(&parent);

        let nested =
            NestedMutableSnapshot::new(2, SnapshotIdSet::new().set(1), None, None, parent_weak, 1);

        let result = nested.apply();
        assert!(result.is_success());
        assert!(nested.applied.get());
    }

    #[test]
    fn test_nested_merge_sets_parent_pending_changes() {
        let _guard = reset_runtime();
        struct TestObj {
            id: crate::state::ObjectId,
        }
        impl StateObject for TestObj {
            fn object_id(&self) -> crate::state::ObjectId {
                self.id
            }
            fn first_record(&self) -> Rc<crate::state::StateRecord> {
                mock_state_record()
            }
            fn try_readable_record(
                &self,
                snapshot_id: crate::snapshot_id_set::SnapshotId,
                invalid: &SnapshotIdSet,
            ) -> Option<Rc<crate::state::StateRecord>> {
                Some(self.readable_record(snapshot_id, invalid))
            }
            fn readable_record(
                &self,
                _snapshot_id: crate::snapshot_id_set::SnapshotId,
                _invalid: &SnapshotIdSet,
            ) -> Rc<crate::state::StateRecord> {
                mock_state_record()
            }
            fn prepend_state_record(&self, _record: Rc<crate::state::StateRecord>) {}
            fn promote_record(
                &self,
                _child_id: crate::snapshot_id_set::SnapshotId,
            ) -> Result<(), &'static str> {
                Ok(())
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
        let child = parent.take_nested_mutable_snapshot(None, None);

        let obj = Arc::new(TestObj {
            id: crate::state::ObjectId(100),
        });
        child.record_write(obj);
        assert!(!parent.has_pending_changes());
        child.apply().check();
        assert!(parent.has_pending_changes());
    }

    #[test]
    fn test_nested_conflict_with_parent_same_object() {
        let _guard = reset_runtime();
        struct TestObj {
            id: crate::state::ObjectId,
        }
        impl StateObject for TestObj {
            fn object_id(&self) -> crate::state::ObjectId {
                self.id
            }
            fn first_record(&self) -> Rc<crate::state::StateRecord> {
                mock_state_record()
            }
            fn try_readable_record(
                &self,
                snapshot_id: crate::snapshot_id_set::SnapshotId,
                invalid: &SnapshotIdSet,
            ) -> Option<Rc<crate::state::StateRecord>> {
                Some(self.readable_record(snapshot_id, invalid))
            }
            fn readable_record(
                &self,
                _snapshot_id: crate::snapshot_id_set::SnapshotId,
                _invalid: &SnapshotIdSet,
            ) -> Rc<crate::state::StateRecord> {
                mock_state_record()
            }
            fn prepend_state_record(&self, _record: Rc<crate::state::StateRecord>) {}
            fn promote_record(
                &self,
                _child_id: crate::snapshot_id_set::SnapshotId,
            ) -> Result<(), &'static str> {
                Ok(())
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
        let child = parent.take_nested_mutable_snapshot(None, None);

        let obj = Arc::new(TestObj {
            id: crate::state::ObjectId(200),
        });
        parent.record_write(obj.clone());
        child.record_write(obj.clone());

        let result = child.apply();
        assert!(result.is_failure());
    }

    #[test]
    fn test_nested_mutable_apply_twice_fails() {
        let _guard = reset_runtime();
        let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
        let parent_weak = Arc::downgrade(&parent);

        let nested =
            NestedMutableSnapshot::new(2, SnapshotIdSet::new().set(1), None, None, parent_weak, 1);

        nested.apply().check();
        let result = nested.apply();
        assert!(result.is_failure());
    }

    #[test]
    fn test_nested_mutable_dispose() {
        let _guard = reset_runtime();
        let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
        let parent_weak = Arc::downgrade(&parent);

        let nested =
            NestedMutableSnapshot::new(2, SnapshotIdSet::new().set(1), None, None, parent_weak, 1);

        nested.dispose();
        assert!(nested.is_disposed());
    }
}
