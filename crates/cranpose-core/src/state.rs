use std::{
    any::{Any, TypeId},
    cell::{Cell, OnceCell, RefCell},
    fmt,
    hash::Hash,
    marker::PhantomData,
    ops::Deref,
    rc::{Rc, Weak},
};

use smallvec::{SmallVec, smallvec};

use crate::{
    RecomposeScope, RecomposeScopeInner, RuntimeHandle, ScopeId, StateId,
    collections::map::HashMap,
    debug_trace::debug_record_scope_invalidation,
    runtime,
    snapshot_id_set::{SnapshotId, SnapshotIdSet},
    snapshot_v2::{
        AnySnapshot, GlobalSnapshot, advance_global_snapshot, allocate_record_id, current_snapshot,
    },
    with_current_composer_opt,
};

mod derived_state;
mod record_chain;
mod snapshot_state_list;

pub(crate) use derived_state::DerivedState;
pub(crate) use record_chain::RecordChain;
pub use snapshot_state_list::SnapshotStateList;

pub(crate) const PREEXISTING_SNAPSHOT_ID: SnapshotId = SnapshotId(1);

const INVALID_SNAPSHOT_ID: SnapshotId = SnapshotId(0);

const SNAPSHOT_ID_MAX: SnapshotId = SnapshotId(usize::MAX);

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug, Default)]
pub struct ObjectId(pub(crate) usize);

impl ObjectId {
    #[cfg(test)]
    pub(crate) fn new<T: ?Sized + 'static>(object: &Rc<T>) -> Self {
        Self(Rc::as_ptr(object) as *const () as usize)
    }
}

pub struct StateRecord {
    snapshot_id: Cell<SnapshotId>,
    tombstone: Cell<bool>,
    value: RefCell<Option<Box<dyn RecordValue>>>,
}

pub(crate) trait RecordValue: Any {
    fn clone_boxed(&self) -> Box<dyn RecordValue>;
    fn assign_into(&self, target: &mut Option<Box<dyn RecordValue>>);
    /// Whether dropping the value releases anything beyond its box.
    fn holds_resources(&self) -> bool;
}

impl<T: Any + Clone> RecordValue for T {
    fn holds_resources(&self) -> bool {
        std::mem::needs_drop::<T>()
    }

    fn clone_boxed(&self) -> Box<dyn RecordValue> {
        Box::new(self.clone())
    }

    fn assign_into(&self, target: &mut Option<Box<dyn RecordValue>>) {
        match target
            .as_mut()
            .and_then(|value| (&mut **value as &mut dyn Any).downcast_mut::<T>())
        {
            Some(slot) => slot.clone_from(self),
            None => *target = Some(Box::new(self.clone())),
        }
    }
}

#[derive(Debug)]
struct StateReadFailure {
    state_id: ObjectId,
    snapshot_id: SnapshotId,
    fresh_snapshot_id: SnapshotId,
    fresh_invalid: SnapshotIdSet,
    record_chain: Vec<(SnapshotId, bool)>,
}

impl fmt::Display for StateReadFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
    Missing,
}

impl StateRecord {
    pub(crate) fn new<T: Any + Clone>(snapshot_id: SnapshotId, value: T) -> Rc<Self> {
        Self::with_boxed(snapshot_id, Box::new(value))
    }

    fn with_boxed(snapshot_id: SnapshotId, value: Box<dyn RecordValue>) -> Rc<Self> {
        Rc::new(Self {
            snapshot_id: Cell::new(snapshot_id),
            tombstone: Cell::new(false),
            value: RefCell::new(Some(value)),
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
    pub(crate) fn is_tombstone(&self) -> bool {
        self.tombstone.get()
    }

    #[inline]
    pub(crate) fn set_tombstone(&self, tombstone: bool) {
        self.tombstone.set(tombstone);
    }

    /// Takes the value out of a record that no read reaches any more, for
    /// the caller to drop once the operation that cleared it is done: its
    /// destructor may read or write state. A value without drop glue holds
    /// nothing to release and stays, so the record's next write stores over
    /// it in place instead of boxing a new value.
    #[must_use = "drop the cleared value once the record chain is consistent"]
    pub(crate) fn clear_value(&self) -> Option<Box<dyn RecordValue>> {
        let mut value = self.value.borrow_mut();
        if value
            .as_ref()
            .is_some_and(|value| (**value).holds_resources())
        {
            value.take()
        } else {
            None
        }
    }

    /// Marks the record dead and retires its value, for an operation that
    /// holds a [`RetiredValuesScope`].
    pub(crate) fn tombstone_retiring_value(&self) {
        self.set_tombstone(true);
        if let Some(value) = self.clear_value() {
            retire_record_value(value);
        }
    }

    /// Stores `new_value` and returns the value it displaced, for the caller
    /// to drop once the write is published: its destructor may read or write
    /// state.
    #[must_use = "drop the displaced value once the write is published"]
    pub(crate) fn replace_value<T: Any + Clone>(&self, new_value: T) -> Option<T> {
        let mut value = self.value.borrow_mut();
        if let Some(slot) = value
            .as_mut()
            .and_then(|current| (&mut **current as &mut dyn Any).downcast_mut::<T>())
        {
            return Some(std::mem::replace(slot, new_value));
        }
        let placeholder = value.replace(Box::new(new_value));
        drop(value);
        drop(placeholder);
        None
    }

    fn cloned_value(&self) -> Option<Box<dyn RecordValue>> {
        self.value
            .borrow()
            .as_ref()
            .map(|value| (**value).clone_boxed())
    }

    fn cloned_value_of(&self, expected: TypeId) -> Option<Box<dyn RecordValue>> {
        self.value
            .borrow()
            .as_ref()
            .filter(|value| (&***value as &dyn Any).type_id() == expected)
            .map(|value| (**value).clone_boxed())
    }

    fn set_boxed_value(&self, value: Box<dyn RecordValue>) {
        let displaced = self.value.borrow_mut().replace(value);
        if let Some(displaced) = displaced {
            retire_record_value(displaced);
        }
    }

    pub(crate) fn with_value<T: Any, R>(&self, f: impl FnOnce(&T) -> R) -> R {
        self.try_with_value(f)
            .unwrap_or_else(|| panic!("StateRecord value missing or wrong type"))
    }

    pub(crate) fn try_with_value<T: Any, R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        let guard = self.value.borrow();
        let value = guard
            .as_ref()
            .and_then(|boxed| (&**boxed as &dyn Any).downcast_ref::<T>())?;
        Some(f(value))
    }

    #[cfg(test)]
    pub(crate) fn clear_for_reuse(&self) {
        drop(self.value.borrow_mut().take());
    }

    pub(crate) fn assign_value(&self, source: &Self) -> Result<(), StateRecordValueError> {
        if std::ptr::eq(self, source) {
            return match self.value.borrow().is_some() {
                true => Ok(()),
                false => Err(StateRecordValueError::Missing),
            };
        }
        let source_value = source.value.borrow();
        let value = source_value
            .as_ref()
            .ok_or(StateRecordValueError::Missing)?;
        (**value).assign_into(&mut self.value.borrow_mut());
        Ok(())
    }
}

#[cold]
fn log_merge_value_missing(which: &str, state: ObjectId, record: &StateRecord) {
    log::error!(
        "SnapshotMutableState::merge_records {which} record value missing or wrong type (state {state:?}, {which}_id={})",
        record.snapshot_id()
    );
}

fn install_copy_as_head(
    state: &dyn StateObject,
    source: &StateRecord,
    expected: TypeId,
) -> Option<SnapshotId> {
    let value = source.cloned_value_of(expected)?;
    let new_id = allocate_record_id();
    let record = state.records().overwritable_head();
    record.set_boxed_value(value);
    record.set_tombstone(false);
    record.set_snapshot_id(new_id);
    advance_global_snapshot(new_id);
    Some(new_id)
}

fn promote_record_locked(
    state: &dyn StateObject,
    child_id: SnapshotId,
    expected: TypeId,
) -> Result<(), &'static str> {
    let Some(record) = state.records().at(child_id) else {
        log::error!(
            "SnapshotMutableState::promote_record missing child record (state {:?}, child_id={})",
            state.object_id(),
            child_id
        );
        return Err("missing child record");
    };
    if install_copy_as_head(state, &record, expected).is_none() {
        log::error!(
            "SnapshotMutableState::promote_record child record value missing or wrong type (state {:?}, child_id={})",
            state.object_id(),
            child_id
        );
        return Err("child record value missing or wrong type");
    }
    Ok(())
}

fn commit_merged_record_locked(
    state: &dyn StateObject,
    merged: &StateRecord,
    expected: TypeId,
) -> Result<SnapshotId, &'static str> {
    install_copy_as_head(state, merged, expected).ok_or_else(|| {
        log::error!(
            "SnapshotMutableState::commit_merged_record merged record value missing or wrong type (state {:?}, merged_id={})",
            state.object_id(),
            merged.snapshot_id()
        );
        "merged record value missing or wrong type"
    })
}

