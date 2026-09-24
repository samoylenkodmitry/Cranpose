use std::{rc::Rc, sync::Arc};

use super::*;
use crate::{
    collections::map::HashMap,
    state::{PREEXISTING_SNAPSHOT_ID, StateRecord},
};

pub(super) fn find_record_by_id(
    head: &Rc<StateRecord>,
    target: SnapshotId,
) -> Option<Rc<StateRecord>> {
    let mut cursor = Some(Rc::clone(head));
    while let Some(record) = cursor {
        if !record.is_tombstone() && record.snapshot_id() == target {
            return Some(record);
        }
        cursor = record.next();
    }
    None
}

pub(super) fn find_previous_record(
    head: &Rc<StateRecord>,
    base_snapshot_id: SnapshotId,
    invalid: &SnapshotIdSet,
) -> (Option<Rc<StateRecord>>, bool) {
    let mut cursor = Some(Rc::clone(head));
    let mut best: Option<Rc<StateRecord>> = None;
    let mut fallback: Option<Rc<StateRecord>> = None;
    let mut found_base = false;

    while let Some(record) = cursor {
        if !record.is_tombstone() {
            let id = record.snapshot_id();
            let is_valid = id <= base_snapshot_id && !invalid.get(id);
            if is_valid {
                found_base = true;
                let replace = best
                    .as_ref()
                    .is_none_or(|current| current.snapshot_id() < id);
                if replace {
                    best = Some(record.clone());
                }
            }
            if fallback.is_none() {
                fallback = Some(record.clone());
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
        applied: Rc<StateRecord>,
    },
    CommitMerged {
        object_id: StateObjectId,
        state: Arc<dyn StateObject>,
        merged: Rc<StateRecord>,
        applied: Rc<StateRecord>,
    },
}

/// A mutable snapshot that allows isolated state changes.
///
/// Changes made in a mutable snapshot are isolated from other snapshots
/// until `apply()` is called, at which point they become visible atomically.
/// This is a root mutable snapshot (not nested).
///
/// # Thread Safety
/// Contains `Cell<T>` which is not `Send`/`Sync`. This is safe because snapshots
/// are stored in thread-local storage and never shared across threads. The `Arc`
/// is used for cheap cloning within a single thread, not for cross-thread sharing.
pub struct MutableSnapshot {
    state: SnapshotState,
    base_parent_id: SnapshotId,
    nested_count: Cell<usize>,
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
        assert!(!self.applied.get(), "Snapshot has already been applied");
    }

    fn validate_not_disposed(&self) {
        assert!(!self.state.disposed.get(), "Snapshot has been disposed");
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
        enter_snapshot_scope(AnySnapshot::Mutable(self.clone()), f)
    }

    pub fn take_nested_snapshot(
        self: &Arc<Self>,
        read_observer: Option<ReadObserver>,
    ) -> Arc<ReadonlySnapshot> {
        self.validate_not_disposed();
        self.validate_not_applied();

        let merged_observer =
            merge_read_observers(read_observer, self.state.read_observer.borrow().clone());

        let nested = ReadonlySnapshot::new(
            self.state.id.get(),
            self.state.invalid.borrow().clone(),
            merged_observer,
        );

        self.nested_count.set(self.nested_count.get() + 1);

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

        let modified = self.state.modified.borrow();
        if modified.is_empty() {
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
        let this_invalid_for_optimistic = self.state.invalid.borrow().clone();
        let optimistic = super::optimistic_merges(
            parent_snapshot_id,
            self.base_parent_id,
            &modified_objects,
            &next_invalid,
            &this_invalid_for_optimistic,
        );

        let mut operations: Vec<ApplyOperation> = Vec::with_capacity(modified_objects.len());

        for (obj_id, state, writer_id) in &modified_objects {
            let head = state.first_record();
            let Some(applied) = find_record_by_id(&head, *writer_id) else {
                return SnapshotApplyResult::Failure;
            };

            let Some(current) =
                crate::state::readable_record_for(&head, parent_snapshot_id, &next_invalid)
                    .or_else(|| state.try_readable_record(parent_snapshot_id, &parent_invalid))
            else {
                log::error!(
                    "MutableSnapshot::apply missing parent readable record (object_id={obj_id:?}, parent_snapshot_id={parent_snapshot_id})"
                );
                return SnapshotApplyResult::Failure;
            };
            let this_invalid = self.state.invalid.borrow();
            let (previous_opt, found_base) =
                find_previous_record(&head, self.base_parent_id, &this_invalid);
            drop(this_invalid);
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

            if Rc::ptr_eq(&current, &previous) {
                operations.push(ApplyOperation::PromoteChild {
                    object_id: *obj_id,
                    state: state.clone(),
                    writer_id: *writer_id,
                });
                continue;
            }

            let merged = if let Some(candidate) = optimistic
                .as_ref()
                .and_then(|map| map.get(&(Rc::as_ptr(&current) as usize)))
                .cloned()
            {
                candidate
            } else {
                match state.merge_records(
                    Rc::clone(&previous),
                    Rc::clone(&current),
                    Rc::clone(&applied),
                ) {
                    Some(record) => record,
                    None => return SnapshotApplyResult::Failure,
                }
            };

            if Rc::ptr_eq(&merged, &applied) {
                operations.push(ApplyOperation::PromoteChild {
                    object_id: *obj_id,
                    state: state.clone(),
                    writer_id: *writer_id,
                });
            } else if Rc::ptr_eq(&merged, &current) {
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

        for (_, state, _) in &applied_info {
            super::EXTRA_STATE_OBJECTS.with(|cell| {
                cell.borrow_mut().add_trait_object(state);
            });
        }

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

        allocate_nested_mutable_snapshot(self, Arc::downgrade(self), read_observer, write_observer)
    }

    pub(crate) fn merge_child_modifications(
        &self,
        child_modified: &HashMap<StateObjectId, (Arc<dyn StateObject>, SnapshotId)>,
    ) -> Result<(), ()> {
        {
            let parent_mod = self.state.modified.borrow();
            for key in child_modified.keys() {
                if parent_mod.contains_key(key) {
                    return Err(());
                }
            }
        }

        let mut parent_mod = self.state.modified.borrow_mut();
        for (key, value) in child_modified {
            parent_mod.entry(*key).or_insert_with(|| value.clone());
        }
        Ok(())
    }
}

impl NestedMutableHost for MutableSnapshot {
    fn snapshot_state(&self) -> &SnapshotState {
        &self.state
    }

    fn nested_count(&self) -> &Cell<usize> {
        &self.nested_count
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
#[path = "tests/mutable_tests.rs"]
mod tests;
