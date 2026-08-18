// StateRecord uses Rc with Cell for single-threaded shared ownership in the snapshot system.
#![allow(clippy::arc_with_non_send_sync)]

use crate::collections::map::{HashMap, HashSet};
use crate::debug_trace::debug_record_scope_invalidation;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::hash::Hash;
use std::marker::PhantomData;
use std::ops::Deref;
use std::rc::{Rc, Weak as RcWeak};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use crate::snapshot_id_set::{SnapshotId, SnapshotIdSet};
use crate::snapshot_pinning::lowest_pinned_snapshot;
use crate::snapshot_v2::{
    advance_global_snapshot, allocate_record_id, current_snapshot, AnySnapshot, GlobalSnapshot,
};
use crate::{
    runtime, with_current_composer_opt, RecomposeScope, RecomposeScopeInner, RuntimeHandle,
    ScopeId, StateId,
};

pub(crate) const PREEXISTING_SNAPSHOT_ID: SnapshotId = 1;

const INVALID_SNAPSHOT_ID: SnapshotId = 0;

/// Maximum snapshot ID used to mark records as invisible during initialization
const SNAPSHOT_ID_MAX: SnapshotId = usize::MAX;

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug, Default)]
pub struct ObjectId(pub(crate) usize);

impl ObjectId {
    pub(crate) fn new<T: ?Sized + 'static>(object: &Arc<T>) -> Self {
        Self(Arc::as_ptr(object) as *const () as usize)
    }

    #[inline]
    pub(crate) fn as_usize(self) -> usize {
        self.0
    }
}

/// A record in the state history chain.
///
/// # Thread Safety
/// Contains `Cell<T>` which is not `Send`/`Sync`. This is safe because state records
/// are accessed only from the UI thread via thread-local snapshot system. The `Rc`
/// is used for cheap cloning and shared ownership within a single thread.
pub struct StateRecord {
    snapshot_id: Cell<SnapshotId>,
    tombstone: Cell<bool>,
    next: Cell<Option<Rc<StateRecord>>>,
    value: RefCell<Option<Box<dyn Any>>>,
}

#[derive(Debug)]
struct StateReadFailure {
    state_id: ObjectId,
    snapshot_id: SnapshotId,
    fresh_snapshot_id: SnapshotId,
    fresh_invalid: SnapshotIdSet,
    record_chain: Vec<(SnapshotId, bool)>,
}

impl std::fmt::Display for StateReadFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Reading a state that was created after the snapshot was taken or in a snapshot that has not yet been applied\n\
             state={:?}, snapshot_id={}, fresh_snapshot_id={}, fresh_invalid={:?}\n\
             record_chain={:?}",
            self.state_id,
            self.snapshot_id,
            self.fresh_snapshot_id,
            self.fresh_invalid,
            self.record_chain
        )
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum StateRecordValueError {
    MissingOrWrongType { expected: &'static str },
}

impl StateRecord {
    pub(crate) fn new<T: Any>(
        snapshot_id: SnapshotId,
        value: T,
        next: Option<Rc<StateRecord>>,
    ) -> Rc<Self> {
        Rc::new(Self {
            snapshot_id: Cell::new(snapshot_id),
            tombstone: Cell::new(false),
            next: Cell::new(next),
            value: RefCell::new(Some(Box::new(value))),
        })
    }

    #[inline]
    pub(crate) fn snapshot_id(&self) -> SnapshotId {
        self.snapshot_id.get()
    }

    #[inline]
    pub(crate) fn set_snapshot_id(&self, id: SnapshotId) {
        self.snapshot_id.set(id);
    }

    #[inline]
    pub(crate) fn next(&self) -> Option<Rc<StateRecord>> {
        self.next.take().inspect(|record| {
            self.next.set(Some(Rc::clone(record)));
        })
    }

    #[inline]
    pub(crate) fn set_next(&self, next: Option<Rc<StateRecord>>) {
        self.next.set(next);
    }

    #[inline]
    pub(crate) fn is_tombstone(&self) -> bool {
        self.tombstone.get()
    }

    #[inline]
    pub(crate) fn set_tombstone(&self, tombstone: bool) {
        self.tombstone.set(tombstone);
    }

    pub(crate) fn clear_value(&self) {
        self.value.borrow_mut().take();
    }

    pub(crate) fn replace_value<T: Any>(&self, new_value: T) {
        *self.value.borrow_mut() = Some(Box::new(new_value));
    }

    pub(crate) fn with_value<T: Any, R>(&self, f: impl FnOnce(&T) -> R) -> R {
        self.try_with_value(f)
            .unwrap_or_else(|| panic!("StateRecord value missing or wrong type"))
    }

    pub(crate) fn try_with_value<T: Any, R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        let guard = self.value.borrow();
        let value = guard.as_ref().and_then(|boxed| boxed.downcast_ref::<T>())?;
        Some(f(value))
    }

    /// Clears the value from this record to free memory.
    /// Used when marking records as reusable - clears the value to reduce memory usage.
    #[cfg(test)]
    pub(crate) fn clear_for_reuse(&self) {
        self.clear_value();
    }

    /// Copies the value from the source record into this record.
    ///
    /// This is used during record reuse to copy valid data from a readable record
    /// into a reused record, and during cleanup to preserve data in records being
    /// marked as INVALID_SNAPSHOT.
    ///
    pub(crate) fn assign_value<T: Any + Clone>(
        &self,
        source: &StateRecord,
    ) -> Result<(), StateRecordValueError> {
        let cloned_value = source.try_with_value(|value: &T| value.clone()).ok_or(
            StateRecordValueError::MissingOrWrongType {
                expected: std::any::type_name::<T>(),
            },
        )?;
        self.replace_value(cloned_value);
        Ok(())
    }
}

impl Drop for StateRecord {
    fn drop(&mut self) {
        // Prevent recursive drop of deep chains which can cause stack overflow.
        // We iteratively detach and drop the next record if we are the sole owner.
        let mut next = self.next.take();
        while let Some(node) = next {
            match Rc::try_unwrap(node) {
                Ok(record) => {
                    // We were the last owner. Take its next pointer to continue the loop.
                    // The record itself will be dropped here, but since next is None,
                    // it won't recurse.
                    next = record.next.take();
                }
                Err(_) => {
                    // Someone else holds a reference to this node.
                    // The chain destruction stops here.
                    break;
                }
            }
        }
    }
}

/// Owns the mutable head pointer for a state's record chain.
///
/// Snapshot code clones the current head, prepends new records, and swaps in
/// replacement heads frequently. Centralizing those operations keeps the
/// `RefCell<Rc<StateRecord>>` borrow protocol out of the higher-level state
/// logic.
struct CurrentRecord {
    head: RefCell<Rc<StateRecord>>,
}

impl CurrentRecord {
    fn new(head: Rc<StateRecord>) -> Self {
        Self {
            head: RefCell::new(head),
        }
    }

    fn clone_head(&self) -> Rc<StateRecord> {
        self.head.borrow().clone()
    }

    fn replace(&self, new_head: Rc<StateRecord>) {
        *self.head.borrow_mut() = new_head;
    }

    fn prepend(&self, record: Rc<StateRecord>) {
        let current_head = self.clone_head();
        record.set_next(Some(current_head));
        self.replace(record);
    }
}

#[inline]
fn record_is_valid_for(
    record: &Rc<StateRecord>,
    snapshot_id: SnapshotId,
    invalid: &SnapshotIdSet,
) -> bool {
    if record.is_tombstone() {
        return false;
    }

    let candidate = record.snapshot_id();
    if candidate == INVALID_SNAPSHOT_ID || candidate > snapshot_id {
        return false;
    }

    candidate == snapshot_id || !invalid.get(candidate)
}

pub(crate) fn readable_record_for(
    head: &Rc<StateRecord>,
    snapshot_id: SnapshotId,
    invalid: &SnapshotIdSet,
) -> Option<Rc<StateRecord>> {
    // Find the highest valid record in the chain.
    // We must scan the full chain because reused records may not be prepended
    // as the head but still need to be found (e.g., after writes using record reuse).
    let mut best: Option<Rc<StateRecord>> = None;
    let mut cursor = Some(Rc::clone(head));

    while let Some(record) = cursor {
        if record_is_valid_for(&record, snapshot_id, invalid) {
            let replace = best
                .as_ref()
                .map(|current| current.snapshot_id() < record.snapshot_id())
                .unwrap_or(true);
            if replace {
                best = Some(Rc::clone(&record));
            }
        }
        cursor = record.next();
    }

    best
}

/// Finds the youngest record in the chain, or the first one matching the predicate.
///
/// Searches the record chain starting from the given head:
/// - If a record matches the predicate, returns it immediately
/// - Otherwise, tracks the youngest record (highest snapshot_id) and returns it
fn find_youngest_or<F>(head: &Rc<StateRecord>, predicate: F) -> Rc<StateRecord>
where
    F: Fn(&Rc<StateRecord>) -> bool,
{
    let mut current = Some(Rc::clone(head));
    let mut youngest = Rc::clone(head);

    while let Some(record) = current {
        if predicate(&record) {
            return record;
        }
        if youngest.snapshot_id() < record.snapshot_id() {
            youngest = Rc::clone(&record);
        }
        current = record.next();
    }

    youngest
}

/// Finds a StateRecord that can be safely reused because no open snapshot can see it.
///
/// Returns a record that either:
/// 1. Is marked as INVALID_SNAPSHOT (abandoned/tombstone)
/// 2. Is obscured by a newer record (both are below the reuse limit)
///
/// The reuse limit is `lowest_pinned_snapshot - 1`, meaning any record with a snapshot ID
/// at or below this value cannot be selected by any currently open snapshot.
///
/// Note: PREEXISTING records (snapshot_id=1) are never reused to maintain the ability
/// for all snapshots to read the initial state.
pub(crate) fn used_locked(head: &Rc<StateRecord>) -> Option<Rc<StateRecord>> {
    let mut current = Some(Rc::clone(head));
    let mut valid_record: Option<Rc<StateRecord>> = None;

    // Calculate reuse limit: records below this ID are invisible to all open snapshots
    let reuse_limit = lowest_pinned_snapshot()
        .map(|lowest| lowest.saturating_sub(1))
        .unwrap_or_else(|| allocate_record_id().saturating_sub(1));

    let invalid = SnapshotIdSet::EMPTY;

    while let Some(record) = current {
        let current_id = record.snapshot_id();

        // Never reuse PREEXISTING records - they must always be available as a fallback
        if current_id == PREEXISTING_SNAPSHOT_ID {
            current = record.next();
            continue;
        }

        // Fast path: records marked INVALID_SNAPSHOT can be reused immediately
        if current_id == INVALID_SNAPSHOT_ID {
            return Some(record);
        }

        if record.is_tombstone() && current_id < reuse_limit {
            return Some(record);
        }

        // Check if this record is valid for snapshots at or below the reuse limit
        if record_is_valid_for(&record, reuse_limit, &invalid) {
            if let Some(ref existing) = valid_record {
                // We found two valid records below the reuse limit.
                // This means one obscures the other - return the older one for reuse.
                return Some(if current_id < existing.snapshot_id() {
                    record
                } else {
                    Rc::clone(existing)
                });
            } else {
                // First valid record below reuse limit - keep looking
                valid_record = Some(record.clone());
            }
        }

        current = record.next();
    }

    // No reusable record found
    None
}