/// Drops `value`, a record value displaced inside snapshot bookkeeping, once
/// the outermost [`RetiredValuesScope`] ends, or now when none is open: its
/// destructor may read or write state.
pub(crate) fn retire_record_value(value: Box<dyn RecordValue>) {
    let held = crate::thread_state::try_snapshots(|state| {
        if state.retire_depth.get() == 0 {
            return Some(value);
        }
        state.retired_record_values.borrow_mut().push(value);
        None
    });
    drop(held);
}

/// Holds back the drop of retired record values until the outermost scope
/// ends, so no destructor runs while snapshot bookkeeping is half done.
pub(crate) struct RetiredValuesScope(());

impl RetiredValuesScope {
    pub(crate) fn enter() -> Self {
        crate::thread_state::snapshots(|state| {
            state.retire_depth.set(state.retire_depth.get() + 1);
        });
        Self(())
    }
}

impl Drop for RetiredValuesScope {
    fn drop(&mut self) {
        let retired = crate::thread_state::snapshots(|state| {
            let depth = state.retire_depth.get() - 1;
            state.retire_depth.set(depth);
            (depth == 0).then(|| state.retired_record_values.take())
        });
        drop(retired);
    }
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
    fn records(&self) -> &RecordChain;
    fn try_readable_record(
        &self,
        snapshot_id: SnapshotId,
        invalid: &SnapshotIdSet,
    ) -> Option<Rc<StateRecord>>;
    fn readable_record(&self, snapshot_id: SnapshotId, invalid: &SnapshotIdSet) -> Rc<StateRecord>;

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

    fn overwrite_unused_records(&self) -> bool {
        false
    }

    fn observation_lease(&self) -> Option<Rc<dyn Any>> {
        None
    }

    fn as_any(&self) -> &dyn Any;
}

pub(crate) struct SnapshotMutableState<T> {
    records: RecordChain,
    policy: Rc<dyn MutationPolicy<T>>,
    id: ObjectId,
    weak_self: Weak<Self>,
    apply_observer: ApplyObserver,
    subscribers: Subscribers,
}

// Who subscribes to a state, whatever its value's type: the leases on its
// reads, the scopes that observe it and the callbacks to run when it gains
// its first subscriber. Kept apart from the value's type, so its code is
// compiled once for every state type.
#[derive(Default)]
pub(crate) struct Subscribers {
    lease: Rc<()>,
    scopes: Cell<usize>,
    callbacks: RefCell<Vec<Rc<dyn Fn()>>>,
}

impl Subscribers {
    fn acquire_observation_lease(&self) -> Option<Rc<dyn Any>> {
        let was_empty = !self.has_subscribers();
        let lease = Rc::clone(&self.lease);
        if was_empty {
            notify_subscriber_callbacks(&self.callbacks);
        }
        Some(lease)
    }

    fn add_scope_observer(&self) -> bool {
        let was_empty = !self.has_subscribers();
        let count = self
            .scopes
            .get()
            .checked_add(1)
            .expect("state scope observation count overflow");
        self.scopes.set(count);
        was_empty
    }

    fn remove_scope_observers(&self, count: usize) {
        if count == 0 {
            return;
        }
        let remaining = self
            .scopes
            .get()
            .checked_sub(count)
            .expect("state scope observation count underflow");
        self.scopes.set(remaining);
    }

    fn has_subscribers(&self) -> bool {
        Rc::strong_count(&self.lease) > 1 || self.scopes.get() > 0
    }

    fn subscriber_callback(&self, callback: Rc<dyn Fn()>, notify: bool) {
        self.callbacks.borrow_mut().push(Rc::clone(&callback));
        if notify {
            callback();
        }
    }

    fn notify_subscribers(&self) {
        notify_subscriber_callbacks(&self.callbacks);
    }
}

