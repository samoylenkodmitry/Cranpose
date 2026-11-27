//! The record of a state that the active snapshot reads.

use std::{fmt, rc::Rc};

use super::{
    MutableState, ObjectId, RecordChain, SnapshotMutableState, State, StateArenaHandle,
    StateObject, StateRecord, active_snapshot,
};
use crate::{
    snapshot_id_set::{SnapshotId, SnapshotIdSet},
    snapshot_v2::GlobalSnapshot,
};

#[derive(Debug)]
pub(super) struct StateReadFailure {
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

// Who hears of a read of a state's record.
#[derive(Clone, Copy)]
pub(super) enum ReadNotice<'a> {
    // The active snapshot's read observers, which hear of the state.
    Observers(&'a dyn StateObject),
    // No observer, as for a read inside Compose's
    // `Snapshot.withoutReadObservation`.
    Nobody,
}

// The record of a state whose records are `records` that the active
// snapshot reads, after a fresh snapshot's and the global one's when that
// snapshot's own misses; `notice` says who hears of the read. Out of line,
// one function for every state type.
#[inline(never)]
pub(super) fn readable_record_for_active_snapshot(
    notice: ReadNotice<'_>,
    records: &RecordChain,
    id: ObjectId,
) -> Result<Rc<StateRecord>, StateReadFailure> {
    let readable = |snapshot_id, invalid: &SnapshotIdSet| records.readable(snapshot_id, invalid);
    let snapshot = active_snapshot();
    if let ReadNotice::Observers(state) = notice {
        snapshot.record_read(state);
    }

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

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the read that tells no observer, beside the record it reads"
)]
impl<T: Clone + 'static> SnapshotMutableState<T> {
    // The value, read without telling the snapshot's read observers.
    pub(crate) fn value_unobserved(&self) -> T {
        readable_record_for_active_snapshot(ReadNotice::Nobody, &self.records, self.id)
            .unwrap_or_else(|failure| panic!("{failure}"))
            .with_value(Clone::clone)
    }

    // The value, read without telling the snapshot's read observers, or `None`
    // when no record is readable.
    pub(crate) fn try_value_unobserved(&self) -> Option<T> {
        readable_record_for_active_snapshot(ReadNotice::Nobody, &self.records, self.id)
            .ok()?
            .try_with_value(Clone::clone)
    }
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "optional unobserved reads live beside the snapshot record reader"
)]
impl<T: Clone + 'static> State<T> {
    /// Reads the value without subscribing the current scope or telling a read observer. Returns
    /// `None` if the state was released or has no readable value.
    pub fn try_get_non_reactive(&self) -> Option<T> {
        self.try_with_fresh_inner(|inner| inner.state.try_value_unobserved())?
    }
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "optional reads live beside the snapshot record reader"
)]
impl<T: Clone + 'static> MutableState<T> {
    /// Returns a copy of the value without subscribing a scope or observer, or `None` if the state
    /// was released or has no readable value.
    pub fn try_get_non_reactive(&self) -> Option<T> {
        self.try_with_inner(|inner| inner.state.try_value_unobserved())?
    }

    /// Returns a copy of the value and subscribes the current scope, or `None` if the state was
    /// released or has no readable value.
    pub fn try_value_subscribed(&self) -> Option<T> {
        self.try_with_inner(|inner| {
            let value = inner.state.try_get()?;
            inner.register_current_scope();
            Some(value)
        })?
    }
}
