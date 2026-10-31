use std::rc::Rc;

use super::*;
use crate::{
    collections::map::HashMap,
    state::{ObjectId, PREEXISTING_SNAPSHOT_ID, StateRecord},
};

enum ApplyOperation {
    PromoteChild {
        object_id: ObjectId,
        state: Rc<dyn StateObject>,
        writer_id: SnapshotId,
    },
    PromoteExisting {
        object_id: ObjectId,
        state: Rc<dyn StateObject>,
        source_id: SnapshotId,
        applied: Rc<StateRecord>,
    },
    CommitMerged {
        object_id: ObjectId,
        state: Rc<dyn StateObject>,
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
/// are stored in thread-local storage and never shared across threads. The `Rc`
/// is used for cheap cloning within a single thread, not for cross-thread sharing.
pub struct MutableSnapshot {
    pub(super) state: SnapshotState,
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
        runtime_tracking: RuntimeTracking,
    ) -> Rc<Self> {
        Rc::new(Self {
            state: SnapshotState::new(id, invalid, read_observer, write_observer, runtime_tracking),
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
    ) -> Rc<Self> {
        Self::from_parts(
            id,
            invalid,
            read_observer,
            write_observer,
            base_parent_id,
            RuntimeTracking::Untracked,
        )
    }

    fn validate_not_applied(&self) {
        assert!(!self.applied.get(), "Snapshot has already been applied");
    }

    fn validate_not_disposed(&self) {
        assert!(!self.state.disposed.get(), "Snapshot has been disposed");
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

    /// Returns this snapshot, which is its own root.
    pub fn root_mutable(self: &Rc<Self>) -> Rc<Self> {
        Rc::clone(self)
    }

    /// Runs `f` with this snapshot as the current snapshot of the thread and returns its result.
    /// The previous current snapshot is restored when `f` returns.
    pub fn enter<T>(self: &Rc<Self>, f: impl FnOnce() -> T) -> T {
        enter_snapshot_scope(AnySnapshot::Mutable(Rc::clone(self)), f)
    }

    /// Takes a read-only snapshot of this snapshot's view of state. Reads inside the new snapshot
    /// call both `read_observer` and this snapshot's own read observer. Panics if this snapshot was
    /// applied or disposed.
    pub fn take_nested_snapshot(
        self: &Rc<Self>,
        read_observer: Option<ReadObserver>,
    ) -> Rc<ReadonlySnapshot> {
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

        let parent_weak = Rc::downgrade(self);
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
        self.validate_not_applied();
        self.validate_not_disposed();
        self.state.record_write(state, self.state.id.get());
    }

    /// Returns `true` once the snapshot was disposed.
    pub fn is_disposed(&self) -> bool {
        self.state.disposed.get()
    }

    /// Makes the writes of this snapshot visible to the global state. Returns `Failure` if the
    /// snapshot was already applied or disposed, or if a state object it wrote changed in the
    /// global state in a way that cannot be merged. After a successful apply with writes, the apply
    /// observers are called.
    pub fn apply(&self) -> SnapshotApplyResult {
        // Values the apply displaces drop once it has landed.
        let _retiring = crate::state::RetiredValuesScope::enter();
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
        let mut modified_objects: Vec<(ObjectId, Rc<dyn StateObject>, SnapshotId)> =
            Vec::with_capacity(modified.len());
        for (&obj_id, (obj, writer_id)) in modified.iter() {
            modified_objects.push((obj_id, Rc::clone(obj), *writer_id));
        }

        drop(modified);

        let parent_snapshot = GlobalSnapshot::get_or_create();
        let parent_snapshot_id = parent_snapshot.snapshot_id();
        let parent_invalid = parent_snapshot.invalid();
        drop(parent_snapshot);

        let next_invalid = runtime::open_snapshots().clear(parent_snapshot_id);
        let this_invalid_for_optimistic = self.state.invalid.borrow().clone();
        let optimistic = optimistic_merges(
            parent_snapshot_id,
            self.base_parent_id,
            &modified_objects,
            &next_invalid,
            &this_invalid_for_optimistic,
        );

        let mut operations: Vec<ApplyOperation> = Vec::with_capacity(modified_objects.len());

        for (obj_id, state, writer_id) in &modified_objects {
            let records = state.records();
            let Some(applied) = records.live_at(*writer_id) else {
                return SnapshotApplyResult::Failure;
            };

            let Some(current) = records
                .readable(parent_snapshot_id, &next_invalid)
                .or_else(|| state.try_readable_record(parent_snapshot_id, &parent_invalid))
            else {
                log::error!(
                    "MutableSnapshot::apply missing parent readable record (object_id={obj_id:?}, parent_snapshot_id={parent_snapshot_id})"
                );
                return SnapshotApplyResult::Failure;
            };
            let this_invalid = self.state.invalid.borrow();
            let (previous_opt, found_base) = records.previous(self.base_parent_id, &this_invalid);
            drop(this_invalid);
            let Some(previous) = previous_opt else {
                return SnapshotApplyResult::Failure;
            };

            if !found_base || previous.snapshot_id() == PREEXISTING_SNAPSHOT_ID {
                operations.push(ApplyOperation::PromoteChild {
                    object_id: *obj_id,
                    state: Rc::clone(state),
                    writer_id: *writer_id,
                });
                continue;
            }

            if Rc::ptr_eq(&current, &previous) {
                operations.push(ApplyOperation::PromoteChild {
                    object_id: *obj_id,
                    state: Rc::clone(state),
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
                    state: Rc::clone(state),
                    writer_id: *writer_id,
                });
            } else if Rc::ptr_eq(&merged, &current) {
                operations.push(ApplyOperation::PromoteExisting {
                    object_id: *obj_id,
                    state: Rc::clone(state),
                    source_id: current.snapshot_id(),
                    applied: Rc::clone(&applied),
                });
            } else {
                operations.push(ApplyOperation::CommitMerged {
                    object_id: *obj_id,
                    state: Rc::clone(state),
                    merged: Rc::clone(&merged),
                    applied: Rc::clone(&applied),
                });
            }
        }

        let mut applied_info: Vec<(ObjectId, Rc<dyn StateObject>, SnapshotId)> =
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
                    let new_head_id = state.records().head().snapshot_id();
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
                    applied.tombstone_retiring_value();
                    let new_head_id = state.records().head().snapshot_id();
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
                    applied.tombstone_retiring_value();
                    applied_info.push((object_id, state, new_head_id));
                }
            }
        }

        for (obj_id, _, head_id) in &applied_info {
            set_last_write(*obj_id, *head_id);
        }

        self.applied.set(true);
        self.state.dispose();

        crate::thread_state::snapshots(|snapshots| {
            let mut extra_state_objects = snapshots.extra_state_objects.borrow_mut();
            for (_, state, _) in &applied_info {
                extra_state_objects.add_trait_object(state);
            }
        });

        let observer_states: Vec<Rc<dyn StateObject>> = applied_info
            .iter()
            .map(|(_, state, _)| Rc::clone(state))
            .collect();
        notify_apply_observers(&observer_states, this_id);
        SnapshotApplyResult::Success
    }

    /// Takes a mutable snapshot nested in this one. Its `apply` moves the writes into this
    /// snapshot. Panics if this snapshot was applied or disposed.
    pub fn take_nested_mutable_snapshot(
        self: &Rc<Self>,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
    ) -> Rc<NestedMutableSnapshot> {
        self.validate_not_disposed();
        self.validate_not_applied();

        allocate_nested_mutable_snapshot(self, Rc::downgrade(self), read_observer, write_observer)
    }

    pub(crate) fn merge_child_modifications(
        &self,
        child_modified: &HashMap<ObjectId, (Rc<dyn StateObject>, SnapshotId)>,
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
    ) -> Vec<(ObjectId, Rc<dyn StateObject>, SnapshotId)> {
        let modified = self.state.modified.borrow();
        modified
            .iter()
            .map(|(&obj_id, (state, writer_id))| (obj_id, Rc::clone(state), *writer_id))
            .collect()
    }

    pub(crate) fn debug_base_parent_id(&self) -> SnapshotId {
        self.base_parent_id
    }
}

#[cfg(test)]
#[path = "tests/mutable_tests.rs"]
mod tests;