#[inline(always)]
fn assert_record_chain(
    records: &RecordChain,
    id: ObjectId,
    caller: &str,
    snapshot_context: Option<SnapshotId>,
) {
    if should_check_chain_integrity() {
        records.check_integrity(id, caller, snapshot_context);
    }
}

enum Merge<'a, T> {
    KeepCurrent,
    Value(Option<T>),
    Missing(&'static str, &'a Rc<StateRecord>),
}

type ApplyObserver = OnceCell<Box<dyn Fn() + 'static>>;

fn notify_applied(observer: &ApplyObserver) {
    if let Some(observer) = observer.get() {
        observer();
    }
}

fn finish_apply(
    observer: &ApplyObserver,
    records: &RecordChain,
    id: ObjectId,
    caller: &str,
    snapshot_id: SnapshotId,
) {
    notify_applied(observer);
    assert_record_chain(records, id, caller, Some(snapshot_id));
}

// What a write does that does not depend on the state's value type, so it
// is compiled once instead of once per state type.
struct WriteTarget<'a> {
    records: &'a RecordChain,
    id: ObjectId,
}

impl WriteTarget<'_> {
    fn assert_chain_integrity(&self, caller: &str, snapshot_context: Option<SnapshotId>) {
        assert_record_chain(self.records, self.id, caller, snapshot_context);
    }

    fn begin_global_write(&self, global: &GlobalSnapshot, snapshot_id: SnapshotId) {
        assert!(
            !global.has_pending_children(),
            "SnapshotMutableState::set attempted global write while pending children {:?} exist (state {:?}, snapshot_id={})",
            global.pending_children(),
            self.id,
            snapshot_id
        );
        mark_update_write(self.id);
    }

    /// Returns a head record for a global write and the id under which
    /// [`Self::publish_global_record`] publishes it.
    fn push_global_record(&self) -> (Rc<StateRecord>, SnapshotId) {
        let new_id = allocate_record_id();
        (self.records.overwritable_head(), new_id)
    }

    /// Publishes a pushed record once its value is stored: nothing may read
    /// the record while the store runs.
    fn publish_global_record(
        &self,
        record: &StateRecord,
        new_id: SnapshotId,
        snapshot_id: SnapshotId,
    ) {
        record.set_snapshot_id(new_id);
        record.set_tombstone(false);
        advance_global_snapshot(new_id);
        self.assert_chain_integrity("set(global-push)", Some(snapshot_id));
    }

    /// Finishes a global write that stored its value in `record`, the record
    /// at the global snapshot's current id.
    fn finish_global_write(
        &self,
        record: &StateRecord,
        written: Option<Rc<dyn StateObject>>,
        snapshot_id: SnapshotId,
    ) {
        let mut cleared: SmallVec<[Box<dyn RecordValue>; 2]> = SmallVec::new();
        self.records.clear_hidden_after(record, &mut cleared);
        self.assert_chain_integrity("set(global-tombstone)", Some(snapshot_id));
        if let Some(modified) = written.as_ref() {
            crate::snapshot_v2::notify_apply_observers(
                std::slice::from_ref(modified),
                record.snapshot_id(),
            );
        }
        drop(cleared);
    }

    fn begin_child_write(
        &self,
        snapshot: &AnySnapshot,
        snapshot_id: SnapshotId,
        invalid: &SnapshotIdSet,
        written: Option<Rc<dyn StateObject>>,
    ) -> Rc<StateRecord> {
        if let Some(state) = written {
            snapshot.record_write(state);
        }
        mark_update_write(self.id);
        self.writable_record(snapshot_id, invalid)
    }

    fn writable_record(&self, snapshot_id: SnapshotId, invalid: &SnapshotIdSet) -> Rc<StateRecord> {
        let Some(readable) = self.records.readable(snapshot_id, invalid) else {
            let cloned_value = self
                .records
                .head()
                .cloned_value()
                .unwrap_or_else(|| panic!("StateRecord value missing or wrong type"));
            let new_head = StateRecord::with_boxed(snapshot_id, cloned_value);
            self.records.prepend(Rc::clone(&new_head));
            self.assert_chain_integrity("writable_record(recover)", Some(snapshot_id));
            return new_head;
        };

        if readable.snapshot_id() == snapshot_id {
            return readable;
        }

        let overwritable = self.records.overwritable();
        if let Err(error) = overwritable.assign_value(&readable) {
            log::error!(
                "snapshot writable record could not copy the readable value for state {:?}: {:?}",
                self.id,
                error
            );
        }
        overwritable.set_snapshot_id(snapshot_id);
        overwritable.set_tombstone(false);

        self.assert_chain_integrity("writable_record(reuse)", Some(snapshot_id));

        overwritable
    }
}

// The steps of a write that depend on the state's value type; `write_value`
// takes the rest, compiled once for every state type.
trait PendingWrite {
    // Whether `record` holds a value the state's policy deems the new one.
    fn matches(&self, record: &StateRecord) -> bool;
    // Moves the new value into `record` and keeps the value it displaced, to
    // drop once the write is published.
    fn store(&mut self, record: &StateRecord);
    // The state written, for the snapshots to report.
    fn written(&self) -> Option<Rc<dyn StateObject>>;
}

struct TypedWrite<'a, T> {
    state: &'a SnapshotMutableState<T>,
    value: Option<T>,
    displaced: Option<T>,
}

impl<T: Clone + 'static> PendingWrite for TypedWrite<'_, T> {
    fn matches(&self, record: &StateRecord) -> bool {
        self.value.as_ref().is_some_and(|value| {
            record.with_value(|current: &T| self.state.policy.equivalent(current, value))
        })
    }

    fn store(&mut self, record: &StateRecord) {
        if let Some(value) = self.value.take() {
            self.displaced = record.replace_value(value);
        }
    }

    fn written(&self) -> Option<Rc<dyn StateObject>> {
        self.state.written_state()
    }
}