/// Creates a new overwritable record for a state object, reusing an existing record if possible.
///
/// The record is initially marked with SNAPSHOT_ID_MAX to make it invisible to all snapshots
/// during initialization. The caller must:
/// 1. Copy/set the desired value into the record
/// 2. Set the final snapshot_id
///
/// Returns a record that is either:
/// - A reused record (if `used_locked()` found one), marked with SNAPSHOT_ID_MAX
/// - A newly created record, prepended to the state's record chain via `prepend_state_record()`
pub(crate) fn new_overwritable_record_locked(state: &dyn StateObject) -> Rc<StateRecord> {
    let state_head = state.first_record();

    // Try to reuse an existing record
    if let Some(reusable) = used_locked(&state_head) {
        // Mark as invisible during initialization
        reusable.set_snapshot_id(SNAPSHOT_ID_MAX);
        return reusable;
    }

    // No reusable record found; create an invisible record and let the caller
    // replace the unit initializer before publishing the final snapshot id.
    let new_record = StateRecord::new(
        SNAPSHOT_ID_MAX,
        (),
        None, // next will be set by prepend_state_record
    );

    // Prepend the new record to the state's chain
    state.prepend_state_record(Rc::clone(&new_record));

    new_record
}

/// Creates an overwritable record and ensures it is the head of the record chain.
///
/// This is used for global snapshot writes where the newest record must be at the head
/// to keep tombstoning logic consistent. Reused records are unlinked from their current
/// position before being prepended.
pub(crate) fn new_overwritable_record_as_head_locked(state: &dyn StateObject) -> Rc<StateRecord> {
    let head = state.first_record();

    if let Some(reusable) = used_locked(&head) {
        reusable.set_snapshot_id(SNAPSHOT_ID_MAX);

        if !Rc::ptr_eq(&head, &reusable) {
            let mut cursor = Some(Rc::clone(&head));
            let mut unlinked = false;

            while let Some(node) = cursor {
                let next = node.next();
                if let Some(next_record) = next {
                    if Rc::ptr_eq(&next_record, &reusable) {
                        node.set_next(reusable.next());
                        unlinked = true;
                        break;
                    }
                    cursor = Some(next_record);
                } else {
                    break;
                }
            }

            if !unlinked {
                debug_assert!(
                    false,
                    "new_overwritable_record_as_head_locked: reusable record not found in chain"
                );
                let new_record = StateRecord::new(SNAPSHOT_ID_MAX, (), None);
                state.prepend_state_record(Rc::clone(&new_record));
                return new_record;
            }

            state.prepend_state_record(Rc::clone(&reusable));
        }

        return reusable;
    }

    let new_record = StateRecord::new(SNAPSHOT_ID_MAX, (), None);
    state.prepend_state_record(Rc::clone(&new_record));
    new_record
}

/// Overwrites unused records in a state object's record chain with data from retained records.
///
/// This function implements Kotlin's `overwriteUnusedRecordsLocked` to reclaim memory by:
/// 1. Finding records below the reuse limit (records invisible to all open snapshots)
/// 2. Keeping the highest record below the reuse limit (so lowest pinned snapshot can see it)
/// 3. Marking older obscured records as INVALID_SNAPSHOT and copying valid data into them
///
/// The valid data is copied from a "young" record (above reuse limit) to ensure that if
/// an invalidated record is somehow accessed, it contains current valid data rather than
/// cleared/garbage values.
///
/// Returns `true` if the state has multiple retained records and should stay in extraStateObjects,
/// `false` if it can be removed from tracking.
pub(crate) fn overwrite_unused_records_locked<T: Any + Clone>(state: &dyn StateObject) -> bool {
    let head = state.first_record();
    let mut current = Some(Rc::clone(&head));
    let mut overwrite_record: Option<Rc<StateRecord>> = None;
    let mut valid_record: Option<Rc<StateRecord>> = None;

    // Calculate reuse limit: records below this ID are invisible to all open snapshots
    // Mirrors Kotlin's: val reuseLimit = pinningTable.lowestOrDefault(nextSnapshotId)
    let reuse_limit =
        lowest_pinned_snapshot().unwrap_or_else(crate::snapshot_v2::peek_next_snapshot_id);

    let mut retained_records = 0;

    while let Some(record) = current {
        let current_id = record.snapshot_id();

        if current_id == INVALID_SNAPSHOT_ID {
            // Already invalid, skip
        } else if current_id < reuse_limit {
            if valid_record.is_none() {
                // If any records are below reuse_limit, we must keep the highest one
                // so the lowest snapshot can select it
                valid_record = Some(Rc::clone(&record));
                retained_records += 1;
            } else {
                // We have two records below the reuse limit - one obscures the other
                // Overwrite the older one (lower snapshot_id)
                let Some(valid) = valid_record.as_ref() else {
                    valid_record = Some(Rc::clone(&record));
                    retained_records += 1;
                    current = record.next();
                    continue;
                };
                let record_to_overwrite = if current_id < valid.snapshot_id() {
                    Rc::clone(&record)
                } else {
                    // Keep current as valid, overwrite the previous valid
                    let to_overwrite = Rc::clone(valid);
                    valid_record = Some(Rc::clone(&record));
                    to_overwrite
                };

                // Lazily find a young record to copy data from
                let source_record = overwrite_record.get_or_insert_with(|| {
                    find_youngest_or(&head, |r| r.snapshot_id() >= reuse_limit)
                });

                // Mark the old record as invalid and copy valid data into it
                record_to_overwrite.set_snapshot_id(INVALID_SNAPSHOT_ID);
                if let Err(error) = record_to_overwrite.assign_value::<T>(source_record) {
                    log::error!(
                        "snapshot cleanup could not copy retained state record value for state {:?}: {:?}",
                        state.object_id(),
                        error
                    );
                }
            }
        } else {
            // Record is above reuse limit - it's still visible and must be kept
            retained_records += 1;
        }

        current = record.next();
    }

    // Return true if we have multiple records that must be retained
    // (state should stay in extraStateObjects for future cleanup)
    retained_records > 1
}

fn active_snapshot() -> AnySnapshot {
    current_snapshot().unwrap_or_else(|| AnySnapshot::Global(GlobalSnapshot::get_or_create()))
}

pub(crate) trait MutationPolicy<T>: Send + Sync {
    fn equivalent(&self, a: &T, b: &T) -> bool;
    fn merge(&self, _previous: &T, _current: &T, _applied: &T) -> Option<T> {
        None
    }
}

pub(crate) struct NeverEqual;

impl<T> MutationPolicy<T> for NeverEqual {
    fn equivalent(&self, _a: &T, _b: &T) -> bool {
        false
    }
}

pub(crate) struct StructuralEqual;

impl<T: PartialEq> MutationPolicy<T> for StructuralEqual {
    fn equivalent(&self, a: &T, b: &T) -> bool {
        a == b
    }
}

pub trait StateObject: Any {
    fn object_id(&self) -> ObjectId;
    fn first_record(&self) -> Rc<StateRecord>;
    fn try_readable_record(
        &self,
        snapshot_id: SnapshotId,
        invalid: &SnapshotIdSet,
    ) -> Option<Rc<StateRecord>>;
    fn readable_record(&self, snapshot_id: SnapshotId, invalid: &SnapshotIdSet) -> Rc<StateRecord>;

    /// Prepends a record to the head of the record chain.
    /// This is used when reusing records - the record's next pointer is updated to point to the current head,
    /// and the head is updated to point to the new record.
    fn prepend_state_record(&self, record: Rc<StateRecord>);

    fn merge_records(
        &self,
        _previous: Rc<StateRecord>,
        _current: Rc<StateRecord>,
        _applied: Rc<StateRecord>,
    ) -> Option<Rc<StateRecord>> {
        None
    }

    fn commit_merged_record(&self, _merged: Rc<StateRecord>) -> Result<SnapshotId, &'static str> {
        Err("StateObject does not support merged record commits")
    }
    fn promote_record(&self, child_id: SnapshotId) -> Result<(), &'static str>;

    /// Overwrites unused records in this state's record chain with valid data.
    ///
    /// Returns `true` if the state has multiple retained records and should stay in extraStateObjects,
    /// `false` if it can be removed from tracking.
    fn overwrite_unused_records(&self) -> bool {
        false // Default implementation for states that don't support cleanup
    }

    fn observation_lease(&self) -> Option<Box<dyn Any>> {
        None
    }

    /// Downcast to Any for testing/debugging purposes.
    fn as_any(&self) -> &dyn Any;
}

pub(crate) struct SnapshotMutableState<T> {
    head: CurrentRecord,
    policy: Arc<dyn MutationPolicy<T>>,
    id: ObjectId,
    weak_self: Mutex<Option<Weak<Self>>>,
    apply_observers: Mutex<Vec<Box<dyn Fn() + 'static>>>,
    read_observation_count: Cell<usize>,
    scope_observation_count: Cell<usize>,
    subscriber_callbacks: RefCell<Vec<RcWeak<dyn Fn()>>>,
}

struct StateObservationLease<T: Clone + 'static> {
    state: Weak<SnapshotMutableState<T>>,
}

impl<T: Clone + 'static> Drop for StateObservationLease<T> {
    fn drop(&mut self) {
        if let Some(state) = self.state.upgrade() {
            let count = state
                .read_observation_count
                .get()
                .checked_sub(1)
                .expect("state observation lease count underflow");
            state.read_observation_count.set(count);
        }
    }
}

impl<T> SnapshotMutableState<T> {
    fn assert_chain_integrity(&self, caller: &str, snapshot_context: Option<SnapshotId>) {
        if !should_check_chain_integrity() {
            return;
        }
        let head = self.head.clone_head();
        let mut cursor = Some(head);
        let mut seen: HashSet<usize> = HashSet::default();
        let mut ids = Vec::new();

        while let Some(record) = cursor {
            let addr = Rc::as_ptr(&record) as usize;
            assert!(
                seen.insert(addr),
                "SnapshotMutableState::{} detected duplicate/cycle at record {:p} for state {:?} (snapshot_context={:?}, chain_ids={:?})",
                caller,
                Rc::as_ptr(&record),
                self.id,
                snapshot_context,
                ids
            );
            ids.push(record.snapshot_id());
            cursor = record.next();
        }

        assert!(
            !ids.is_empty(),
            "SnapshotMutableState::{} finished integrity scan with empty id list for state {:?} (snapshot_context={:?})",
            caller,
            self.id,
            snapshot_context
        );
    }
}

fn should_check_chain_integrity() -> bool {
    #[cfg(debug_assertions)]
    {
        true
    }

    #[cfg(not(debug_assertions))]
    {
        crate::env_flag!("CRANPOSE_ASSERT_STATE_CHAIN")
    }
}

