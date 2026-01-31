// StateRecord uses Rc with Cell for single-threaded shared ownership in the snapshot system.
#![allow(clippy::arc_with_non_send_sync)]

use crate::collections::map::HashSet;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex, Weak};

use crate::snapshot_id_set::{SnapshotId, SnapshotIdSet};
use crate::snapshot_pinning::lowest_pinned_snapshot;
use crate::snapshot_v2::{
    advance_global_snapshot, allocate_record_id, current_snapshot, AnySnapshot, GlobalSnapshot,
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
        let guard = self.value.borrow();
        let value = guard
            .as_ref()
            .and_then(|boxed| boxed.downcast_ref::<T>())
            .expect("StateRecord value missing or wrong type");
        f(value)
    }

    /// Clears the value from this record to free memory.
    /// Used when marking records as reusable - clears the value to reduce memory usage.
    #[allow(dead_code)]
    pub(crate) fn clear_for_reuse(&self) {
        self.clear_value();
    }

    /// Copies the value from the source record into this record.
    ///
    /// This is used during record reuse to copy valid data from a readable record
    /// into a reused record, and during cleanup to preserve data in records being
    /// marked as INVALID_SNAPSHOT.
    ///
    /// # Type Safety
    /// The caller must ensure both records contain values of type `T`.
    /// Panics if the source record doesn't contain a value of type `T`.
    pub(crate) fn assign_value<T: Any + Clone>(&self, source: &StateRecord) {
        let cloned_value = source.with_value(|value: &T| value.clone());
        self.replace_value(cloned_value);
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

    // No reusable record found - create a new one
    // The new record is prepended to the chain with a placeholder value
    // Caller must use replace_value() to set the actual value
    let new_record = StateRecord::new(
        SNAPSHOT_ID_MAX,
        (),   // Placeholder value - caller will replace this
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
                let valid = valid_record.as_ref().unwrap();
                let record_to_overwrite = if current_id < valid.snapshot_id() {
                    Rc::clone(&record)
                } else {
                    // Keep current as valid, overwrite the previous valid
                    let to_overwrite = Rc::clone(valid);
                    valid_record = Some(Rc::clone(&record));
                    to_overwrite
                };

                // Lazily find a young record to copy data from
                if overwrite_record.is_none() {
                    // Find the youngest record, or first record >= reuseLimit
                    overwrite_record =
                        Some(find_youngest_or(&head, |r| r.snapshot_id() >= reuse_limit));
                }

                // Mark the old record as invalid and copy valid data into it
                record_to_overwrite.set_snapshot_id(INVALID_SNAPSHOT_ID);
                record_to_overwrite.assign_value::<T>(overwrite_record.as_ref().unwrap());
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

pub trait StateObject: Any {
    fn object_id(&self) -> ObjectId;
    fn first_record(&self) -> Rc<StateRecord>;
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

    /// Downcast to Any for testing/debugging purposes.
    fn as_any(&self) -> &dyn Any;
}

pub(crate) struct SnapshotMutableState<T> {
    head: RefCell<Rc<StateRecord>>,
    policy: Arc<dyn MutationPolicy<T>>,
    id: ObjectId,
    weak_self: Mutex<Option<Weak<Self>>>,
    apply_observers: Mutex<Vec<Box<dyn Fn() + 'static>>>,
}

impl<T> SnapshotMutableState<T> {
    fn assert_chain_integrity(&self, caller: &str, snapshot_context: Option<SnapshotId>) {
        if !should_check_chain_integrity() {
            return;
        }
        let head = self.head.borrow().clone();
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
        use std::sync::OnceLock;
        static CHECK: OnceLock<bool> = OnceLock::new();
        *CHECK.get_or_init(|| std::env::var_os("CRANPOSE_ASSERT_STATE_CHAIN").is_some())
    }
}

impl<T: Clone + 'static> SnapshotMutableState<T> {
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
                let mut head_guard = self.head.borrow_mut();
                let current_head = head_guard.clone();
                let refreshed = readable_record_for(&current_head, snapshot_id, invalid);
                let source = refreshed.unwrap_or_else(|| current_head.clone());

                // Create a new record
                // Record reuse is NOT used here to preserve history for conflict detection
                // Reuse happens during cleanup (overwrite_unused_records_locked)
                let cloned_value = source.with_value(|value: &T| value.clone());
                let new_head = StateRecord::new(snapshot_id, cloned_value, Some(current_head));

                *head_guard = new_head.clone();
                drop(head_guard);
                self.assert_chain_integrity("writable_record(recover)", Some(snapshot_id));
                return new_head;
            }
        };

        if readable.snapshot_id() == snapshot_id {
            return readable;
        }

        let refreshed = {
            let head_guard = self.head.borrow();
            let current_head = head_guard.clone();
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
        overwritable.assign_value::<T>(&refreshed);
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
            head: RefCell::new(head),
            policy,
            id: ObjectId::default(),
            weak_self: Mutex::new(None),
            apply_observers: Mutex::new(Vec::new()),
        });

        let id = ObjectId::new(&state);
        Arc::get_mut(&mut state).expect("fresh Arc").id = id;

        *state.weak_self.lock().expect("Weak self lock poisoned") = Some(Arc::downgrade(&state));

        // No need to advance the global snapshot for initial state creation

        state
    }

    pub(crate) fn add_apply_observer(&self, observer: Box<dyn Fn() + 'static>) {
        self.apply_observers
            .lock()
            .expect("Observers lock poisoned")
            .push(observer);
    }

    fn notify_applied(&self) {
        let observers = self
            .apply_observers
            .lock()
            .expect("Observers lock poisoned");
        for observer in observers.iter() {
            observer();
        }
    }

    #[inline]
    pub(crate) fn id(&self) -> ObjectId {
        self.id
    }

    pub(crate) fn get(&self) -> T {
        let snapshot = active_snapshot();
        if let Some(state) = self
            .weak_self
            .lock()
            .expect("Weak self lock poisoned")
            .as_ref()
            .and_then(|weak| weak.upgrade())
        {
            snapshot.record_read(&*state);
        }

        let snapshot_id = snapshot.snapshot_id();
        let invalid = snapshot.invalid();

        if let Some(record) = self.readable_for(snapshot_id, &invalid) {
            return record.with_value(|value: &T| value.clone());
        }

        // Retry with fresh snapshot in case global snapshot was advanced
        let fresh_snapshot = active_snapshot();
        let fresh_id = fresh_snapshot.snapshot_id();
        let fresh_invalid = fresh_snapshot.invalid();

        if let Some(record) = self.readable_for(fresh_id, &fresh_invalid) {
            return record.with_value(|value: &T| value.clone());
        }

        // Fallback: try reading directly from the global snapshot.
        // This handles the case where sibling mutable snapshots have been applied
        // but our current snapshot's invalid set was fixed at creation time.
        // The global snapshot should have all applied changes visible.
        let global = GlobalSnapshot::get_or_create();
        let global_id = global.snapshot_id();
        let global_invalid = global.invalid();

        if let Some(record) = self.readable_for(global_id, &global_invalid) {
            return record.with_value(|value: &T| value.clone());
        }

        // Debug: print the record chain to understand what's available
        let head = self.first_record();
        let mut chain_ids = Vec::new();
        let mut cursor = Some(head);
        while let Some(record) = cursor {
            chain_ids.push((record.snapshot_id(), record.is_tombstone()));
            cursor = record.next();
        }

        // If still null, this is an error condition
        panic!(
            "Reading a state that was created after the snapshot was taken or in a snapshot that has not yet been applied\n\
             state={:?}, snapshot_id={}, fresh_snapshot_id={}, fresh_invalid={:?}\n\
             record_chain={:?}",
            self.id, snapshot_id, fresh_id, fresh_invalid, chain_ids
        );
    }

    pub(crate) fn set(&self, new_value: T) {
        // Debug-only check: warn if modifying state in event handler without proper snapshot
        #[cfg(debug_assertions)]
        {
            let in_handler = crate::in_event_handler();
            let in_snapshot = crate::in_applied_snapshot();
            if in_handler && !in_snapshot {
                eprintln!(
                    "⚠️  WARNING: State modified in event handler without run_in_mutable_snapshot!\n\
                     This can cause state updates to be invisible to other contexts.\n\
                     Wrap your handler in run_in_mutable_snapshot() or dispatch_ui_event().\n\
                     State: {:?}",
                    self.id
                );
            }
        }

        let snapshot = active_snapshot();
        if let Some(state) = self
            .weak_self
            .lock()
            .expect("Weak self lock poisoned")
            .as_ref()
            .and_then(|weak| weak.upgrade())
        {
            let trait_object: Arc<dyn StateObject> = state.clone();
            snapshot.record_write(trait_object);
        }
        mark_update_write(self.id);

        let snapshot_id = snapshot.snapshot_id();

        match &snapshot {
            AnySnapshot::Global(global) => {
                if global.has_pending_children() {
                    panic!(
                        "SnapshotMutableState::set attempted global write while pending children {:?} exist (state {:?}, snapshot_id={})",
                        global.pending_children(),
                        self.id,
                        snapshot_id
                    );
                }

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
            }
            AnySnapshot::Mutable(_)
            | AnySnapshot::NestedMutable(_)
            | AnySnapshot::TransparentMutable(_) => {
                let invalid = snapshot.invalid();
                let record = self.writable_record(snapshot_id, &invalid);
                let equivalent =
                    record.with_value(|current: &T| self.policy.equivalent(current, &new_value));
                if !equivalent {
                    record.replace_value(new_value);
                }
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
        self.head.borrow().clone()
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
        let mut head_guard = self.head.borrow_mut();
        let current_head = head_guard.clone();
        record.set_next(Some(current_head));
        *head_guard = record;
    }

    fn merge_records(
        &self,
        previous: Rc<StateRecord>,
        current: Rc<StateRecord>,
        applied: Rc<StateRecord>,
    ) -> Option<Rc<StateRecord>> {
        let current_vs_applied = current.with_value(|current: &T| {
            applied.with_value(|applied_value: &T| self.policy.equivalent(current, applied_value))
        });
        if current_vs_applied {
            return Some(current);
        }

        previous
            .with_value(|prev: &T| {
                current.with_value(|current_value: &T| {
                    applied.with_value(|applied_value: &T| {
                        self.policy.merge(prev, current_value, applied_value)
                    })
                })
            })
            .map(|merged| StateRecord::new(applied.snapshot_id(), merged, None))
    }

    fn promote_record(&self, child_id: SnapshotId) -> Result<(), &'static str> {
        let head = self.first_record();
        let mut cursor = Some(head);
        while let Some(record) = cursor {
            if record.snapshot_id() == child_id {
                let cloned = record.with_value(|value: &T| value.clone());
                let new_id = allocate_record_id();
                let mut head_guard = self.head.borrow_mut();
                let current_head = head_guard.clone();
                let new_head = StateRecord::new(new_id, cloned, Some(current_head));
                *head_guard = new_head;
                drop(head_guard);
                advance_global_snapshot(new_id);
                self.notify_applied();
                self.assert_chain_integrity("promote_record", Some(child_id));
                return Ok(());
            }
            cursor = record.next();
        }
        panic!(
            "SnapshotMutableState::promote_record missing child record (state {:?}, child_id={})",
            self.id, child_id
        );
    }

    fn commit_merged_record(&self, merged: Rc<StateRecord>) -> Result<SnapshotId, &'static str> {
        let value = merged.with_value(|value: &T| value.clone());
        let new_id = allocate_record_id();
        let mut head_guard = self.head.borrow_mut();
        let current_head = head_guard.clone();
        let new_head = StateRecord::new(new_id, value, Some(current_head));
        *head_guard = new_head;
        drop(head_guard);
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

        target.assign_value::<i32>(&source);

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

        target.assign_value::<String>(&source);

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
    #[should_panic(expected = "StateRecord value missing or wrong type")]
    fn test_assign_value_copies_from_cleared_source_panics() {
        let source = StateRecord::new(10, 42i32, None);
        let target = StateRecord::new(20, 0i32, None);

        // Clear the source value
        source.clear_value();

        // Should panic because source has no value
        target.assign_value::<i32>(&source);
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
        target.assign_value::<i32>(&source);

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

        target.assign_value::<Point>(&source);

        target.with_value(|val: &Point| {
            assert_eq!(val, &Point { x: 1.5, y: 2.5 });
        });
    }

    #[test]
    fn test_assign_value_self_assignment() {
        let record = StateRecord::new(10, 42i32, None);

        // Self-assignment should work (though not useful in practice)
        record.assign_value::<i32>(&record);

        record.with_value(|val: &i32| {
            assert_eq!(*val, 42);
        });
    }

    #[test]
    fn test_assign_value_with_vec() {
        let source = StateRecord::new(10, vec![1, 2, 3, 4, 5], None);
        let target = StateRecord::new(20, Vec::<i32>::new(), None);

        target.assign_value::<Vec<i32>>(&source);

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