// Writes the value `write` holds to the state `target` names, in the active
// snapshot, and returns whether it changed the value that snapshot reads.
// Out of line, one function for every state type.
#[inline(never)]
fn write_value(target: &WriteTarget<'_>, write: &mut dyn PendingWrite) -> bool {
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
                target.id
            );
        }
    }

    let snapshot = active_snapshot();
    let snapshot_id = snapshot.snapshot_id();
    let readable_for =
        |snapshot_id, invalid: &SnapshotIdSet| target.records.readable(snapshot_id, invalid);

    match &snapshot {
        AnySnapshot::Global(global) => {
            let readable = snapshot.with_invalid(|invalid| readable_for(snapshot_id, invalid));
            if readable
                .as_deref()
                .is_some_and(|record| write.matches(record))
            {
                return false;
            }
            target.begin_global_write(global, snapshot_id);
            // A record the global snapshot wrote at its current id is
            // seen by no other snapshot: one taken since would have moved
            // the global to a new id. The write replaces its value there,
            // as Compose overwrites the record of the snapshot it writes.
            if let Some(record) = readable.filter(|record| record.snapshot_id() == snapshot_id) {
                write.store(&record);
                target.finish_global_write(&record, write.written(), snapshot_id);
            } else {
                let (record, new_id) = target.push_global_record();
                write.store(&record);
                target.publish_global_record(&record, new_id, snapshot_id);
                target.finish_global_write(&record, write.written(), snapshot_id);
            }
        }
        AnySnapshot::Mutable(_)
        | AnySnapshot::NestedMutable(_)
        | AnySnapshot::TransparentMutable(_) => {
            let invalid = snapshot.invalid();
            if readable_for(snapshot_id, &invalid)
                .as_deref()
                .is_some_and(|record| write.matches(record))
            {
                return false;
            }
            let record =
                target.begin_child_write(&snapshot, snapshot_id, &invalid, write.written());
            write.store(&record);
            target.assert_chain_integrity("set(child-writable)", Some(snapshot_id));
        }
        AnySnapshot::Readonly(_)
        | AnySnapshot::NestedReadonly(_)
        | AnySnapshot::TransparentReadonly(_) => {
            panic!("Cannot write to a read-only snapshot");
        }
    }

    true
}

fn should_check_chain_integrity() -> bool {
    #[cfg(debug_assertions)]
    {
        true
    }

    #[cfg(not(debug_assertions))]
    {
        crate::DIAGNOSTICS.assert_state_chain.is_set()
    }
}

// The record of the state `state`, whose records are `records`, that the
// active snapshot reads, after a fresh snapshot's and the global one's when
// that snapshot's own misses. Out of line, one function for every state
// type.
#[inline(never)]
fn readable_record_for_active_snapshot(
    state: &dyn StateObject,
    records: &RecordChain,
    id: ObjectId,
) -> Result<Rc<StateRecord>, StateReadFailure> {
    let readable = |snapshot_id, invalid: &SnapshotIdSet| records.readable(snapshot_id, invalid);
    let snapshot = active_snapshot();
    snapshot.record_read(state);

    let snapshot_id = snapshot.snapshot_id();
    if let Some(record) = snapshot.with_invalid(|invalid| readable(snapshot_id, invalid)) {
        return Ok(record);
    }

    let fresh_snapshot = active_snapshot();
    let fresh_id = fresh_snapshot.snapshot_id();
    let fresh_invalid = fresh_snapshot.invalid();

    if let Some(record) = readable(fresh_id, &fresh_invalid) {
        return Ok(record);
    }

    let global = GlobalSnapshot::get_or_create();
    let global_id = global.snapshot_id();
    let global_invalid = global.invalid();

    if let Some(record) = readable(global_id, &global_invalid) {
        return Ok(record);
    }

    Err(StateReadFailure {
        state_id: id,
        snapshot_id,
        fresh_snapshot_id: fresh_id,
        fresh_invalid,
        record_chain: records.ids(),
    })
}

impl<T: Clone + 'static> SnapshotMutableState<T> {
    fn readable_record_for_active_snapshot(&self) -> Result<Rc<StateRecord>, StateReadFailure> {
        readable_record_for_active_snapshot(self, &self.records, self.id)
    }

    fn readable_for(
        &self,
        snapshot_id: SnapshotId,
        invalid: &SnapshotIdSet,
    ) -> Option<Rc<StateRecord>> {
        self.records.readable(snapshot_id, invalid)
    }

    pub(crate) fn new_in_rc(initial: T, policy: Rc<dyn MutationPolicy<T>>) -> Rc<Self> {
        let snapshot = active_snapshot();
        let snapshot_id = snapshot.snapshot_id();

        let tail = StateRecord::new(PREEXISTING_SNAPSHOT_ID, initial.clone());
        let head = StateRecord::new(snapshot_id, initial);

        Rc::new_cyclic(|weak_self| Self {
            records: RecordChain::new(smallvec![head, tail]),
            policy,
            id: ObjectId(weak_self.as_ptr() as usize),
            weak_self: Weak::clone(weak_self),
            apply_observer: OnceCell::new(),
            subscribers: Subscribers::default(),
        })
    }

    pub(crate) fn set_apply_observer(&self, observer: Box<dyn Fn() + 'static>) {
        assert!(
            self.apply_observer.set(observer).is_ok(),
            "state observer already installed"
        );
    }

    fn finish_apply(&self, caller: &str, snapshot_id: SnapshotId) {
        finish_apply(
            &self.apply_observer,
            &self.records,
            self.id,
            caller,
            snapshot_id,
        );
    }

    fn upgrade_self(&self) -> Option<Rc<Self>> {
        self.weak_self.upgrade()
    }

    fn written_state(&self) -> Option<Rc<dyn StateObject>> {
        self.upgrade_self()
            .map(|state| -> Rc<dyn StateObject> { state })
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

    pub(crate) fn with_value<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let record = self
            .readable_record_for_active_snapshot()
            .unwrap_or_else(|failure| panic!("{failure}"));
        record.with_value(f)
    }

    pub(crate) fn get(&self) -> T {
        self.with_value(Clone::clone)
    }

    pub(crate) fn set(&self, new_value: T) -> bool {
        let target = WriteTarget {
            records: &self.records,
            id: self.id,
        };
        let mut write = TypedWrite {
            state: self,
            value: Some(new_value),
            displaced: None,
        };
        // The value the write displaced drops with `write`, once the write
        // is published.
        write_value(&target, &mut write)
    }

    fn try_readable_record(
        &self,
        snapshot_id: SnapshotId,
        invalid: &SnapshotIdSet,
    ) -> Option<Rc<StateRecord>> {
        self.readable_for(snapshot_id, invalid)
    }
}

pub(crate) struct UpdateScope {
    id: ObjectId,
    finished: bool,
}

impl UpdateScope {
    pub(crate) fn new(id: ObjectId) -> Self {
        crate::thread_state::snapshots(|state| {
            state.active_updates.borrow_mut().insert(id);
            state.pending_writes.borrow_mut().remove(&id);
        });
        Self {
            id,
            finished: false,
        }
    }