impl<T: Clone + 'static> SnapshotMutableState<T> {
    fn record_chain_debug(&self) -> Vec<(SnapshotId, bool)> {
        let mut chain_ids = Vec::new();
        let mut cursor = Some(self.first_record());
        while let Some(record) = cursor {
            chain_ids.push((record.snapshot_id(), record.is_tombstone()));
            cursor = record.next();
        }
        chain_ids
    }

    fn readable_record_for_active_snapshot(&self) -> Result<Rc<StateRecord>, StateReadFailure> {
        let snapshot = active_snapshot();
        if let Some(state) = self.upgrade_self() {
            snapshot.record_read(&*state);
        }

        let snapshot_id = snapshot.snapshot_id();
        let invalid = snapshot.invalid();

        if let Some(record) = self.readable_for(snapshot_id, &invalid) {
            return Ok(record);
        }

        let fresh_snapshot = active_snapshot();
        let fresh_id = fresh_snapshot.snapshot_id();
        let fresh_invalid = fresh_snapshot.invalid();

        if let Some(record) = self.readable_for(fresh_id, &fresh_invalid) {
            return Ok(record);
        }

        let global = GlobalSnapshot::get_or_create();
        let global_id = global.snapshot_id();
        let global_invalid = global.invalid();

        if let Some(record) = self.readable_for(global_id, &global_invalid) {
            return Ok(record);
        }

        Err(StateReadFailure {
            state_id: self.id,
            snapshot_id,
            fresh_snapshot_id: fresh_id,
            fresh_invalid,
            record_chain: self.record_chain_debug(),
        })
    }

    fn readable_for(
        &self,
        snapshot_id: SnapshotId,
        invalid: &SnapshotIdSet,
    ) -> Option<Rc<StateRecord>> {
        let head = self.first_record();
        readable_record_for(&head, snapshot_id, invalid)
    }

    fn writable_record(&self, snapshot_id: SnapshotId, invalid: &SnapshotIdSet) -> Rc<StateRecord> {
        let readable = match self.readable_for(snapshot_id, invalid) {
            Some(record) => record,
            None => {
                let current_head = self.head.clone_head();
                let refreshed = readable_record_for(&current_head, snapshot_id, invalid);
                let source = refreshed.unwrap_or_else(|| current_head.clone());

                // Create a new record
                // Record reuse is NOT used here to preserve history for conflict detection
                // Reuse happens during cleanup (overwrite_unused_records_locked)
                let cloned_value = source.with_value(|value: &T| value.clone());
                let new_head = StateRecord::new(snapshot_id, cloned_value, Some(current_head));
                self.head.replace(new_head.clone());
                self.assert_chain_integrity("writable_record(recover)", Some(snapshot_id));
                return new_head;
            }
        };

        if readable.snapshot_id() == snapshot_id {
            return readable;
        }

        let refreshed = {
            let current_head = self.head.clone_head();
            let refreshed = readable_record_for(&current_head, snapshot_id, invalid).unwrap_or_else(
                || {
                    panic!(
                        "SnapshotMutableState::writable_record failed to locate refreshed readable record (state {:?}, snapshot_id={}, invalid={:?})",
                        self.id, snapshot_id, invalid
                    )
                },
            );

            if refreshed.snapshot_id() == snapshot_id {
                return refreshed;
            }

            Rc::clone(&refreshed)
        };

        let overwritable = new_overwritable_record_locked(self);
        if let Err(error) = overwritable.assign_value::<T>(&refreshed) {
            log::error!(
                "snapshot writable record could not copy refreshed value for state {:?}: {:?}",
                self.id,
                error
            );
        }
        overwritable.set_snapshot_id(snapshot_id);
        overwritable.set_tombstone(false);

        self.assert_chain_integrity("writable_record(reuse)", Some(snapshot_id));

        overwritable
    }

    pub(crate) fn new_in_arc(initial: T, policy: Arc<dyn MutationPolicy<T>>) -> Arc<Self> {
        let snapshot = active_snapshot();
        let snapshot_id = snapshot.snapshot_id();

        let tail = StateRecord::new(PREEXISTING_SNAPSHOT_ID, initial.clone(), None);
        let head = StateRecord::new(snapshot_id, initial, Some(tail));

        let mut state = Arc::new(Self {
            head: CurrentRecord::new(head),
            policy,
            id: ObjectId::default(),
            weak_self: Mutex::new(None),
            apply_observers: Mutex::new(Vec::new()),
            read_observation_count: Cell::new(0),
            scope_observation_count: Cell::new(0),
            subscriber_callbacks: RefCell::new(Vec::new()),
        });

        let id = ObjectId::new(&state);
        if let Some(state_inner) = Arc::get_mut(&mut state) {
            state_inner.id = id;
        }

        *state.lock_weak_self() = Some(Arc::downgrade(&state));

        // No need to advance the global snapshot for initial state creation

        state
    }

    pub(crate) fn add_apply_observer(&self, observer: Box<dyn Fn() + 'static>) {
        self.lock_apply_observers().push(observer);
    }

    fn acquire_observation_lease(&self) -> Option<Box<dyn Any>> {
        let state = self.lock_weak_self().as_ref()?.clone();
        let was_empty = !self.has_subscribers();
        let count = self
            .read_observation_count
            .get()
            .checked_add(1)
            .expect("state observation lease count overflow");
        self.read_observation_count.set(count);
        if was_empty {
            notify_subscriber_callbacks(&self.subscriber_callbacks);
        }
        Some(Box::new(StateObservationLease { state }))
    }

    fn add_scope_observer(&self) -> bool {
        let was_empty = !self.has_subscribers();
        let count = self
            .scope_observation_count
            .get()
            .checked_add(1)
            .expect("state scope observation count overflow");
        self.scope_observation_count.set(count);
        was_empty
    }

    fn remove_scope_observers(&self, count: usize) {
        if count == 0 {
            return;
        }
        let remaining = self
            .scope_observation_count
            .get()
            .checked_sub(count)
            .expect("state scope observation count underflow");
        self.scope_observation_count.set(remaining);
    }

    fn has_subscribers(&self) -> bool {
        self.read_observation_count.get() > 0 || self.scope_observation_count.get() > 0
    }

    fn subscriber_callback(&self, callback: Rc<dyn Fn()>, notify: bool) {
        register_subscriber_callback(&self.subscriber_callbacks, &callback);
        if notify {
            callback();
        }
        drop(callback);
        self.subscriber_callbacks
            .borrow_mut()
            .retain(|callback| callback.upgrade().is_some());
    }

    #[cfg(test)]
    fn subscriber_callback_count(&self) -> usize {
        self.subscriber_callbacks.borrow().len()
    }

    fn notify_subscribers(&self) {
        notify_subscriber_callbacks(&self.subscriber_callbacks);
    }

    fn notify_applied(&self) {
        let observers = self.lock_apply_observers();
        for observer in observers.iter() {
            observer();
        }
    }

    fn lock_weak_self(&self) -> MutexGuard<'_, Option<Weak<Self>>> {
        self.weak_self
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_apply_observers(&self) -> MutexGuard<'_, Vec<Box<dyn Fn() + 'static>>> {
        self.apply_observers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn upgrade_self(&self) -> Option<Arc<Self>> {
        self.lock_weak_self()
            .as_ref()
            .and_then(|weak| weak.upgrade())
    }

    #[inline]
    pub(crate) fn id(&self) -> ObjectId {
        self.id
    }

    pub(crate) fn try_with_value<R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        let record = self.readable_record_for_active_snapshot().ok()?;
        record.try_with_value(f)
    }

    pub(crate) fn try_get(&self) -> Option<T> {
        self.try_with_value(Clone::clone)
    }

    pub(crate) fn get(&self) -> T {
        let record = self
            .readable_record_for_active_snapshot()
            .unwrap_or_else(|failure| panic!("{failure}"));
        record.with_value(|value: &T| value.clone())
    }

    pub(crate) fn set(&self, new_value: T) -> bool {
        // Debug-only check: warn if modifying state in event handler without proper snapshot
        #[cfg(debug_assertions)]
        {
            let in_handler = crate::in_event_handler();
            let in_snapshot = crate::in_applied_snapshot();
            if in_handler && !in_snapshot {
                log::warn!(
                    target: "cranpose::state",
                    "State modified in event handler without run_in_mutable_snapshot; \
                     this can make updates invisible to other contexts. Wrap the handler \
                     in run_in_mutable_snapshot() or dispatch_ui_event(). State: {:?}",
                    self.id
                );
            }
        }

        let snapshot = active_snapshot();
        let snapshot_id = snapshot.snapshot_id();

        match &snapshot {
            AnySnapshot::Global(global) => {
                let invalid = snapshot.invalid();
                let equivalent = self
                    .readable_for(snapshot_id, &invalid)
                    .map(|record| {
                        record.with_value(|current: &T| self.policy.equivalent(current, &new_value))
                    })
                    .unwrap_or(false);
                if equivalent {
                    return false;
                }

                if global.has_pending_children() {
                    panic!(
                        "SnapshotMutableState::set attempted global write while pending children {:?} exist (state {:?}, snapshot_id={})",
                        global.pending_children(),
                        self.id,
                        snapshot_id
                    );
                }

                let mut written_state: Option<Arc<dyn StateObject>> = None;
                if let Some(state) = self.upgrade_self() {
                    let trait_object: Arc<dyn StateObject> = state.clone();
                    snapshot.record_write(trait_object.clone());
                    written_state = Some(trait_object);
                }
                mark_update_write(self.id);

                let new_id = allocate_record_id();
                let record = new_overwritable_record_as_head_locked(self);
                record.replace_value(new_value);
                record.set_snapshot_id(new_id);
                record.set_tombstone(false);
                advance_global_snapshot(new_id);
                self.assert_chain_integrity("set(global-push)", Some(snapshot_id));

                if !global.has_pending_children() {
                    let mut cursor = record.next();
                    while let Some(node) = cursor {
                        if !node.is_tombstone() && node.snapshot_id() != PREEXISTING_SNAPSHOT_ID {
                            node.clear_value();
                            node.set_tombstone(true);
                        }
                        cursor = node.next();
                    }
                    self.assert_chain_integrity("set(global-tombstone)", Some(snapshot_id));
                }

                if let Some(modified) = written_state.as_ref() {
                    crate::snapshot_v2::notify_apply_observers(
                        std::slice::from_ref(modified),
                        new_id,
                    );
                }
            }
            AnySnapshot::Mutable(_)
            | AnySnapshot::NestedMutable(_)
            | AnySnapshot::TransparentMutable(_) => {
                let invalid = snapshot.invalid();
                let equivalent = self
                    .readable_for(snapshot_id, &invalid)
                    .map(|record| {
                        record.with_value(|current: &T| self.policy.equivalent(current, &new_value))
                    })
                    .unwrap_or(false);
                if equivalent {
                    return false;
                }

                if let Some(state) = self.upgrade_self() {
                    let trait_object: Arc<dyn StateObject> = state.clone();
                    snapshot.record_write(trait_object);
                }
                mark_update_write(self.id);

                let record = self.writable_record(snapshot_id, &invalid);
                record.replace_value(new_value);
                self.assert_chain_integrity("set(child-writable)", Some(snapshot_id));
            }
            AnySnapshot::Readonly(_)
            | AnySnapshot::NestedReadonly(_)
            | AnySnapshot::TransparentReadonly(_) => {
                panic!("Cannot write to a read-only snapshot");
            }
        }

        // Retain the prior record chain so concurrent readers never observe freed nodes.
        // Compose proper prunes when it can prove no readers exist; for now we keep
        // the historical chain with tombstoned values to avoid use-after-free crashes
        // under heavy UI load.
        true
    }
}

