//! The record of a state that the active snapshot reads.

use std::{fmt, rc::Rc};

use super::{ObjectId, RecordChain, StateObject, StateRecord, active_snapshot};
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

// The record of the state `state`, whose records are `records`, that the
// active snapshot reads, after a fresh snapshot's and the global one's when
// that snapshot's own misses. Out of line, one function for every state
// type.
#[inline(never)]
pub(super) fn readable_record_for_active_snapshot(
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
