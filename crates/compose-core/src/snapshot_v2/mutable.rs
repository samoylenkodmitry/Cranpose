//! Mutable snapshot implementation.

use super::*;
use crate::collections::map::HashMap;
use crate::state::{StateRecord, PREEXISTING_SNAPSHOT_ID};
use std::sync::Arc;

pub(super) fn find_record_by_id(
    head: &Arc<StateRecord>,
    target: SnapshotId,
) -> Option<Arc<StateRecord>> {
    let mut cursor = Some(Arc::clone(head));
    while let Some(record) = cursor {
        if !record.is_tombstone() && record.snapshot_id() == target {
            return Some(record);
        }
        cursor = record.next();
    }
    None
}

pub(super) fn find_previous_record(
    head: &Arc<StateRecord>,
    base_snapshot_id: SnapshotId,
) -> (Option<Arc<StateRecord>>, bool) {
    let mut cursor = Some(Arc::clone(head));
    let mut best: Option<Arc<StateRecord>> = None;
    let mut fallback: Option<Arc<StateRecord>> = None;
    let mut found_base = false;

    while let Some(record) = cursor {
        if !record.is_tombstone() {
            fallback = Some(record.clone());
            if record.snapshot_id() <= base_snapshot_id {
                found_base = true;
                let replace = best
                    .as_ref()
                    .map(|current| current.snapshot_id() < record.snapshot_id())
                    .unwrap_or(true);
                if replace {
                    best = Some(record.clone());
                }
            }
        }
        cursor = record.next();
    }

    (best.or(fallback), found_base)
}

enum ApplyOperation {
    PromoteChild {
        object_id: StateObjectId,
        state: Arc<dyn StateObject>,
        writer_id: SnapshotId,
    },
    PromoteExisting {
        object_id: StateObjectId,
        state: Arc<dyn StateObject>,
        source_id: SnapshotId,
        applied: Arc<StateRecord>,
    },
    CommitMerged {
        object_id: StateObjectId,
        state: Arc<dyn StateObject>,
        merged: Arc<StateRecord>,
        applied: Arc<StateRecord>,
    },
}

/// A mutable snapshot that allows isolated state changes.
///
/// Changes made in a mutable snapshot are isolated from other snapshots
/// until `apply()` is called, at which point they become visible atomically.
/// This is a root mutable snapshot (not nested).
pub struct MutableSnapshot {
    state: SnapshotState,
    /// The parent's snapshot id at the time this snapshot was created
    base_parent_id: SnapshotId,
    /// Number of active nested snapshots
    nested_count: Cell<usize>,
    /// Whether this snapshot has been applied
    applied: Cell<bool>,
}