thread_local! {
    static ACTIVE_UPDATES: RefCell<HashSet<ObjectId>> = RefCell::new(HashSet::default());
    static PENDING_WRITES: RefCell<HashSet<ObjectId>> = RefCell::new(HashSet::default());
}

pub(crate) struct UpdateScope {
    id: ObjectId,
    finished: bool,
}

impl UpdateScope {
    pub(crate) fn new(id: ObjectId) -> Self {
        ACTIVE_UPDATES.with(|active| {
            active.borrow_mut().insert(id);
        });
        PENDING_WRITES.with(|pending| {
            pending.borrow_mut().remove(&id);
        });
        Self {
            id,
            finished: false,
        }
    }

    pub(crate) fn finish(mut self) -> bool {
        self.finished = true;
        ACTIVE_UPDATES.with(|active| {
            active.borrow_mut().remove(&self.id);
        });
        PENDING_WRITES.with(|pending| pending.borrow_mut().remove(&self.id))
    }
}

impl Drop for UpdateScope {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        ACTIVE_UPDATES.with(|active| {
            active.borrow_mut().remove(&self.id);
        });
        PENDING_WRITES.with(|pending| {
            pending.borrow_mut().remove(&self.id);
        });
    }
}

fn mark_update_write(id: ObjectId) {
    ACTIVE_UPDATES.with(|active| {
        if active.borrow().contains(&id) {
            PENDING_WRITES.with(|pending| {
                pending.borrow_mut().insert(id);
            });
        }
    });
}

impl<T: Clone + 'static> SnapshotMutableState<T> {
    /// Try to find a readable record, returning None if no valid record exists.
    fn try_readable_record(
        &self,
        snapshot_id: SnapshotId,
        invalid: &SnapshotIdSet,
    ) -> Option<Rc<StateRecord>> {
        self.readable_for(snapshot_id, invalid)
    }
}

impl<T: Clone + 'static> StateObject for SnapshotMutableState<T> {
    fn object_id(&self) -> ObjectId {
        self.id
    }

    fn first_record(&self) -> Rc<StateRecord> {
        self.head.clone_head()
    }

    fn try_readable_record(
        &self,
        snapshot_id: SnapshotId,
        invalid: &SnapshotIdSet,
    ) -> Option<Rc<StateRecord>> {
        self.try_readable_record(snapshot_id, invalid)
    }

    fn readable_record(&self, snapshot_id: SnapshotId, invalid: &SnapshotIdSet) -> Rc<StateRecord> {
        self.try_readable_record(snapshot_id, invalid)
            .unwrap_or_else(|| {
                panic!(
                    "SnapshotMutableState::readable_record returned null (state={:?}, snapshot_id={})",
                    self.id, snapshot_id
                )
            })
    }

    fn prepend_state_record(&self, record: Rc<StateRecord>) {
        self.head.prepend(record);
    }

    fn observation_lease(&self) -> Option<Box<dyn Any>> {
        self.acquire_observation_lease()
    }

    fn merge_records(
        &self,
        previous: Rc<StateRecord>,
        current: Rc<StateRecord>,
        applied: Rc<StateRecord>,
    ) -> Option<Rc<StateRecord>> {
        let Some(current_value) = current.try_with_value(|value: &T| value.clone()) else {
            log::error!(
                "SnapshotMutableState::merge_records current record value missing or wrong type (state {:?}, current_id={})",
                self.id,
                current.snapshot_id()
            );
            return None;
        };
        let Some(applied_value) = applied.try_with_value(|value: &T| value.clone()) else {
            log::error!(
                "SnapshotMutableState::merge_records applied record value missing or wrong type (state {:?}, applied_id={})",
                self.id,
                applied.snapshot_id()
            );
            return None;
        };
        if self.policy.equivalent(&current_value, &applied_value) {
            return Some(current);
        }

        let Some(previous_value) = previous.try_with_value(|value: &T| value.clone()) else {
            log::error!(
                "SnapshotMutableState::merge_records previous record value missing or wrong type (state {:?}, previous_id={})",
                self.id,
                previous.snapshot_id()
            );
            return None;
        };
        let merged = self
            .policy
            .merge(&previous_value, &current_value, &applied_value)?;

        Some(StateRecord::new(applied.snapshot_id(), merged, None))
    }

    fn promote_record(&self, child_id: SnapshotId) -> Result<(), &'static str> {
        let head = self.first_record();
        let mut cursor = Some(head);
        while let Some(record) = cursor {
            if record.snapshot_id() == child_id {
                let Some(cloned) = record.try_with_value(|value: &T| value.clone()) else {
                    log::error!(
                        "SnapshotMutableState::promote_record child record value missing or wrong type (state {:?}, child_id={})",
                        self.id,
                        child_id
                    );
                    return Err("child record value missing or wrong type");
                };
                // Publish through the reuse primitive: every apply retires
                // the previous head, so an unconditional allocation here
                // grows the record chain by one per UI event — the chain is
                // scanned linearly on every read, so frames decay for the
                // whole session (user-visible as sinking fps under repeated
                // handle drags).
                let new_id = allocate_record_id();
                let promoted = new_overwritable_record_as_head_locked(self);
                promoted.replace_value(cloned);
                promoted.set_tombstone(false);
                promoted.set_snapshot_id(new_id);
                advance_global_snapshot(new_id);
                self.notify_applied();
                self.assert_chain_integrity("promote_record", Some(child_id));
                return Ok(());
            }
            cursor = record.next();
        }
        log::error!(
            "SnapshotMutableState::promote_record missing child record (state {:?}, child_id={})",
            self.id,
            child_id
        );
        Err("missing child record")
    }

    fn commit_merged_record(&self, merged: Rc<StateRecord>) -> Result<SnapshotId, &'static str> {
        let Some(value) = merged.try_with_value(|value: &T| value.clone()) else {
            log::error!(
                "SnapshotMutableState::commit_merged_record merged record value missing or wrong type (state {:?}, merged_id={})",
                self.id,
                merged.snapshot_id()
            );
            return Err("merged record value missing or wrong type");
        };
        // Same reuse discipline as `promote_record`: merged commits happen
        // per conflicting apply and must not grow the chain.
        let new_id = allocate_record_id();
        let committed = new_overwritable_record_as_head_locked(self);
        committed.replace_value(value);
        committed.set_tombstone(false);
        committed.set_snapshot_id(new_id);
        advance_global_snapshot(new_id);
        self.notify_applied();
        self.assert_chain_integrity("commit_merged_record", Some(new_id));
        Ok(new_id)
    }

    fn overwrite_unused_records(&self) -> bool {
        overwrite_unused_records_locked::<T>(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub(crate) struct MutableStateInner<T: Clone + 'static> {
    pub(crate) state: Arc<SnapshotMutableState<T>>,
    pub(crate) watchers: RefCell<HashMap<ScopeId, RcWeak<RecomposeScopeInner>>>,
    runtime: RuntimeHandle,
    state_id: Cell<Option<StateId>>,
}

fn notify_subscriber_callbacks(callbacks: &RefCell<Vec<RcWeak<dyn Fn()>>>) {
    let callbacks_snapshot = std::mem::take(&mut *callbacks.borrow_mut());
    let mut live = Vec::with_capacity(callbacks_snapshot.len());
    for callback in callbacks_snapshot {
        let Some(callback) = callback.upgrade() else {
            continue;
        };
        callback();
        live.push(callback);
    }
    let mut registered = callbacks.borrow_mut();
    registered.retain(|callback| callback.upgrade().is_some());
    for callback in live {
        let callback = Rc::downgrade(&callback);
        if !registered
            .iter()
            .any(|registered| registered.ptr_eq(&callback))
        {
            registered.push(callback);
        }
    }
}

fn register_subscriber_callback(
    callbacks: &RefCell<Vec<RcWeak<dyn Fn()>>>,
    callback: &Rc<dyn Fn()>,
) {
    let callback_weak = Rc::downgrade(callback);
    let mut callbacks = callbacks.borrow_mut();
    callbacks.retain(|callback| callback.upgrade().is_some());
    if !callbacks
        .iter()
        .any(|registered| registered.ptr_eq(&callback_weak))
    {
        callbacks.push(callback_weak);
    }
}

fn shrink_watchers_if_sparse(watchers: &mut HashMap<ScopeId, RcWeak<RecomposeScopeInner>>) {
    let len = watchers.len();
    let capacity = watchers.capacity();
    if capacity > len.saturating_mul(4).max(32) {
        watchers.shrink_to_fit();
    }
}

impl<T: Clone + 'static> MutableStateInner<T> {
    pub(crate) fn new_with_policy(
        value: T,
        runtime: RuntimeHandle,
        policy: Arc<dyn MutationPolicy<T>>,
    ) -> Self {
        Self {
            state: SnapshotMutableState::new_in_arc(value, policy),
            watchers: RefCell::new(HashMap::default()),
            runtime,
            state_id: Cell::new(None),
        }
    }

    pub(crate) fn install_snapshot_observer(&self, state_id: StateId) {
        self.state_id.set(Some(state_id));
        let runtime_handle = self.runtime.clone();
        self.state.add_apply_observer(Box::new(move || {
            let runtime = runtime_handle.clone();
            runtime_handle.enqueue_ui_task(Box::new(move || {
                runtime.with_state_arena(|arena| {
                    let _ = arena.with_typed_opt::<T, _>(state_id, |inner| {
                        inner.invalidate_watchers();
                    });
                });
            }));
        }));
    }

    fn register_scope(&self, scope: &RecomposeScope) -> (bool, bool) {
        let mut watchers = self.watchers.borrow_mut();
        let before = watchers.len();
        watchers.retain(|_, existing| existing.upgrade().is_some());
        self.state.remove_scope_observers(before - watchers.len());
        let registered = match watchers.get(&scope.id()) {
            Some(_) => false,
            _ => {
                watchers.insert(scope.id(), scope.downgrade());
                true
            }
        };
        drop(watchers);
        let became_subscribed = registered && self.state.add_scope_observer();
        (registered, became_subscribed)
    }

    fn has_subscribers(&self) -> bool {
        let mut watchers = self.watchers.borrow_mut();
        let before = watchers.len();
        watchers.retain(|_, existing| existing.upgrade().is_some());
        self.state.remove_scope_observers(before - watchers.len());
        self.state.has_subscribers()
    }

    pub(crate) fn unregister_scope(&self, scope_id: ScopeId) {
        let mut watchers = self.watchers.borrow_mut();
        // Only prune a DEAD entry. Scope ids are derived from the scope
        // allocation's address, so a dropped scope's id can already belong
        // to a live replacement — its Drop-time unregister must not wipe
        // the new scope's registration.
        let removed = if watchers
            .get(&scope_id)
            .is_some_and(|weak| weak.upgrade().is_none())
        {
            watchers.remove(&scope_id);
            shrink_watchers_if_sparse(&mut watchers);
            true
        } else {
            false
        };
        drop(watchers);
        self.state.remove_scope_observers(usize::from(removed));
    }

    fn state_id(&self) -> Option<StateId> {
        self.state_id.get()
    }

    fn invalidate_watchers(&self) {
        let (watchers, removed_count): (Vec<RecomposeScope>, usize) = {
            let mut watchers = self.watchers.borrow_mut();
            let before = watchers.len();
            let mut live = Vec::with_capacity(watchers.len());
            watchers.retain(|_, scope| {
                if let Some(inner) = scope.upgrade() {
                    live.push(RecomposeScope { inner });
                    true
                } else {
                    false
                }
            });
            let removed_count = before - watchers.len();
            shrink_watchers_if_sparse(&mut watchers);
            (live, removed_count)
        };
        self.state.remove_scope_observers(removed_count);

        for watcher in watchers {
            debug_record_scope_invalidation::<T>(watcher.id(), self.state_id.get());
            if let Some(state_id) = self.state_id.get() {
                watcher.invalidate_from_state(state_id);
            } else {
                watcher.invalidate();
            }
        }
    }
}