    pub(crate) fn finish(mut self) -> bool {
        self.finished = true;
        crate::thread_state::snapshots(|state| {
            state.active_updates.borrow_mut().remove(&self.id);
            state.pending_writes.borrow_mut().remove(&self.id)
        })
    }
}

impl Drop for UpdateScope {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        crate::thread_state::snapshots(|state| {
            state.active_updates.borrow_mut().remove(&self.id);
            state.pending_writes.borrow_mut().remove(&self.id);
        });
    }
}

fn mark_update_write(id: ObjectId) {
    crate::thread_state::snapshots(|state| {
        if state.active_updates.borrow().contains(&id) {
            state.pending_writes.borrow_mut().insert(id);
        }
    });
}

impl<T: Clone + 'static> StateObject for SnapshotMutableState<T> {
    fn object_id(&self) -> ObjectId {
        self.id
    }

    fn records(&self) -> &RecordChain {
        &self.records
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

    fn observation_lease(&self) -> Option<Rc<dyn Any>> {
        self.subscribers.acquire_observation_lease()
    }

    fn merge_records(
        &self,
        previous: Rc<StateRecord>,
        current: Rc<StateRecord>,
        applied: Rc<StateRecord>,
    ) -> Option<Rc<StateRecord>> {
        let outcome = current
            .try_with_value(|current_value: &T| {
                applied
                    .try_with_value(|applied_value: &T| {
                        if self.policy.equivalent(current_value, applied_value) {
                            return Merge::KeepCurrent;
                        }
                        previous
                            .try_with_value(|previous_value: &T| {
                                Merge::Value(self.policy.merge(
                                    previous_value,
                                    current_value,
                                    applied_value,
                                ))
                            })
                            .unwrap_or(Merge::Missing("previous", &previous))
                    })
                    .unwrap_or(Merge::Missing("applied", &applied))
            })
            .unwrap_or(Merge::Missing("current", &current));
        match outcome {
            Merge::KeepCurrent => Some(current),
            Merge::Value(merged) => {
                merged.map(|value| StateRecord::new(applied.snapshot_id(), value))
            }
            Merge::Missing(which, record) => {
                log_merge_value_missing(which, self.id, record);
                None
            }
        }
    }

    fn promote_record(&self, child_id: SnapshotId) -> Result<(), &'static str> {
        promote_record_locked(self, child_id, TypeId::of::<T>())?;
        self.finish_apply("promote_record", child_id);
        Ok(())
    }

    fn commit_merged_record(&self, merged: Rc<StateRecord>) -> Result<SnapshotId, &'static str> {
        let new_id = commit_merged_record_locked(self, &merged, TypeId::of::<T>())?;
        self.finish_apply("commit_merged_record", new_id);
        Ok(new_id)
    }

    fn overwrite_unused_records(&self) -> bool {
        self.records.overwrite_unused()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub(crate) struct MutableStateInner<T: Clone + 'static> {
    pub(crate) state: Rc<SnapshotMutableState<T>>,
    watchers: StateWatchers,
}

// The scopes that read a state and the runtime they recompose in, whatever
// the state's value type, so that registering a reader and invalidating the
// readers are compiled once for every state type.
struct StateWatchers {
    scopes: RefCell<HashMap<ScopeId, Weak<RecomposeScopeInner>>>,
    runtime: RuntimeHandle,
    state_id: Cell<Option<StateId>>,
}

fn notify_subscriber_callbacks(callbacks: &RefCell<Vec<Rc<dyn Fn()>>>) {
    let registered = callbacks.borrow().len();
    for index in 0..registered {
        let callback = callbacks.borrow().get(index).map(Rc::clone);
        if let Some(callback) = callback {
            callback();
        }
    }
}

fn shrink_watchers_if_sparse(watchers: &mut HashMap<ScopeId, Weak<RecomposeScopeInner>>) {
    let len = watchers.len();
    let capacity = watchers.capacity();
    if capacity > len.saturating_mul(4).max(32) {
        watchers.shrink_to_fit();
    }
}

// What a state's apply observer runs: invalidates the scopes that read the
// state, on the UI thread. The arena finds the readers by the state's id, so
// the closure is one function for every state type.
fn watcher_invalidation(runtime: RuntimeHandle, state_id: StateId) -> Box<dyn Fn()> {
    Box::new(move || {
        let ui_runtime = runtime.clone();
        runtime.enqueue_ui_task(Box::new(move || {
            ui_runtime.with_state_arena(|arena| arena.invalidate_watchers(state_id));
        }));
    })
}

impl<T: Clone + 'static> MutableStateInner<T> {
    pub(crate) fn new_with_policy(
        value: T,
        runtime: RuntimeHandle,
        policy: Rc<dyn MutationPolicy<T>>,
    ) -> Self {
        Self {
            state: SnapshotMutableState::new_in_rc(value, policy),
            watchers: StateWatchers {
                scopes: RefCell::new(HashMap::default()),
                runtime,
                state_id: Cell::new(None),
            },
        }
    }

    pub(crate) fn install_snapshot_observer(&self, state_id: StateId) {
        self.watchers.state_id.set(Some(state_id));
        self.state.set_apply_observer(watcher_invalidation(
            self.watchers.runtime.clone(),
            state_id,
        ));
    }

    fn has_subscribers(&self) -> bool {
        self.watchers.has_subscribers(&self.state.subscribers)
    }

    /// Removes the live scope `scope_id` from the watchers: its last body did
    /// not read the state.
    pub(crate) fn forget_scope(&self, scope_id: ScopeId) {
        self.watchers
            .forget_scope(&self.state.subscribers, scope_id);
    }

    pub(crate) fn unregister_scope(&self, scope_id: ScopeId) {
        self.watchers
            .unregister_scope(&self.state.subscribers, scope_id);
    }

    pub(crate) fn invalidate_watchers(&self) {
        self.watchers
            .invalidate(&self.state.subscribers, std::any::type_name::<T>());
    }

    // Subscribes the scope that is composing to this state.
    fn register_current_scope(&self) {
        register_current_state_scope(&self.watchers, &self.state.subscribers);
    }
}