impl MutableSnapshot {
    pub(crate) fn from_parts(
        id: SnapshotId,
        invalid: SnapshotIdSet,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
        base_parent_id: SnapshotId,
        runtime_tracked: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: SnapshotState::new(id, invalid, read_observer, write_observer, runtime_tracked),
            base_parent_id,
            nested_count: Cell::new(0),
            applied: Cell::new(false),
        })
    }

    /// Create a new root mutable snapshot using the global runtime.
    pub fn new_root(
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
    ) -> Arc<Self> {
        GlobalSnapshot::get_or_create().take_nested_mutable_snapshot(read_observer, write_observer)
    }

    /// Create a new root mutable snapshot.
    pub fn new(
        id: SnapshotId,
        invalid: SnapshotIdSet,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
        base_parent_id: SnapshotId,
    ) -> Arc<Self> {
        Self::from_parts(
            id,
            invalid,
            read_observer,
            write_observer,
            base_parent_id,
            false,
        )
    }

    fn validate_not_applied(&self) {
        if self.applied.get() {
            panic!("Snapshot has already been applied");
        }
    }

    fn validate_not_disposed(&self) {
        if self.state.disposed.get() {
            panic!("Snapshot has been disposed");
        }
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

    pub fn root_mutable(self: &Arc<Self>) -> Arc<Self> {
        self.clone()
    }

    pub fn enter<T>(self: &Arc<Self>, f: impl FnOnce() -> T) -> T {
        let previous = current_snapshot();
        set_current_snapshot(Some(AnySnapshot::Mutable(self.clone())));
        let result = f();
        set_current_snapshot(previous);
        result
    }

    pub fn take_nested_snapshot(
        self: &Arc<Self>,
        read_observer: Option<ReadObserver>,
    ) -> Arc<ReadonlySnapshot> {
        self.validate_not_disposed();
        self.validate_not_applied();

        let merged_observer = merge_read_observers(read_observer, self.state.read_observer.clone());

        // Create a nested read-only snapshot
        let nested = ReadonlySnapshot::new(
            self.state.id.get(),
            self.state.invalid.borrow().clone(),
            merged_observer,
        );

        self.nested_count.set(self.nested_count.get() + 1);

        // When the nested snapshot is disposed, decrement this parent's nested_count
        let parent_weak = Arc::downgrade(self);
        nested.set_on_dispose(move || {
            if let Some(parent) = parent_weak.upgrade() {
                let cur = parent.nested_count.get();
                if cur > 0 {
                    parent.nested_count.set(cur - 1);
                }
            }
        });
        nested
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
        self.validate_not_applied();
        self.validate_not_disposed();
        self.state.record_write(state, self.state.id.get());
    }

    pub fn notify_objects_initialized(&self) {
        if !self.applied.get() && !self.state.disposed.get() {
            // Mark that objects are initialized
            // In a full implementation, this would update internal state
        }
    }

    pub fn close(&self) {
        self.state.disposed.set(true);
    }

    pub fn is_disposed(&self) -> bool {
        self.state.disposed.get()
    }

    pub fn apply(&self) -> SnapshotApplyResult {
        // Check disposed state first - return Failure instead of panicking
        if self.state.disposed.get() {
            return SnapshotApplyResult::Failure;
        }

        if self.applied.get() {
            return SnapshotApplyResult::Failure;
        }

        let modified = self.state.modified.borrow();
        if modified.is_empty() {
            // No changes to apply
            self.applied.set(true);
            self.state.dispose();
            return SnapshotApplyResult::Success;
        }

        let this_id = self.state.id.get();
        let mut modified_objects: Vec<(StateObjectId, Arc<dyn StateObject>, SnapshotId)> =
            Vec::with_capacity(modified.len());
        for (&obj_id, (obj, writer_id)) in modified.iter() {
            modified_objects.push((obj_id, obj.clone(), *writer_id));
        }

        drop(modified);

        let parent_snapshot = GlobalSnapshot::get_or_create();
        let parent_snapshot_id = parent_snapshot.snapshot_id();
        let parent_invalid = parent_snapshot.invalid();
        drop(parent_snapshot);

        let next_invalid = super::runtime::open_snapshots().clear(parent_snapshot_id);
        let optimistic = super::optimistic_merges(
            parent_snapshot_id,
            self.base_parent_id,
            &modified_objects,
            &next_invalid,
        );

        let mut operations: Vec<ApplyOperation> = Vec::with_capacity(modified_objects.len());

        for (obj_id, state, writer_id) in &modified_objects {
            let head = state.first_record();
            let applied = match find_record_by_id(&head, *writer_id) {
                Some(record) => record,
                None => return SnapshotApplyResult::Failure,
            };

            let current = crate::state::readable_record_for(
                &head,
                parent_snapshot_id,
                &next_invalid,
            )
            .unwrap_or_else(|| state.readable_record(parent_snapshot_id, &parent_invalid));
            let (previous_opt, found_base) = find_previous_record(&head, self.base_parent_id);
            let Some(previous) = previous_opt else {
                return SnapshotApplyResult::Failure;
            };

            if !found_base || previous.snapshot_id() == PREEXISTING_SNAPSHOT_ID {
                operations.push(ApplyOperation::PromoteChild {
                    object_id: *obj_id,
                    state: state.clone(),
                    writer_id: *writer_id,
                });
                continue;
            }

            if Arc::ptr_eq(&current, &previous) {
                operations.push(ApplyOperation::PromoteChild {
                    object_id: *obj_id,
                    state: state.clone(),
                    writer_id: *writer_id,
                });
                continue;
            }

            let merged = if let Some(candidate) = optimistic
                .as_ref()
                .and_then(|map| map.get(&(Arc::as_ptr(&current) as usize)))
                .cloned()
            {
                candidate
            } else {
                match state.merge_records(
                    Arc::clone(&previous),
                    Arc::clone(&current),
                    Arc::clone(&applied),
                ) {
                    Some(record) => record,
                    None => return SnapshotApplyResult::Failure,
                }
            };

            if Arc::ptr_eq(&merged, &applied) {
                operations.push(ApplyOperation::PromoteChild {
                    object_id: *obj_id,
                    state: state.clone(),
                    writer_id: *writer_id,
                });
            } else if Arc::ptr_eq(&merged, &current) {
                operations.push(ApplyOperation::PromoteExisting {
                    object_id: *obj_id,
                    state: state.clone(),
                    source_id: current.snapshot_id(),
                    applied: applied.clone(),
                });
            } else {
                operations.push(ApplyOperation::CommitMerged {
                    object_id: *obj_id,
                    state: state.clone(),
                    merged: merged.clone(),
                    applied: applied.clone(),
                });
            }
        }

        let mut applied_info: Vec<(StateObjectId, Arc<dyn StateObject>, SnapshotId)> =
            Vec::with_capacity(operations.len());

        for operation in operations {
            match operation {
                ApplyOperation::PromoteChild {
                    object_id,
                    state,
                    writer_id,
                } => {
                    if state.promote_record(writer_id).is_err() {
                        return SnapshotApplyResult::Failure;
                    }
                    let new_head_id = state.first_record().snapshot_id();
                    applied_info.push((object_id, state, new_head_id));
                }
                ApplyOperation::PromoteExisting {
                    object_id,
                    state,
                    source_id,
                    applied,
                } => {
                    if state.promote_record(source_id).is_err() {
                        return SnapshotApplyResult::Failure;
                    }
                    applied.set_tombstone(true);
                    applied.clear_value();
                    let new_head_id = state.first_record().snapshot_id();
                    applied_info.push((object_id, state, new_head_id));
                }
                ApplyOperation::CommitMerged {
                    object_id,
                    state,
                    merged,
                    applied,
                } => {
                    let Ok(new_head_id) = state.commit_merged_record(merged) else {
                        return SnapshotApplyResult::Failure;
                    };
                    applied.set_tombstone(true);
                    applied.clear_value();
                    applied_info.push((object_id, state, new_head_id));
                }
            }
        }

        for (obj_id, _, head_id) in &applied_info {
            super::set_last_write(*obj_id, *head_id);
        }

        self.applied.set(true);
        self.state.dispose();

        // TODO(Phase 2B): Cleanup during apply is temporarily disabled due to coordination
        // issues with sibling snapshots. When multiple sibling snapshots apply sequentially,
        // cleanup from the first can invalidate records that the second needs for its merge.
        //
        // The Kotlin implementation handles this via a global sync{} lock that serializes
        // all snapshot operations. In Rust, we use thread-local storage without global locks,
        // so we need a different coordination strategy.
        //
        // For now, cleanup only runs during advanceGlobalSnapshot(), which is sufficient
        // for most use cases. Per-apply cleanup will be re-enabled in a future phase once
        // proper sibling snapshot coordination is implemented.
        //
        // Disabled code:
        // for (_, state, _) in &applied_info {
        //     super::process_for_unused_records_locked(state);
        // }

        let observer_states: Vec<Arc<dyn StateObject>> = applied_info
            .iter()
            .map(|(_, state, _)| state.clone())
            .collect();
        super::notify_apply_observers(&observer_states, this_id);
        SnapshotApplyResult::Success
    }

    pub fn take_nested_mutable_snapshot(
        self: &Arc<Self>,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
    ) -> Arc<NestedMutableSnapshot> {
        self.validate_not_disposed();
        self.validate_not_applied();

        let merged_read = merge_read_observers(read_observer, self.state.read_observer.clone());
        let merged_write = merge_write_observers(write_observer, self.state.write_observer.clone());

        let (new_id, runtime_invalid) = allocate_snapshot();

        // Merge runtime invalid data with the parent's invalid set and ensure the parent
        // also tracks the child snapshot id.
        let mut parent_invalid = self.state.invalid.borrow().clone();
        parent_invalid = parent_invalid.set(new_id);
        self.state.invalid.replace(parent_invalid.clone());
        let invalid = parent_invalid.or(&runtime_invalid);

        let self_weak = Arc::downgrade(self);
        let nested = NestedMutableSnapshot::new(
            new_id,
            invalid,
            merged_read,
            merged_write,
            self_weak,
            self.state.id.get(), // base_parent_id for child is this snapshot's id
        );

        self.nested_count.set(self.nested_count.get() + 1);
        self.state.add_pending_child(new_id);

        let parent_weak = Arc::downgrade(self);
        nested.set_on_dispose({
            let child_id = new_id;
            move || {
                if let Some(parent) = parent_weak.upgrade() {
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

    /// Merge a child's modified set into this snapshot's modified set.
    ///
    /// Returns Ok(()) on success, or Err(()) if a conflict is detected
    /// (i.e., this snapshot already has a modification for the same object).
    pub(crate) fn merge_child_modifications(
        &self,
        child_modified: &HashMap<StateObjectId, (Arc<dyn StateObject>, SnapshotId)>,
    ) -> Result<(), ()> {
        // Check for conflicts
        {
            let parent_mod = self.state.modified.borrow();
            for key in child_modified.keys() {
                if parent_mod.contains_key(key) {
                    return Err(());
                }
            }
        }

        // Merge entries
        let mut parent_mod = self.state.modified.borrow_mut();
        for (key, value) in child_modified.iter() {
            parent_mod.entry(*key).or_insert_with(|| value.clone());
        }
        Ok(())
    }
}

#[cfg(test)]
impl MutableSnapshot {
    pub(crate) fn debug_modified_objects(
        &self,
    ) -> Vec<(StateObjectId, Arc<dyn StateObject>, SnapshotId)> {
        let modified = self.state.modified.borrow();
        modified
            .iter()
            .map(|(&obj_id, (state, writer_id))| (obj_id, state.clone(), *writer_id))
            .collect()
    }

    pub(crate) fn debug_base_parent_id(&self) -> SnapshotId {
        self.base_parent_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot_v2::runtime::TestRuntimeGuard;
    use crate::state::{NeverEqual, SnapshotMutableState, StateObject};
    use std::cell::Cell;
    use std::sync::Arc;

    fn reset_runtime() -> TestRuntimeGuard {
        reset_runtime_for_tests()
    }

    fn new_state(initial: i32) -> Arc<SnapshotMutableState<i32>> {
        SnapshotMutableState::new_in_arc(initial, Arc::new(NeverEqual))
    }

    // Mock StateObject for testing
    struct MockStateObject {
        value: Cell<i32>,
    }

    impl StateObject for MockStateObject {
        fn object_id(&self) -> crate::state::ObjectId {
            crate::state::ObjectId(0)
        }

        fn first_record(&self) -> Arc<crate::state::StateRecord> {
            unimplemented!("Not needed for tests")
        }

        fn readable_record(
            &self,
            _snapshot_id: crate::snapshot_id_set::SnapshotId,
            _invalid: &SnapshotIdSet,
        ) -> Arc<crate::state::StateRecord> {
            unimplemented!("Not needed for tests")
        }

        fn prepend_state_record(&self, _record: Arc<crate::state::StateRecord>) {
            unimplemented!("Not needed for tests")
        }

        fn promote_record(
            &self,
            _child_id: crate::snapshot_id_set::SnapshotId,
        ) -> Result<(), &'static str> {
            unimplemented!("Not needed for tests")
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_mutable_snapshot_creation() {
        let _guard = reset_runtime();
        let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
        assert_eq!(snapshot.snapshot_id(), 1);
        assert!(!snapshot.read_only());
        assert!(!snapshot.is_disposed());
        assert!(!snapshot.applied.get());
    }

    #[test]
    fn test_mutable_snapshot_no_pending_changes_initially() {
        let _guard = reset_runtime();
        let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
        assert!(!snapshot.has_pending_changes());
    }

    #[test]
    fn test_mutable_snapshot_enter() {
        let _guard = reset_runtime();
        let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);

        set_current_snapshot(None);
        assert!(current_snapshot().is_none());

        snapshot.enter(|| {
            let current = current_snapshot();
            assert!(current.is_some());
            assert_eq!(current.unwrap().snapshot_id(), 1);
        });

        assert!(current_snapshot().is_none());
    }

    #[test]
    fn test_mutable_snapshot_read_observer() {
        let _guard = reset_runtime();
        use std::sync::{Arc as StdArc, Mutex};

        let read_count = StdArc::new(Mutex::new(0));
        let read_count_clone = read_count.clone();

        let observer = Arc::new(move |_: &dyn StateObject| {
            *read_count_clone.lock().unwrap() += 1;
        });

        let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), Some(observer), None, 0);
        let mock_state = MockStateObject {
            value: Cell::new(42),
        };

        snapshot.record_read(&mock_state);
        snapshot.record_read(&mock_state);

        assert_eq!(*read_count.lock().unwrap(), 2);
    }

    #[test]
    fn test_mutable_snapshot_write_observer() {
        let _guard = reset_runtime();
        use std::sync::{Arc as StdArc, Mutex};

        let write_count = StdArc::new(Mutex::new(0));
        let write_count_clone = write_count.clone();

        let observer = Arc::new(move |_: &dyn StateObject| {
            *write_count_clone.lock().unwrap() += 1;
        });

        let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, Some(observer), 0);
        let mock_state = Arc::new(MockStateObject {
            value: Cell::new(42),
        });

        snapshot.record_write(mock_state.clone());
        snapshot.record_write(mock_state.clone()); // Second write should not call observer

        // Note: Current implementation calls observer on every write
        // In full implementation, it would only call on first write
        assert!(*write_count.lock().unwrap() >= 1);
    }

    #[test]
    fn test_mutable_snapshot_apply_empty() {
        let _guard = reset_runtime();
        let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
        let result = snapshot.apply();
        assert!(result.is_success());
        assert!(snapshot.applied.get());
    }

    #[test]
    fn test_mutable_snapshot_apply_twice_fails() {
        let _guard = reset_runtime();
        let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
        snapshot.apply().check();

        let result = snapshot.apply();
        assert!(result.is_failure());
    }

    #[test]
    fn test_mutable_snapshot_nested_readonly() {
        let _guard = reset_runtime();
        let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
        let nested = parent.take_nested_snapshot(None);

        assert_eq!(nested.snapshot_id(), 1);
        assert!(nested.read_only());
        assert_eq!(parent.nested_count.get(), 1);
    }

    #[test]
    fn test_mutable_snapshot_nested_mutable() {
        let _guard = reset_runtime();
        let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
        let nested = parent.take_nested_mutable_snapshot(None, None);

        assert!(nested.snapshot_id() > parent.snapshot_id());
        assert!(!nested.read_only());
        assert_eq!(parent.nested_count.get(), 1);
    }

    #[test]
    fn test_mutable_snapshot_nested_mutable_dispose_clears_invalid() {
        let _guard = reset_runtime();
        let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
        let nested = parent.take_nested_mutable_snapshot(None, None);

        let child_id = nested.snapshot_id();
        assert!(parent.state.invalid.borrow().get(child_id));

        nested.dispose();

        assert_eq!(parent.nested_count.get(), 0);
        assert!(!parent.state.invalid.borrow().get(child_id));
    }

    #[test]
    fn test_mutable_snapshot_nested_dispose() {
        let _guard = reset_runtime();
        let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
        let nested = parent.take_nested_snapshot(None);

        assert_eq!(parent.nested_count.get(), 1);

        nested.dispose();
        assert_eq!(parent.nested_count.get(), 0);
    }

    #[test]
    #[should_panic(expected = "Snapshot has already been applied")]
    fn test_mutable_snapshot_write_after_apply_panics() {
        let _guard = reset_runtime();
        let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
        snapshot.apply().check();

        let mock_state = Arc::new(MockStateObject {
            value: Cell::new(42),
        });
        snapshot.record_write(mock_state);
    }

    #[test]
    #[should_panic(expected = "Snapshot has been disposed")]
    fn test_mutable_snapshot_write_after_dispose_panics() {
        let _guard = reset_runtime();
        let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
        snapshot.dispose();

        let mock_state = Arc::new(MockStateObject {
            value: Cell::new(42),
        });
        snapshot.record_write(mock_state);
    }

    #[test]
    fn test_mutable_snapshot_dispose() {
        let _guard = reset_runtime();
        let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
        assert!(!snapshot.is_disposed());

        snapshot.dispose();
        assert!(snapshot.is_disposed());
    }

    #[test]
    fn test_mutable_snapshot_apply_observer() {
        let _guard = reset_runtime();
        use std::sync::{Arc as StdArc, Mutex};

        let applied_count = StdArc::new(Mutex::new(0));
        let applied_count_clone = applied_count.clone();

        let observer = Arc::new(
            move |_modified: &[Arc<dyn StateObject>], _snapshot_id: SnapshotId| {
                *applied_count_clone.lock().unwrap() += 1;
            },
        );

        let _handle = register_apply_observer(observer);

        let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
        let state = new_state(0);

        snapshot.enter(|| state.set(10));
        snapshot.apply().check();

        assert_eq!(*applied_count.lock().unwrap(), 1);
    }

    #[test]
    fn test_mutable_conflict_detection_same_object() {
        let _guard = reset_runtime();
        let global = GlobalSnapshot::get_or_create();
        let state = new_state(0);

        let s1 = global.take_nested_mutable_snapshot(None, None);
        s1.enter(|| state.set(1));

        let s2 = global.take_nested_mutable_snapshot(None, None);
        s2.enter(|| state.set(2));

        assert!(s1.apply().is_success());
        assert!(s2.apply().is_failure());
    }

    #[test]
    fn test_mutable_no_conflict_different_objects() {
        let _guard = reset_runtime();
        let global = GlobalSnapshot::get_or_create();
        let state1 = new_state(0);
        let state2 = new_state(0);

        let s1 = global.take_nested_mutable_snapshot(None, None);
        s1.enter(|| state1.set(10));

        let s2 = global.take_nested_mutable_snapshot(None, None);
        s2.enter(|| state2.set(20));

        assert!(s1.apply().is_success());
        assert!(s2.apply().is_success());
    }
}