impl<T: Clone + 'static> Drop for MutableStateInner<T> {
    fn drop(&mut self) {
        self.state
            .remove_scope_observers(self.watchers.get_mut().len());
    }
}

fn register_current_state_scope<T: Clone + 'static>(inner: &MutableStateInner<T>) {
    let Some(Some(scope)) =
        with_current_composer_opt(|composer| composer.current_state_invalidation_scope())
    else {
        return;
    };
    let (registered, became_subscribed) = inner.register_scope(&scope);
    if registered {
        if let Some(state_id) = inner.state_id() {
            scope.record_state_subscription(state_id);
        }
        if became_subscribed {
            inner.state.notify_subscribers();
        }
    }
}

/// Cheap copyable read-only view of a state cell.
pub struct State<T: Clone + 'static> {
    id: StateId,
    runtime_id: runtime::RuntimeId,
    _marker: PhantomData<fn() -> T>,
}

/// Cheap copyable mutable view of a state cell.
///
/// Ownership lives elsewhere: a composition slot, an [`OwnedMutableState`], or
/// the runtime for states created with [`crate::mutableStateOf`] /
/// [`MutableState::with_runtime`].
pub struct MutableState<T: Clone + 'static> {
    id: StateId,
    runtime_id: runtime::RuntimeId,
    _marker: PhantomData<fn() -> T>,
}

/// Owning state handle for reclaimable state cells.
#[derive(Clone)]
pub struct OwnedMutableState<T: Clone + 'static> {
    state: MutableState<T>,
    _lease: Rc<runtime::StateHandleLease>,
    _marker: PhantomData<fn() -> T>,
}

impl<T: Clone + 'static> PartialEq for State<T> {
    fn eq(&self, other: &Self) -> bool {
        self.state_id() == other.state_id() && self.runtime_id() == other.runtime_id()
    }
}

impl<T: Clone + 'static> Eq for State<T> {}

impl<T: Clone + 'static> PartialEq for MutableState<T> {
    fn eq(&self, other: &Self) -> bool {
        self.state_id() == other.state_id() && self.runtime_id() == other.runtime_id()
    }
}

impl<T: Clone + 'static> Eq for MutableState<T> {}

impl<T: Clone + 'static> Copy for State<T> {}

impl<T: Clone + 'static> Clone for State<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Clone + 'static> Copy for MutableState<T> {}

impl<T: Clone + 'static> Clone for MutableState<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Clone + 'static> State<T> {
    fn state_id(&self) -> StateId {
        self.id
    }

    fn runtime_id(&self) -> runtime::RuntimeId {
        self.runtime_id
    }

    fn runtime_handle(&self) -> RuntimeHandle {
        runtime::runtime_handle_by_id(self.runtime_id())
            .unwrap_or_else(|| panic!("runtime {:?} dropped", self.runtime_id()))
    }

    fn runtime_handle_opt(&self) -> Option<RuntimeHandle> {
        runtime::runtime_handle_by_id(self.runtime_id())
    }

    fn with_inner<R>(&self, f: impl FnOnce(&MutableStateInner<T>) -> R) -> R {
        self.runtime_handle()
            .with_state_arena(|arena| arena.with_typed::<T, R>(self.state_id(), f))
    }

    fn try_with_inner<R>(&self, f: impl FnOnce(&MutableStateInner<T>) -> R) -> Option<R> {
        self.runtime_handle_opt()?
            .try_with_state_arena(|arena| arena.with_typed_opt::<T, R>(self.state_id(), f))?
    }

    fn subscribe_current_scope(&self) {
        self.with_inner(register_current_state_scope::<T>);
    }

    pub fn is_alive(&self) -> bool {
        self.try_with_inner(|_| ()).is_some()
    }

    pub fn try_with<R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        self.try_with_inner(|inner| inner.state.try_with_value(f))?
    }

    pub fn try_value(&self) -> Option<T> {
        self.try_with_inner(|inner| inner.state.try_get())?
    }

    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let value = self.with_inner(|inner| inner.state.get());
        self.subscribe_current_scope();
        f(&value)
    }

    pub fn value(&self) -> T {
        let value = self.with_inner(|inner| inner.state.get());
        self.subscribe_current_scope();
        value
    }

    pub fn get(&self) -> T {
        self.value()
    }

    pub fn has_subscribers(&self) -> bool {
        self.with_inner(MutableStateInner::has_subscribers)
    }

    pub fn on_subscriber(&self, callback: Rc<dyn Fn()>) {
        self.with_inner(|inner| {
            inner
                .state
                .subscriber_callback(callback, inner.has_subscribers())
        });
    }
}

impl<T: Clone + 'static> MutableState<T> {
    pub fn with_runtime(value: T, runtime: RuntimeHandle) -> Self {
        runtime.alloc_persistent_state(value)
    }

    fn from_parts(id: StateId, runtime_id: runtime::RuntimeId) -> Self {
        Self {
            id,
            runtime_id,
            _marker: PhantomData,
        }
    }

    pub(crate) fn from_lease(lease: &Rc<runtime::StateHandleLease>) -> Self {
        Self::from_parts(lease.id(), lease.runtime().id())
    }

    fn state_id(&self) -> StateId {
        self.id
    }

    fn runtime_id(&self) -> runtime::RuntimeId {
        self.runtime_id
    }

    fn runtime_handle(&self) -> RuntimeHandle {
        runtime::runtime_handle_by_id(self.runtime_id())
            .unwrap_or_else(|| panic!("runtime {:?} dropped", self.runtime_id()))
    }

    fn runtime_handle_opt(&self) -> Option<RuntimeHandle> {
        runtime::runtime_handle_by_id(self.runtime_id())
    }

    fn with_inner<R>(&self, f: impl FnOnce(&MutableStateInner<T>) -> R) -> R {
        self.runtime_handle()
            .with_state_arena(|arena| arena.with_typed::<T, R>(self.state_id(), f))
    }

    fn try_with_inner<R>(&self, f: impl FnOnce(&MutableStateInner<T>) -> R) -> Option<R> {
        self.runtime_handle_opt()?
            .try_with_state_arena(|arena| arena.with_typed_opt::<T, R>(self.state_id(), f))?
    }

    pub fn is_alive(&self) -> bool {
        self.try_with_inner(|_| ()).is_some()
    }

    pub fn try_with<R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        self.try_with_inner(|inner| inner.state.try_with_value(f))?
    }

    pub fn try_value(&self) -> Option<T> {
        self.try_with_inner(|inner| inner.state.try_get())?
    }

    pub fn as_state(&self) -> State<T> {
        State {
            id: self.id,
            runtime_id: self.runtime_id,
            _marker: PhantomData,
        }
    }

    pub fn try_retain(&self) -> Option<OwnedMutableState<T>> {
        let lease = self
            .runtime_handle_opt()?
            .retain_state_lease(self.state_id())?;
        Some(OwnedMutableState {
            state: *self,
            _lease: lease,
            _marker: PhantomData,
        })
    }

    pub fn retain(&self) -> OwnedMutableState<T> {
        self.try_retain()
            .unwrap_or_else(|| panic!("state {:?} is no longer alive", self.state_id()))
    }

    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let value = self.with_inner(|inner| inner.state.get());
        self.subscribe_current_scope();
        f(&value)
    }

    pub fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let runtime = self.runtime_handle();
        runtime.assert_ui_thread();
        runtime.with_state_arena(|arena| {
            arena.with_typed::<T, R>(self.state_id(), |inner| {
                let mut value = inner.state.get();
                let tracker = UpdateScope::new(inner.state.id());
                let result = f(&mut value);
                let wrote_elsewhere = tracker.finish();
                if !wrote_elsewhere && inner.state.set(value) {
                    inner.invalidate_watchers();
                }
                result
            })
        })
    }

    pub fn replace(&self, value: T) {
        let Some(runtime) = self.runtime_handle_opt() else {
            log::debug!(
                "MutableState::replace skipped: runtime {:?} dropped",
                self.runtime_id()
            );
            return;
        };
        runtime.assert_ui_thread();
        let replaced = runtime
            .try_with_state_arena(|arena| {
                arena.with_typed_opt::<T, ()>(self.state_id(), |inner| {
                    if inner.state.set(value) {
                        inner.invalidate_watchers();
                    }
                })
            })
            .flatten();
        if replaced.is_none() {
            log::debug!(
                "MutableState::replace skipped: state cell released (slot={}, gen={})",
                self.state_id().slot(),
                self.state_id().generation(),
            );
        }
    }

    pub fn set_value(&self, value: T) {
        self.replace(value);
    }

    pub fn set(&self, value: T) {
        self.replace(value);
    }

    pub fn value(&self) -> T {
        let value = self.with_inner(|inner| inner.state.get());
        self.subscribe_current_scope();
        value
    }

    pub fn get(&self) -> T {
        self.value()
    }

    pub fn get_non_reactive(&self) -> T {
        self.with_inner(|inner| inner.state.get())
    }

    #[doc(hidden)]
    pub fn runtime_state_id(&self) -> StateId {
        self.state_id()
    }

    #[doc(hidden)]
    pub fn subscribe_current_scope_only(&self) {
        self.subscribe_current_scope();
    }

    fn subscribe_current_scope(&self) {
        self.with_inner(register_current_state_scope::<T>);
    }

    #[cfg(test)]
    pub(crate) fn watcher_count(&self) -> usize {
        self.with_inner(|inner| inner.watchers.borrow().len())
    }

    #[cfg(test)]
    pub(crate) fn watcher_capacity(&self) -> usize {
        self.with_inner(|inner| inner.watchers.borrow().capacity())
    }

    #[cfg(test)]
    pub(crate) fn subscriber_callback_count(&self) -> usize {
        self.with_inner(|inner| inner.state.subscriber_callback_count())
    }

    #[cfg(test)]
    pub(crate) fn state_id_for_test(&self) -> StateId {
        self.state_id()
    }

    #[cfg(test)]
    pub(crate) fn subscribe_scope_for_test(&self, scope: &RecomposeScope) {
        self.as_state().subscribe_scope_for_test(scope);
    }
}