impl StateWatchers {
    /// Adds `scope` to the state's watchers, saying whether it was new and
    /// whether the state thereby gained its first subscriber.
    ///
    /// Dead watchers still count as subscribers until pruned. They are
    /// pruned whenever no live one is left, so a reader arriving after every
    /// earlier one died still makes the state newly subscribed, and
    /// otherwise only when the map is full, before the insert would grow
    /// it: a state read by thousands of scopes, such as the density, must
    /// not scan them all for every new reader.
    fn register_scope(&self, subscribers: &Subscribers, scope: &RecomposeScope) -> (bool, bool) {
        let mut watchers = self.scopes.borrow_mut();
        let id = scope.id();
        match watchers.get(&id) {
            Some(existing) if existing.strong_count() > 0 => return (false, false),
            Some(_) => {
                watchers.remove(&id);
                subscribers.remove_scope_observers(1);
            }
            None => {}
        }
        let any_live = watchers.values().any(|watcher| watcher.strong_count() > 0);
        if !any_live || watchers.len() >= watchers.capacity() {
            let before = watchers.len();
            watchers.retain(|_, watcher| watcher.strong_count() > 0);
            subscribers.remove_scope_observers(before - watchers.len());
        }
        watchers.insert(id, scope.downgrade());
        drop(watchers);
        (true, subscribers.add_scope_observer())
    }

    fn has_subscribers(&self, subscribers: &Subscribers) -> bool {
        let mut watchers = self.scopes.borrow_mut();
        let before = watchers.len();
        watchers.retain(|_, existing| existing.upgrade().is_some());
        subscribers.remove_scope_observers(before - watchers.len());
        subscribers.has_subscribers()
    }

    fn forget_scope(&self, subscribers: &Subscribers, scope_id: ScopeId) {
        let mut watchers = self.scopes.borrow_mut();
        let removed = watchers.remove(&scope_id).is_some();
        if removed {
            shrink_watchers_if_sparse(&mut watchers);
        }
        drop(watchers);
        subscribers.remove_scope_observers(usize::from(removed));
    }

    fn unregister_scope(&self, subscribers: &Subscribers, scope_id: ScopeId) {
        let mut watchers = self.scopes.borrow_mut();
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
        subscribers.remove_scope_observers(usize::from(removed));
    }

    // Invalidates the live scopes that read the state, whose value type is
    // named `type_name`, and drops the dead ones.
    fn invalidate(&self, subscribers: &Subscribers, type_name: &'static str) {
        // A state has few watchers: their copy stays inline.
        let (watchers, removed_count): (SmallVec<[RecomposeScope; 4]>, usize) = {
            let mut watchers = self.scopes.borrow_mut();
            let before = watchers.len();
            let mut live = SmallVec::new();
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
        subscribers.remove_scope_observers(removed_count);

        for watcher in watchers {
            debug_record_scope_invalidation(watcher.id(), self.state_id.get(), type_name);
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
            .subscribers
            .remove_scope_observers(self.watchers.scopes.get_mut().len());
    }
}

// Out of line, one function for every state type: each read of a state
// registers its reader here.
#[inline(never)]
fn register_current_state_scope(watchers: &StateWatchers, subscribers: &Subscribers) {
    let state_id = watchers.state_id.get();
    let Some(Some((scope, read))) =
        with_current_composer_opt(|composer| composer.note_state_read(state_id))
    else {
        return;
    };
    let (registered, became_subscribed) = watchers.register_scope(subscribers, &scope);
    if let (crate::ScopeRead::NotReading, true, Some(state_id)) = (read, registered, state_id) {
        scope.record_state_subscription(state_id);
    }
    if became_subscribed {
        subscribers.notify_subscribers();
    }
}

trait StateArenaHandle<T: Clone + 'static> {
    fn state_id(&self) -> StateId;
    fn runtime_id(&self) -> runtime::RuntimeId;

    fn runtime_handle(&self) -> RuntimeHandle {
        runtime::runtime_handle_by_id(self.runtime_id())
            .unwrap_or_else(|| panic!("runtime {:?} dropped", self.runtime_id()))
    }

    fn runtime_handle_opt(&self) -> Option<RuntimeHandle> {
        runtime::runtime_handle_by_id(self.runtime_id())
    }

    fn with_inner<R>(&self, f: impl FnOnce(&MutableStateInner<T>) -> R) -> R {
        runtime::with_state_arena_by_id(self.runtime_id(), |arena| {
            arena.with_typed::<T, R>(self.state_id(), f)
        })
        .unwrap_or_else(|| panic!("runtime {:?} dropped", self.runtime_id()))
    }

    fn try_with_inner<R>(&self, f: impl FnOnce(&MutableStateInner<T>) -> R) -> Option<R> {
        runtime::with_state_arena_by_id(self.runtime_id(), |arena| {
            arena.with_typed_opt::<T, R>(self.state_id(), f)
        })?
    }

    fn read_subscribed<R>(&self, read: impl FnOnce(&MutableStateInner<T>) -> R) -> R {
        self.with_inner(|inner| {
            let result = read(inner);
            inner.register_current_scope();
            result
        })
    }
}

/// Cheap copyable read-only view of a state cell.
pub struct State<T: Clone + 'static> {
    id: StateId,
    runtime_id: runtime::RuntimeId,
    /// The cell holds a derived state's value, which a read brings up to
    /// date first.
    derived: bool,
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

/// Two owning handles are equal when they hold the same state, as two
/// [`MutableState`] handles are.
impl<T: Clone + 'static> PartialEq for OwnedMutableState<T> {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
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

impl<T: Clone + 'static> StateArenaHandle<T> for State<T> {
    fn state_id(&self) -> StateId {
        self.id
    }

    fn runtime_id(&self) -> runtime::RuntimeId {
        self.runtime_id
    }
}