impl<T: Clone + 'static> OwnedMutableState<T> {
    pub fn with_runtime(value: T, runtime: RuntimeHandle) -> Self {
        let lease = runtime.alloc_state(value);
        Self {
            state: MutableState::from_lease(&lease),
            _lease: lease,
            _marker: PhantomData,
        }
    }

    pub fn with_runtime_structural_eq(value: T, runtime: RuntimeHandle) -> Self
    where
        T: PartialEq,
    {
        Self::with_runtime_and_policy(value, runtime, Arc::new(StructuralEqual))
    }

    pub(crate) fn with_runtime_and_policy(
        value: T,
        runtime: RuntimeHandle,
        policy: Arc<dyn MutationPolicy<T>>,
    ) -> Self {
        let lease = runtime.alloc_state_with_policy(value, policy);
        Self {
            state: MutableState::from_lease(&lease),
            _lease: lease,
            _marker: PhantomData,
        }
    }

    pub fn handle(&self) -> MutableState<T> {
        self.state
    }

    pub fn as_state(&self) -> State<T> {
        self.state.as_state()
    }
}

impl<T: Clone + 'static> Deref for OwnedMutableState<T> {
    type Target = MutableState<T>;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

#[cfg(test)]
impl<T: Clone + 'static> State<T> {
    pub(crate) fn subscribe_scope_for_test(&self, scope: &RecomposeScope) {
        self.with_inner(|inner| {
            let (registered, became_subscribed) = inner.register_scope(scope);
            if registered {
                if let Some(state_id) = inner.state_id() {
                    scope.record_state_subscription(state_id);
                }
                if became_subscribed {
                    inner.state.notify_subscribers();
                }
            }
        });
    }
}

impl<T: fmt::Debug + Clone + 'static> fmt::Debug for MutableState<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(value) = self.try_value() {
            f.debug_struct("MutableState")
                .field("value", &value)
                .finish()
        } else {
            f.write_str("MutableState { value: <unavailable> }")
        }
    }
}

#[derive(Clone)]
pub struct SnapshotStateList<T: Clone + 'static> {
    state: OwnedMutableState<Vec<T>>,
}

impl<T: Clone + 'static> SnapshotStateList<T> {
    pub fn with_runtime<I>(values: I, runtime: RuntimeHandle) -> Self
    where
        I: IntoIterator<Item = T>,
    {
        let initial: Vec<T> = values.into_iter().collect();
        Self {
            state: OwnedMutableState::with_runtime(initial, runtime),
        }
    }

    pub fn as_state(&self) -> State<Vec<T>> {
        self.state.as_state()
    }

    pub fn as_mutable_state(&self) -> MutableState<Vec<T>> {
        self.state.handle()
    }

    pub fn len(&self) -> usize {
        self.state.with(|values| values.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn to_vec(&self) -> Vec<T> {
        self.state.with(|values| values.clone())
    }

    pub fn iter(&self) -> Vec<T> {
        self.to_vec()
    }

    pub fn get(&self, index: usize) -> T {
        self.state.with(|values| values[index].clone())
    }

    pub fn get_opt(&self, index: usize) -> Option<T> {
        self.state.with(|values| values.get(index).cloned())
    }

    pub fn first(&self) -> Option<T> {
        self.get_opt(0)
    }

    pub fn last(&self) -> Option<T> {
        self.state.with(|values| values.last().cloned())
    }

    pub fn push(&self, value: T) {
        self.state.update(|values| values.push(value));
    }

    pub fn extend<I>(&self, iter: I)
    where
        I: IntoIterator<Item = T>,
    {
        self.state.update(|values| values.extend(iter));
    }

    pub fn insert(&self, index: usize, value: T) {
        self.state.update(|values| values.insert(index, value));
    }

    pub fn set(&self, index: usize, value: T) -> T {
        self.state
            .update(|values| std::mem::replace(&mut values[index], value))
    }

    pub fn remove(&self, index: usize) -> T {
        self.state.update(|values| values.remove(index))
    }

    pub fn pop(&self) -> Option<T> {
        self.state.update(|values| values.pop())
    }

    pub fn clear(&self) {
        self.state.replace(Vec::new());
    }

    pub fn retain<F>(&self, mut predicate: F)
    where
        F: FnMut(&T) -> bool,
    {
        self.state
            .update(|values| values.retain(|value| predicate(value)));
    }

    pub fn replace_with<I>(&self, iter: I)
    where
        I: IntoIterator<Item = T>,
    {
        self.state.replace(iter.into_iter().collect());
    }
}

impl<T: fmt::Debug + Clone + 'static> fmt::Debug for SnapshotStateList<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let contents = self.to_vec();
        f.debug_struct("SnapshotStateList")
            .field("values", &contents)
            .finish()
    }
}

#[derive(Clone)]
pub struct SnapshotStateMap<K, V>
where
    K: Clone + Eq + Hash + 'static,
    V: Clone + 'static,
{
    state: OwnedMutableState<HashMap<K, V>>,
}

impl<K, V> SnapshotStateMap<K, V>
where
    K: Clone + Eq + Hash + 'static,
    V: Clone + 'static,
{
    pub fn with_runtime<I>(pairs: I, runtime: RuntimeHandle) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
    {
        let map: HashMap<K, V> = pairs.into_iter().collect();
        Self {
            state: OwnedMutableState::with_runtime(map, runtime),
        }
    }

    pub fn as_state(&self) -> State<HashMap<K, V>> {
        self.state.as_state()
    }

    pub fn as_mutable_state(&self) -> MutableState<HashMap<K, V>> {
        self.state.handle()
    }

    pub fn len(&self) -> usize {
        self.state.with(|map| map.len())
    }

    pub fn is_empty(&self) -> bool {
        self.state.with(|map| map.is_empty())
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.state.with(|map| map.contains_key(key))
    }

    pub fn get(&self, key: &K) -> Option<V> {
        self.state.with(|map| map.get(key).cloned())
    }

    pub fn to_hash_map(&self) -> HashMap<K, V> {
        self.state.with(|map| map.clone())
    }

    pub fn insert(&self, key: K, value: V) -> Option<V> {
        self.state.update(|map| map.insert(key, value))
    }

    pub fn extend<I>(&self, iter: I)
    where
        I: IntoIterator<Item = (K, V)>,
    {
        self.state.update(|map| map.extend(iter));
    }

    pub fn remove(&self, key: &K) -> Option<V> {
        self.state.update(|map| map.remove(key))
    }

    pub fn clear(&self) {
        self.state.replace(HashMap::default());
    }

    pub fn retain<F>(&self, mut predicate: F)
    where
        F: FnMut(&K, &mut V) -> bool,
    {
        self.state.update(|map| map.retain(|k, v| predicate(k, v)));
    }
}

impl<K, V> fmt::Debug for SnapshotStateMap<K, V>
where
    K: Clone + Eq + Hash + fmt::Debug + 'static,
    V: Clone + fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let contents = self.to_hash_map();
        f.debug_struct("SnapshotStateMap")
            .field("entries", &contents)
            .finish()
    }
}

pub(crate) struct DerivedState<T: Clone + 'static> {
    compute: Rc<dyn Fn() -> T>,
    pub(crate) state: OwnedMutableState<T>,
}

impl<T: Clone + 'static> DerivedState<T> {
    pub(crate) fn new(runtime: RuntimeHandle, compute: Rc<dyn Fn() -> T>) -> Self {
        let initial = compute();
        Self {
            compute,
            state: OwnedMutableState::with_runtime(initial, runtime),
        }
    }

    pub(crate) fn set_compute(&mut self, compute: Rc<dyn Fn() -> T>) {
        self.compute = compute;
    }

    pub(crate) fn recompute(&self) {
        let value = (self.compute)();
        self.state.set_value(value);
    }
}