impl<T: Clone + 'static> State<T> {
    /// Returns `true` while the state exists. It ends when its owner releases it or its runtime is
    /// dropped.
    pub fn is_alive(&self) -> bool {
        self.try_with_inner(|_| ()).is_some()
    }

    /// Reads the value through `f` without subscribing the current scope. Returns `None` if the
    /// state was released or has no readable value.
    pub fn try_with<R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        self.try_with_fresh_inner(|inner| inner.state.try_with_value(f))?
    }

    /// Returns a copy of the value without subscribing the current scope, or `None` if the state
    /// was released or has no readable value.
    pub fn try_value(&self) -> Option<T> {
        self.try_with_fresh_inner(|inner| inner.state.try_get())?
    }

    /// Reads a copy of the value through `f` and subscribes the current
    /// scope. `f` may write this state; see [`Self::read`] for a read that
    /// borrows instead of copying.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let value = self.read_fresh_subscribed(|inner| inner.state.get());
        f(&value)
    }

    /// Reads the value in place through `f` and subscribes the current
    /// scope. Nothing is cloned, so a read of a large value costs nothing
    /// beyond `f`; in return `f` borrows the stored value and must not write
    /// this state.
    pub fn read<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        self.read_fresh_subscribed(|inner| inner.state.with_value(f))
    }

    /// Returns a copy of the value and subscribes the current scope, which recomposes when the
    /// value changes. Panics if the state was released.
    pub fn value(&self) -> T {
        self.read_fresh_subscribed(|inner| inner.state.get())
    }

    /// Same as `value`.
    pub fn get(&self) -> T {
        self.value()
    }

    /// Reads the value without subscribing the current scope, for a
    /// composition that changes what it shows by the value only through a
    /// layer or a draw that reads it again.
    pub fn get_non_reactive(&self) -> T {
        self.with_fresh_inner(|inner| inner.state.get())
    }

    /// The view a derived state hands out of its cell.
    fn of_derived(self) -> Self {
        Self {
            derived: true,
            ..self
        }
    }

    /// Runs `f` on the cell, a derived state's once its value is up to date:
    /// a read derives the value again when a state it read changed since
    /// its derivation last ran, as Compose's `derivedStateOf` does. A plain
    /// state's read pays one branch. Panics if the state was released.
    fn with_fresh_inner<R>(&self, f: impl FnOnce(&MutableStateInner<T>) -> R) -> R {
        runtime::with_state_arena_by_id(self.runtime_id, |arena| {
            if self.derived {
                arena.refresh_derived(self.id);
            }
            arena.with_typed::<T, R>(self.id, f)
        })
        .unwrap_or_else(|| panic!("runtime {:?} dropped", self.runtime_id))
    }

    /// [`Self::with_fresh_inner`], or `None` if the state was released.
    fn try_with_fresh_inner<R>(&self, f: impl FnOnce(&MutableStateInner<T>) -> R) -> Option<R> {
        runtime::with_state_arena_by_id(self.runtime_id, |arena| {
            if self.derived {
                arena.refresh_derived(self.id);
            }
            arena.with_typed_opt::<T, R>(self.id, f)
        })?
    }

    /// Reads the up-to-date cell through `read` and subscribes the current
    /// scope.
    fn read_fresh_subscribed<R>(&self, read: impl FnOnce(&MutableStateInner<T>) -> R) -> R {
        self.with_fresh_inner(|inner| {
            let result = read(inner);
            inner.register_current_scope();
            result
        })
    }

    /// Returns `true` if any scope or observer is subscribed to this state.
    pub fn has_subscribers(&self) -> bool {
        self.with_inner(MutableStateInner::has_subscribers)
    }

    /// Runs `callback` each time this state gains its first subscriber, and
    /// right away when it already has one.
    ///
    /// The state owns `callback` for as long as the state lives, so nothing
    /// has to be kept alive on the caller's side.
    pub fn on_subscriber(&self, callback: impl Fn() + 'static) {
        let callback: Rc<dyn Fn()> = Rc::new(callback);
        self.with_inner(|inner| {
            inner
                .state
                .subscribers
                .subscriber_callback(callback, inner.has_subscribers());
        });
    }

    /// Counts this state as subscribed for as long as the returned guard
    /// lives, without observing it from any scope.
    ///
    /// Snapshot observers subscribe reads made under composition or the
    /// draw phase, and producers gated on [`State::has_subscribers`] — an
    /// infinite transition, for one — stop themselves when the last such
    /// subscriber leaves. A consumer that reads the value from a polling
    /// context (a frame effect stepping it into a derived state, a
    /// background task) is invisible to that accounting and would starve
    /// the producer it depends on. Holding this guard keeps the producer
    /// alive; dropping it releases the count.
    pub fn subscription_hold(&self) -> StateSubscriptionHold {
        StateSubscriptionHold {
            _lease: self
                .try_with_inner(|inner| inner.state.observation_lease())
                .flatten(),
        }
    }
}

/// Keeps a [`State`] counted as subscribed while alive; see
/// [`State::subscription_hold`].
pub struct StateSubscriptionHold {
    _lease: Option<Rc<dyn Any>>,
}

impl<T: Clone + 'static> StateArenaHandle<T> for MutableState<T> {
    fn state_id(&self) -> StateId {
        self.id
    }

    fn runtime_id(&self) -> runtime::RuntimeId {
        self.runtime_id
    }
}

// Why a write to `state_id` of `runtime_id` stored nothing: its runtime
// was dropped, or the runtime lives and the state was released. Out of line,
// one function for every state type.
#[cold]
#[inline(never)]
fn log_skipped_replace(runtime_id: runtime::RuntimeId, state_id: StateId, runtime_alive: bool) {
    if runtime_alive {
        log::debug!(
            "MutableState::replace skipped: state cell released (slot={}, gen={})",
            state_id.slot(),
            state_id.generation(),
        );
    } else {
        log::debug!("MutableState::replace skipped: runtime {runtime_id:?} dropped");
    }
}

impl<T: Clone + 'static> MutableState<T> {
    /// Creates a state holding `value` in `runtime`. Every write counts as a change, even of an
    /// equal value. Unless a `remember` owns the state, the runtime keeps it for its own lifetime.
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
        Self::from_parts(lease.id(), lease.runtime_id())
    }

    /// Returns `true` while the state exists. It ends when its owner releases it or its runtime is
    /// dropped.
    pub fn is_alive(&self) -> bool {
        self.try_with_inner(|_| ()).is_some()
    }

    /// Reads the value through `f` without subscribing the current scope. Returns `None` if the
    /// state was released or has no readable value.
    pub fn try_with<R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        self.try_with_inner(|inner| inner.state.try_with_value(f))?
    }

    /// Returns a copy of the value without subscribing the current scope, or `None` if the state
    /// was released or has no readable value.
    pub fn try_value(&self) -> Option<T> {
        self.try_with_inner(|inner| inner.state.try_get())?
    }

    /// Returns a read-only view of this state.
    pub fn as_state(&self) -> State<T> {
        State {
            id: self.id,
            runtime_id: self.runtime_id,
            derived: false,
            _marker: PhantomData,
        }
    }

    /// Returns an [`OwnedMutableState`] that keeps this state alive until its last clone is
    /// dropped. Returns `None` if the state was already released.
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

    /// Returns an [`OwnedMutableState`] that keeps this state alive until its last clone is
    /// dropped. Panics if the state was released.
    pub fn retain(&self) -> OwnedMutableState<T> {
        self.try_retain()
            .unwrap_or_else(|| panic!("state {:?} is no longer alive", self.state_id()))
    }

    /// Reads a copy of the value through `f` and subscribes the current
    /// scope. `f` may write this state; see [`Self::read`] for a read that
    /// borrows instead of copying.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let value = self.read_subscribed(|inner| inner.state.get());
        f(&value)
    }

    /// Reads the value in place through `f` and subscribes the current
    /// scope. Nothing is cloned, so a read of a large value costs nothing
    /// beyond `f`; in return `f` borrows the stored value and must not write
    /// this state.
    pub fn read<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        self.read_subscribed(|inner| inner.state.with_value(f))
    }

    /// Runs `f` on a copy of the value and stores the copy, unless `f` wrote this state itself.
    /// Scopes that read the state are notified if the value changed.
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

    /// Stores `value` and notifies the scopes that read the state, if the state's equality policy
    /// sees a change. Does nothing if the state was released.
    pub fn replace(&self, value: T) {
        // The runtimes this thread finds are its own, so a write found here
        // is on the runtime's UI thread.
        let replaced = runtime::with_state_arena_by_id(self.runtime_id(), |arena| {
            arena.with_typed_opt::<T, ()>(self.state_id(), |inner| {
                if inner.state.set(value) {
                    inner.invalidate_watchers();
                }
            })
        });
        if replaced != Some(Some(())) {
            log_skipped_replace(self.runtime_id(), self.state_id(), replaced.is_some());
        }
    }

    /// Same as `replace`.
    pub fn set_value(&self, value: T) {
        self.replace(value);
    }

    /// Same as `replace`.
    pub fn set(&self, value: T) {
        self.replace(value);
    }

    /// Returns a copy of the value and subscribes the current scope, which recomposes when the
    /// value changes. Panics if the state was released.
    pub fn value(&self) -> T {
        self.read_subscribed(|inner| inner.state.get())
    }

    /// Same as `value`.
    pub fn get(&self) -> T {
        self.value()
    }

    /// Returns a copy of the value without subscribing the current scope.
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
        self.with_inner(MutableStateInner::register_current_scope);
    }

    #[cfg(test)]
    pub(crate) fn watcher_count(&self) -> usize {
        self.with_inner(|inner| inner.watchers.scopes.borrow().len())
    }

    #[cfg(test)]
    pub(crate) fn watcher_capacity(&self) -> usize {
        self.with_inner(|inner| inner.watchers.scopes.borrow().capacity())
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
    /// Creates a state holding `value` in `runtime`. The state is released when the last clone of
    /// this handle is dropped. Every write counts as a change, even of an equal value.
    pub fn with_runtime(value: T, runtime: RuntimeHandle) -> Self {
        let lease = runtime.alloc_state(value);
        Self {
            state: MutableState::from_lease(&lease),
            _lease: lease,
            _marker: PhantomData,
        }
    }

    /// Creates a state holding `value` in `runtime`. Writing a value equal to the current one is
    /// not a change.
    pub fn with_runtime_structural_eq(value: T, runtime: RuntimeHandle) -> Self
    where
        T: PartialEq,
    {
        Self::with_runtime_and_policy(value, runtime, Rc::new(StructuralEqual))
    }

    pub(crate) fn with_runtime_and_policy(
        value: T,
        runtime: RuntimeHandle,
        policy: Rc<dyn MutationPolicy<T>>,
    ) -> Self {
        let lease = runtime.alloc_state_with_policy(value, policy);
        Self {
            state: MutableState::from_lease(&lease),
            _lease: lease,
            _marker: PhantomData,
        }
    }

    /// Returns the copyable [`MutableState`] handle. It does not keep the state alive.
    pub fn handle(&self) -> MutableState<T> {
        self.state
    }

    /// Returns a read-only view of the state.
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
            let (registered, became_subscribed) = inner
                .watchers
                .register_scope(&inner.state.subscribers, scope);
            if registered {
                if let Some(state_id) = inner.watchers.state_id.get() {
                    scope.record_state_subscription(state_id);
                }
                if became_subscribed {
                    inner.state.subscribers.notify_subscribers();
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

/// A hash map whose reads subscribe the current scope and whose writes notify its readers, as
/// Jetpack Compose's `SnapshotStateMap`. Clones share the same map.
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
    /// Creates a map holding `pairs` in `runtime`.
    pub fn with_runtime<I>(pairs: I, runtime: RuntimeHandle) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
    {
        let map: HashMap<K, V> = pairs.into_iter().collect();
        Self {
            state: OwnedMutableState::with_runtime(map, runtime),
        }
    }

    /// Returns a read-only view of the state that holds the map.
    pub fn as_state(&self) -> State<HashMap<K, V>> {
        self.state.as_state()
    }

    /// Returns the writable state that holds the map.
    pub fn as_mutable_state(&self) -> MutableState<HashMap<K, V>> {
        self.state.handle()
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.state.with(HashMap::len)
    }

    /// Returns `true` if the map has no entries.
    pub fn is_empty(&self) -> bool {
        self.state.with(HashMap::is_empty)
    }

    /// Returns `true` if the map has an entry for `key`.
    pub fn contains_key(&self, key: &K) -> bool {
        self.state.with(|map| map.contains_key(key))
    }

    /// Returns a copy of the value under `key`, or `None` if there is none.
    pub fn get(&self, key: &K) -> Option<V> {
        self.state.with(|map| map.get(key).cloned())
    }

    /// Returns a copy of the whole map.
    pub fn to_hash_map(&self) -> HashMap<K, V> {
        self.state.with(Clone::clone)
    }

    /// Stores `value` under `key` and returns the value it replaced, if any.
    pub fn insert(&self, key: K, value: V) -> Option<V> {
        self.state.update(|map| map.insert(key, value))
    }

    /// Inserts all pairs from `iter` in one write, replacing the values of keys already present.
    pub fn extend<I>(&self, iter: I)
    where
        I: IntoIterator<Item = (K, V)>,
    {
        self.state.update(|map| map.extend(iter));
    }

    /// Removes the entry under `key` and returns its value, or `None` if there was none.
    pub fn remove(&self, key: &K) -> Option<V> {
        self.state.update(|map| map.remove(key))
    }

    /// Removes all entries.
    pub fn clear(&self) {
        self.state.replace(HashMap::default());
    }

    /// Keeps the entries for which `predicate` returns `true`. The predicate may change the values
    /// it keeps.
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
#[path = "tests/state_tests.rs"]
mod tests;