impl<T: fmt::Debug + Clone + 'static> fmt::Debug for State<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(value) = self.try_value() {
            f.debug_struct("State").field("value", &value).finish()
        } else {
            f.write_str("State { value: <unavailable> }")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a chain of records for testing
    fn create_record_chain(ids: &[SnapshotId]) -> Rc<StateRecord> {
        let mut head: Option<Rc<StateRecord>> = None;

        // Build chain in reverse order (last ID becomes the tail)
        for &id in ids.iter().rev() {
            head = Some(StateRecord::new(id, 0i32, head));
        }

        head.expect("create_record_chain called with empty ids")
    }

    struct ManualState {
        head: Rc<StateRecord>,
    }

    impl ManualState {
        fn new(head: Rc<StateRecord>) -> Self {
            Self { head }
        }
    }

    impl StateObject for ManualState {
        fn object_id(&self) -> ObjectId {
            ObjectId(999)
        }

        fn first_record(&self) -> Rc<StateRecord> {
            Rc::clone(&self.head)
        }

        fn try_readable_record(&self, _: SnapshotId, _: &SnapshotIdSet) -> Option<Rc<StateRecord>> {
            Some(Rc::clone(&self.head))
        }

        fn readable_record(&self, _: SnapshotId, _: &SnapshotIdSet) -> Rc<StateRecord> {
            Rc::clone(&self.head)
        }

        fn prepend_state_record(&self, _: Rc<StateRecord>) {}

        fn promote_record(&self, _: SnapshotId) -> Result<(), &'static str> {
            Ok(())
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn poison_mutex<T>(mutex: &Mutex<T>) {
        let poison_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = mutex
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            panic!("poison snapshot state mutex for recovery test");
        }));

        assert!(poison_result.is_err());
    }

    #[test]
    fn observation_leases_drive_subscriber_liveness() {
        let state = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));
        let notifications = Rc::new(Cell::new(0));
        let notifications_for_callback = Rc::clone(&notifications);
        let callback: Rc<dyn Fn()> = Rc::new(move || {
            notifications_for_callback.set(notifications_for_callback.get() + 1);
        });
        state.subscriber_callback(callback.clone(), false);

        let first = StateObject::observation_lease(&*state).expect("first observation lease");
        let second = StateObject::observation_lease(&*state).expect("second observation lease");
        assert!(state.has_subscribers());
        assert_eq!(notifications.get(), 1);

        drop(first);
        assert!(state.has_subscribers());
        drop(second);
        assert!(!state.has_subscribers());

        let third = StateObject::observation_lease(&*state).expect("third observation lease");
        assert_eq!(notifications.get(), 2);
        drop(third);
    }

    #[test]
    fn snapshot_mutable_state_recovers_poisoned_weak_self_lock() {
        let state = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));

        poison_mutex(&state.weak_self);

        assert_eq!(state.get(), 100);
        assert!(state.set(101));
        assert_eq!(state.get(), 101);
    }

    #[test]
    fn snapshot_mutable_state_recovers_poisoned_apply_observer_lock() {
        let state = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));
        let calls = Rc::new(Cell::new(0usize));
        let observed_calls = Rc::clone(&calls);

        poison_mutex(&state.apply_observers);

        state.add_apply_observer(Box::new(move || {
            observed_calls.set(observed_calls.get() + 1);
        }));
        state.notify_applied();

        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn snapshot_mutable_state_promote_missing_record_returns_error() {
        let state = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));
        let missing_snapshot = usize::MAX - 17;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            StateObject::promote_record(&*state, missing_snapshot)
        }));

        assert!(
            matches!(result, Ok(Err("missing child record"))),
            "missing child record should be reported through Result, got {result:?}"
        );
    }

    #[test]
    fn snapshot_mutable_state_promote_wrong_record_type_returns_error() {
        let state = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));
        let child_snapshot = usize::MAX - 31;
        let wrong_record = StateRecord::new(child_snapshot, "wrong type", None);
        StateObject::prepend_state_record(&*state, wrong_record);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            StateObject::promote_record(&*state, child_snapshot)
        }));

        assert!(
            matches!(result, Ok(Err("child record value missing or wrong type"))),
            "wrong child record type should be reported through Result, got {result:?}"
        );
    }

    #[test]
    fn snapshot_mutable_state_commit_wrong_record_type_returns_error() {
        let state = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));
        let merged = StateRecord::new(usize::MAX - 43, "wrong type", None);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            StateObject::commit_merged_record(&*state, merged)
        }));

        assert!(
            matches!(result, Ok(Err("merged record value missing or wrong type"))),
            "wrong merged record type should be reported through Result, got {result:?}"
        );
    }

    #[test]
    fn snapshot_mutable_state_merge_wrong_record_type_returns_none() {
        let state = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));
        let previous = StateRecord::new(usize::MAX - 51, 1i32, None);
        let current = StateRecord::new(usize::MAX - 52, "wrong type", None);
        let applied = StateRecord::new(usize::MAX - 53, 2i32, None);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            StateObject::merge_records(&*state, previous, current, applied)
        }));

        match result {
            Ok(None) => {}
            Ok(Some(_)) => panic!("wrong merge record type unexpectedly produced a merged record"),
            Err(_) => panic!("wrong merge record type should not panic"),
        }
    }

    #[test]
    fn test_used_locked_finds_invalid_snapshot() {
        // Create a chain with an INVALID_SNAPSHOT record
        let tail = StateRecord::new(PREEXISTING_SNAPSHOT_ID, 0i32, None);
        let invalid_rec = StateRecord::new(INVALID_SNAPSHOT_ID, 0i32, Some(tail));
        let head = StateRecord::new(10, 0i32, Some(invalid_rec.clone()));

        let result = used_locked(&head);
        assert!(result.is_some());
        assert_eq!(result.unwrap().snapshot_id(), INVALID_SNAPSHOT_ID);
    }

    #[test]
    fn test_used_locked_finds_obscured_record() {
        // Reset pinning state for clean test
        crate::snapshot_pinning::reset_pinning_table();

        // Pin a high snapshot to set a known reuse limit
        // This ensures records 2 and 5 are both below (reuse_limit = 10 - 1 = 9)
        let pin_handle = crate::snapshot_pinning::track_pinning(10, &SnapshotIdSet::EMPTY);

        // Create a chain with two old records below the reuse limit
        let oldest = StateRecord::new(2, 0i32, None);
        let newer = StateRecord::new(5, 0i32, Some(oldest.clone()));
        let head = StateRecord::new(100, 0i32, Some(newer));

        let result = used_locked(&head);

        // Should find the older of the two records below reuse limit
        assert!(result.is_some());
        let reused = result.unwrap();
        assert_eq!(
            reused.snapshot_id(),
            2,
            "Should return the oldest obscured record"
        );

        // Clean up
        crate::snapshot_pinning::release_pinning(pin_handle);
    }

    #[test]
    fn test_used_locked_no_reusable_record() {
        // Reset pinning state
        crate::snapshot_pinning::reset_pinning_table();

        // Create a chain where all records are recent (above reuse limit)
        // Use very high IDs to ensure they're above any reuse limit
        let high_id = allocate_record_id() + 1000;
        let head = create_record_chain(&[high_id, high_id + 1, high_id + 2]);

        let result = used_locked(&head);
        assert!(
            result.is_none(),
            "Should find no reusable records when all are recent"
        );
    }

    #[test]
    fn test_used_locked_single_old_record() {
        // Reset pinning state
        crate::snapshot_pinning::reset_pinning_table();

        // Create a chain with only one old record (should not be reused)
        let old = StateRecord::new(2, 0i32, None);
        let head = StateRecord::new(100, 0i32, Some(old));

        let result = used_locked(&head);
        // With only ONE record below reuse limit, it's still valid and should not be reused
        assert!(result.is_none(), "Single old record should not be reused");
    }

    #[test]
    fn test_readable_record_for_preexisting() {
        let head = create_record_chain(&[PREEXISTING_SNAPSHOT_ID]);
        let invalid = SnapshotIdSet::EMPTY;

        let result = readable_record_for(&head, 10, &invalid);
        assert!(result.is_some());
        assert_eq!(result.unwrap().snapshot_id(), PREEXISTING_SNAPSHOT_ID);
    }

    #[test]
    fn test_readable_record_for_picks_highest_valid() {
        let head = create_record_chain(&[10, 5, PREEXISTING_SNAPSHOT_ID]);
        let invalid = SnapshotIdSet::EMPTY;

        // Reading at snapshot 10 should return record 10
        let result = readable_record_for(&head, 10, &invalid);
        assert!(result.is_some());
        assert_eq!(result.unwrap().snapshot_id(), 10);

        // Reading at snapshot 7 should skip record 10 and return record 5
        let result = readable_record_for(&head, 7, &invalid);
        assert!(result.is_some());
        assert_eq!(result.unwrap().snapshot_id(), 5);
    }

    #[test]
    fn test_new_overwritable_record_locked_reuses_invalid() {
        // Create a state with an INVALID record in the chain
        let state = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));

        // Manually insert an INVALID record into the chain
        let current_head = state.first_record();
        let invalid_rec = StateRecord::new(INVALID_SNAPSHOT_ID, 0i32, current_head.next());
        current_head.set_next(Some(invalid_rec.clone()));

        let result = new_overwritable_record_locked(&*state);

        // Should reuse the INVALID record
        assert!(Rc::ptr_eq(&result, &invalid_rec));
        assert_eq!(result.snapshot_id(), SNAPSHOT_ID_MAX);
    }

    #[test]
    fn test_new_overwritable_record_locked_creates_new() {
        crate::snapshot_pinning::reset_pinning_table();

        // Pin snapshot 1 to prevent PREEXISTING (id=1) from being reusable
        // This ensures the reuse limit is above 1, so PREEXISTING won't be obscured
        let _pin_handle = crate::snapshot_pinning::track_pinning(1, &SnapshotIdSet::EMPTY);

        // Create a state with all recent records (no reusable ones)
        let state = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));
        let old_head = state.first_record();

        let result = new_overwritable_record_locked(&*state);

        // Should create a new record
        assert_eq!(result.snapshot_id(), SNAPSHOT_ID_MAX);

        // Should be prepended to the chain (becomes new head)
        let new_head = state.first_record();
        assert!(
            Rc::ptr_eq(&new_head, &result),
            "new_head ({:p}) should equal result ({:p})",
            Rc::as_ptr(&new_head),
            Rc::as_ptr(&result)
        );

        // The new record should point to the old head
        assert!(result.next().is_some());
        assert!(Rc::ptr_eq(&result.next().unwrap(), &old_head));
    }

    #[test]
    fn test_writable_record_reuses_invalid_record() {
        crate::snapshot_pinning::reset_pinning_table();

        let state = SnapshotMutableState::new_in_arc(7i32, Arc::new(NeverEqual));

        // Inject an INVALID record that should be reused on next write.
        let head = state.first_record();
        let invalid = StateRecord::new(INVALID_SNAPSHOT_ID, 0i32, head.next());
        head.set_next(Some(invalid.clone()));

        let snapshot_id = allocate_record_id();
        let result = state.writable_record(snapshot_id, &SnapshotIdSet::EMPTY);

        assert!(
            Rc::ptr_eq(&result, &invalid),
            "Expected writable_record to reuse the INVALID record"
        );
        assert_eq!(result.snapshot_id(), snapshot_id);
        result.with_value(|value: &i32| {
            assert_eq!(*value, 7, "Reused record should copy the readable value");
        });
        assert!(!result.is_tombstone());
    }

    #[test]
    fn test_writable_record_creates_new_when_reuse_disallowed() {
        crate::snapshot_pinning::reset_pinning_table();
        let pin = crate::snapshot_pinning::track_pinning(1, &SnapshotIdSet::EMPTY);

        let state = SnapshotMutableState::new_in_arc(42i32, Arc::new(NeverEqual));
        let original_head = state.first_record();
        let preexisting = original_head
            .next()
            .expect("preexisting record should exist for newly created state");

        let snapshot_id = allocate_record_id();
        let result = state.writable_record(snapshot_id, &SnapshotIdSet::EMPTY);

        assert!(
            !Rc::ptr_eq(&result, &original_head),
            "Should not reuse the current head when reuse is disallowed"
        );
        assert!(
            !Rc::ptr_eq(&result, &preexisting),
            "Should not reuse the PREEXISTING record"
        );
        assert_eq!(result.snapshot_id(), snapshot_id);
        result.with_value(|value: &i32| assert_eq!(*value, 42));

        let new_head = state.first_record();
        assert!(
            Rc::ptr_eq(&new_head, &result),
            "Newly created record should become the head of the chain"
        );

        crate::snapshot_pinning::release_pinning(pin);
    }

    #[test]
    fn test_state_record_clear_for_reuse() {
        let record = StateRecord::new(10, 42i32, None);

        // Verify value exists before clearing
        record.with_value(|val: &i32| {
            assert_eq!(*val, 42);
        });

        // Clear the record for reuse
        record.clear_for_reuse();

        // Value should be cleared (will panic if we try to access it)
        // Just verify snapshot_id is unchanged
        assert_eq!(record.snapshot_id(), 10);
    }

    #[test]
    fn test_overwrite_unused_records_no_old_records() {
        crate::snapshot_pinning::reset_pinning_table();

        // Create state first to establish snapshot IDs
        let state = SnapshotMutableState::new_in_arc(42i32, Arc::new(NeverEqual));

        // Pin snapshot 1 so reuse limit is 1, making both initial records (1 and 2) above it
        // This ensures PREEXISTING won't be overwritten
        let _pin = crate::snapshot_pinning::track_pinning(1, &SnapshotIdSet::EMPTY);

        let should_retain = state.overwrite_unused_records();

        // With both records above/at reuse limit, we have 2 retained
        assert!(
            should_retain,
            "Should retain multiple records when none are old enough"
        );

        // No records should be marked as INVALID
        let mut cursor = Some(state.first_record());
        while let Some(record) = cursor {
            assert_ne!(record.snapshot_id(), INVALID_SNAPSHOT_ID);
            cursor = record.next();
        }
    }

    #[test]
    fn test_overwrite_unused_records_basic_cleanup() {
        // Test that old records get marked invalid when newer ones exist
        crate::snapshot_pinning::reset_pinning_table();

        // Create simple manual chain to avoid snapshot ID allocation complexity
        let rec1 = StateRecord::new(100, 1i32, None);
        let rec2 = StateRecord::new(200, 2i32, Some(rec1.clone()));
        let rec3 = StateRecord::new(300, 3i32, Some(rec2.clone()));

        // Mock state object for testing
        struct TestState {
            head: Rc<StateRecord>,
        }
        impl StateObject for TestState {
            fn object_id(&self) -> ObjectId {
                ObjectId(999)
            }
            fn first_record(&self) -> Rc<StateRecord> {
                Rc::clone(&self.head)
            }
            fn try_readable_record(
                &self,
                _: SnapshotId,
                _: &SnapshotIdSet,
            ) -> Option<Rc<StateRecord>> {
                Some(Rc::clone(&self.head))
            }
            fn readable_record(&self, _: SnapshotId, _: &SnapshotIdSet) -> Rc<StateRecord> {
                Rc::clone(&self.head)
            }
            fn prepend_state_record(&self, _: Rc<StateRecord>) {}
            fn promote_record(&self, _: SnapshotId) -> Result<(), &'static str> {
                Ok(())
            }
            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        let test_state = TestState { head: rec3.clone() };

        // Pin at 1000 so all three records (100, 200, 300) are below reuse limit
        let _pin = crate::snapshot_pinning::track_pinning(1000, &SnapshotIdSet::EMPTY);

        let result = overwrite_unused_records_locked::<i32>(&test_state);

        // Should keep highest (300), mark others invalid
        assert_eq!(rec3.snapshot_id(), 300);
        assert_eq!(rec2.snapshot_id(), INVALID_SNAPSHOT_ID);
        assert_eq!(rec1.snapshot_id(), INVALID_SNAPSHOT_ID);

        // Only one record retained (300), so should return false
        assert!(!result);
    }

    #[test]
    fn test_overwrite_unused_records_single_record_only() {
        crate::snapshot_pinning::reset_pinning_table();

        let state = SnapshotMutableState::new_in_arc(42i32, Arc::new(NeverEqual));

        // Remove the PREEXISTING record by setting next to None
        let head = state.first_record();
        head.set_next(None);

        let should_retain = state.overwrite_unused_records();

        // With only one record, should return false
        assert!(!should_retain, "Single record should return false");
    }

    #[test]
    fn snapshot_state_try_get_reports_missing_visible_record_without_panicking() {
        crate::snapshot_pinning::reset_pinning_table();

        let state = SnapshotMutableState::new_in_arc(42i32, Arc::new(NeverEqual));
        let head = state.first_record();
        head.set_snapshot_id(SNAPSHOT_ID_MAX);
        head.set_next(None);

        assert_eq!(state.try_get(), None);
    }

    #[test]
    fn test_overwrite_unused_records_clears_values() {
        crate::snapshot_pinning::reset_pinning_table();

        let tail = StateRecord::new(PREEXISTING_SNAPSHOT_ID, 0i32, None);
        let old_rec1 = StateRecord::new(2, 999i32, Some(tail.clone()));
        let old_rec2 = StateRecord::new(3, 888i32, Some(old_rec1.clone()));
        let head = StateRecord::new(150, 42i32, Some(old_rec2.clone()));
        let state = ManualState::new(head.clone());

        // Verify value exists before cleanup
        old_rec1.with_value(|val: &i32| {
            assert_eq!(*val, 999);
        });

        let _pin = crate::snapshot_pinning::track_pinning(100, &SnapshotIdSet::EMPTY);
        overwrite_unused_records_locked::<i32>(&state);

        // The invalidated record should have its value cleared
        assert_eq!(old_rec1.snapshot_id(), INVALID_SNAPSHOT_ID);
        // Value access would panic, so we just verify it was marked invalid
    }

    #[test]
    fn test_overwrite_unused_records_mixed_old_and_new() {
        crate::snapshot_pinning::reset_pinning_table();

        // Create mixed chain: recent (50) -> old (5) -> old (2) -> PREEXISTING
        let preexisting = StateRecord::new(PREEXISTING_SNAPSHOT_ID, 0i32, None);
        let rec2 = StateRecord::new(2, 100i32, Some(preexisting.clone()));
        let rec5 = StateRecord::new(5, 100i32, Some(rec2.clone()));
        let rec50 = StateRecord::new(50, 100i32, Some(rec5.clone()));
        let head = StateRecord::new(120, 100i32, Some(rec50.clone()));
        let state = ManualState::new(head.clone());

        // Pin snapshot 40 so reuse limit is ~40, making 2 and 5 old but 50 recent
        let _pin = crate::snapshot_pinning::track_pinning(40, &SnapshotIdSet::EMPTY);

        let should_retain = overwrite_unused_records_locked::<i32>(&state);
        assert!(should_retain);

        // rec50 is above reuse limit - should stay valid
        assert_eq!(rec50.snapshot_id(), 50);
        // rec5 is highest below reuse limit - should stay valid
        assert_eq!(rec5.snapshot_id(), 5);
        // rec2 is older and below reuse limit - should be invalidated
        assert_eq!(rec2.snapshot_id(), INVALID_SNAPSHOT_ID);
    }

    #[test]
    fn test_readable_record_for_skips_invalid_set() {
        let head = create_record_chain(&[10, 5, PREEXISTING_SNAPSHOT_ID]);
        let invalid = SnapshotIdSet::new().set(5);

        // Reading at snapshot 10 should skip record 5 (in invalid set)
        let result = readable_record_for(&head, 10, &invalid);
        assert!(result.is_some());
        assert_eq!(result.unwrap().snapshot_id(), 10);

        // Reading at snapshot 7 should skip 5 and fall back to PREEXISTING
        let result = readable_record_for(&head, 7, &invalid);
        assert!(result.is_some());
        assert_eq!(result.unwrap().snapshot_id(), PREEXISTING_SNAPSHOT_ID);
    }

    // ========== Tests for assign_value() ==========

    #[test]
    fn test_assign_value_copies_int() {
        let source = StateRecord::new(10, 42i32, None);
        let target = StateRecord::new(20, 0i32, None);

        target.assign_value::<i32>(&source).expect("copy int value");

        // Verify the value was copied
        target.with_value(|val: &i32| {
            assert_eq!(*val, 42);
        });

        // Verify source is unchanged
        source.with_value(|val: &i32| {
            assert_eq!(*val, 42);
        });

        // Verify snapshot IDs are unchanged
        assert_eq!(source.snapshot_id(), 10);
        assert_eq!(target.snapshot_id(), 20);
    }

    #[test]
    fn test_assign_value_copies_string() {
        let source = StateRecord::new(10, "hello".to_string(), None);
        let target = StateRecord::new(20, "world".to_string(), None);

        target
            .assign_value::<String>(&source)
            .expect("copy string value");

        // Verify the value was copied
        target.with_value(|val: &String| {
            assert_eq!(val, "hello");
        });

        // Verify source is unchanged
        source.with_value(|val: &String| {
            assert_eq!(val, "hello");
        });
    }

    #[test]
    fn test_assign_value_reports_cleared_source() {
        let source = StateRecord::new(10, 42i32, None);
        let target = StateRecord::new(20, 0i32, None);

        source.clear_value();

        assert_eq!(
            target.assign_value::<i32>(&source),
            Err(StateRecordValueError::MissingOrWrongType {
                expected: std::any::type_name::<i32>(),
            })
        );
        assert_eq!(target.with_value(|val: &i32| *val), 0);
    }

    #[test]
    fn test_assign_value_overwrites_existing_value() {
        let source = StateRecord::new(10, 100i32, None);
        let target = StateRecord::new(20, 999i32, None);

        // Verify target has initial value
        target.with_value(|val: &i32| {
            assert_eq!(*val, 999);
        });

        // Assign from source
        target
            .assign_value::<i32>(&source)
            .expect("overwrite int value");

        // Verify target now has source's value
        target.with_value(|val: &i32| {
            assert_eq!(*val, 100);
        });
    }

    #[test]
    fn test_assign_value_with_custom_type() {
        #[derive(Clone, PartialEq, Debug)]
        struct Point {
            x: f64,
            y: f64,
        }

        let source = StateRecord::new(10, Point { x: 1.5, y: 2.5 }, None);
        let target = StateRecord::new(20, Point { x: 0.0, y: 0.0 }, None);

        target
            .assign_value::<Point>(&source)
            .expect("copy point value");

        target.with_value(|val: &Point| {
            assert_eq!(val, &Point { x: 1.5, y: 2.5 });
        });
    }

    #[test]
    fn test_assign_value_self_assignment() {
        let record = StateRecord::new(10, 42i32, None);

        // Self-assignment should work (though not useful in practice)
        record
            .assign_value::<i32>(&record)
            .expect("self-assign int value");

        record.with_value(|val: &i32| {
            assert_eq!(*val, 42);
        });
    }

    /// The user-visible leak behind "fps got lower and lower" during
    /// repeated text-handle drags: every pointer event writes drag state
    /// through `run_in_mutable_snapshot` (the `dispatch_ui_event` path).
    /// Each write must leave the record chain bounded — a growing chain
    /// makes every subsequent state read a longer linear scan, degrading
    /// every later frame.
    #[test]
    fn event_loop_writes_keep_the_record_chain_bounded() {
        crate::snapshot_pinning::reset_pinning_table();
        let state = SnapshotMutableState::new_in_arc(0.0f32, Arc::new(NeverEqual));

        let mut lens = Vec::new();
        for event in 0..3000usize {
            crate::run_in_mutable_snapshot(|| {
                state.set(event as f32);
            })
            .expect("event snapshot applies");
            // The renderer reads the value from the global snapshot between
            // events (draw + layout consume the state each frame).
            let _ = state.get();
            if event % 500 == 499 {
                lens.push(state.record_chain_debug().len());
            }
        }

        let final_len = *lens.last().expect("sampled chain lengths");
        assert!(
            final_len <= 16,
            "record chain grew without bound across event-loop writes: {lens:?}"
        );
    }

    #[test]
    fn test_assign_value_with_vec() {
        let source = StateRecord::new(10, vec![1, 2, 3, 4, 5], None);
        let target = StateRecord::new(20, Vec::<i32>::new(), None);

        target
            .assign_value::<Vec<i32>>(&source)
            .expect("copy vec value");

        target.with_value(|val: &Vec<i32>| {
            assert_eq!(val, &vec![1, 2, 3, 4, 5]);
        });

        // Verify it's a deep copy (modifying source won't affect target)
        source.replace_value(vec![10, 20]);
        target.with_value(|val: &Vec<i32>| {
            assert_eq!(val, &vec![1, 2, 3, 4, 5]);
        });
    }
}
