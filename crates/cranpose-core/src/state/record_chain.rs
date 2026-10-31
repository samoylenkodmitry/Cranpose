use std::{cell::RefCell, rc::Rc};

use smallvec::SmallVec;

use super::{
    INVALID_SNAPSHOT_ID, ObjectId, PREEXISTING_SNAPSHOT_ID, RecordValue, SNAPSHOT_ID_MAX,
    StateRecord, retire_record_value,
};
use crate::{
    collections::map::HashSet,
    snapshot_id_set::{SnapshotId, SnapshotIdSet},
    snapshot_pinning::lowest_pinned_snapshot,
};

// The records of one state, newest link first: the chain that Compose links
// from a state's first record. A walk borrows the records in place and
// clones only the record it returns.
pub struct RecordChain {
    records: RefCell<SmallVec<[Rc<StateRecord>; 2]>>,
}

#[inline]
fn record_is_valid_for(
    record: &StateRecord,
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

impl RecordChain {
    pub(crate) fn new(records: SmallVec<[Rc<StateRecord>; 2]>) -> Self {
        debug_assert!(!records.is_empty(), "a state keeps at least one record");
        Self {
            records: RefCell::new(records),
        }
    }

    pub(crate) fn head(&self) -> Rc<StateRecord> {
        Rc::clone(&self.records.borrow()[0])
    }

    pub(crate) fn prepend(&self, record: Rc<StateRecord>) {
        self.records.borrow_mut().insert(0, record);
    }

    // The newest record that the snapshot `snapshot_id`, which does not see
    // the snapshots in `invalid`, reads.
    pub(crate) fn readable(
        &self,
        snapshot_id: SnapshotId,
        invalid: &SnapshotIdSet,
    ) -> Option<Rc<StateRecord>> {
        let records = self.records.borrow();
        let mut best: Option<&Rc<StateRecord>> = None;
        for record in records.iter() {
            // A record no newer than the best one needs no validity check,
            // as the preexisting record behind a newer one.
            if best.is_none_or(|current| current.snapshot_id() < record.snapshot_id())
                && record_is_valid_for(record, snapshot_id, invalid)
            {
                best = Some(record);
            }
        }
        best.cloned()
    }

    // A record that no open snapshot reads, for a write to reuse: an invalid
    // one, a tombstone below the lowest pin, or the older of two records
    // that every open snapshot reads past.
    pub(crate) fn used(&self) -> Option<Rc<StateRecord>> {
        let reuse_limit = lowest_pinned_snapshot().map_or_else(
            || crate::snapshot_v2::peek_next_snapshot_id().previous(),
            SnapshotId::previous,
        );
        let records = self.records.borrow();
        let mut valid_record: Option<&Rc<StateRecord>> = None;
        for record in records.iter() {
            let current_id = record.snapshot_id();
            if current_id == PREEXISTING_SNAPSHOT_ID {
                continue;
            }
            if current_id == INVALID_SNAPSHOT_ID
                || (record.is_tombstone() && current_id < reuse_limit)
            {
                return Some(Rc::clone(record));
            }
            if record_is_valid_for(record, reuse_limit, &SnapshotIdSet::EMPTY) {
                let Some(existing) = valid_record else {
                    valid_record = Some(record);
                    continue;
                };
                let older = if current_id < existing.snapshot_id() {
                    record
                } else {
                    existing
                };
                return Some(Rc::clone(older));
            }
        }
        None
    }

    // A record for a write: a reused one where it is, or a new head.
    pub(crate) fn overwritable(&self) -> Rc<StateRecord> {
        let Some(reusable) = self.used() else {
            return self.prepend_overwritable();
        };
        reusable.set_snapshot_id(SNAPSHOT_ID_MAX);
        reusable
    }

    // A record for a write to own as the head: a reused one moved to the
    // head, the records before it keeping their order, or a new head.
    pub(crate) fn overwritable_head(&self) -> Rc<StateRecord> {
        let Some(reusable) = self.used() else {
            return self.prepend_overwritable();
        };
        reusable.set_snapshot_id(SNAPSHOT_ID_MAX);
        let mut records = self.records.borrow_mut();
        if let Some(at) = records.iter().position(|link| Rc::ptr_eq(link, &reusable)) {
            records[..=at].rotate_right(1);
        }
        drop(records);
        reusable
    }

    fn prepend_overwritable(&self) -> Rc<StateRecord> {
        let record = StateRecord::new(SNAPSHOT_ID_MAX, ());
        self.prepend(Rc::clone(&record));
        record
    }

    // The record with the id `id`, a tombstone or not.
    pub(crate) fn at(&self, id: SnapshotId) -> Option<Rc<StateRecord>> {
        self.records
            .borrow()
            .iter()
            .find(|record| record.snapshot_id() == id)
            .cloned()
    }

    // The live record with the id `id`.
    pub(crate) fn live_at(&self, id: SnapshotId) -> Option<Rc<StateRecord>> {
        self.records
            .borrow()
            .iter()
            .find(|record| !record.is_tombstone() && record.snapshot_id() == id)
            .cloned()
    }

    // The newest live record at or below `base_snapshot_id` outside
    // `invalid`, or else the first live record; and whether the newest
    // exists.
    pub(crate) fn previous(
        &self,
        base_snapshot_id: SnapshotId,
        invalid: &SnapshotIdSet,
    ) -> (Option<Rc<StateRecord>>, bool) {
        let records = self.records.borrow();
        let mut best: Option<&Rc<StateRecord>> = None;
        let mut fallback: Option<&Rc<StateRecord>> = None;
        for record in records.iter().filter(|record| !record.is_tombstone()) {
            let id = record.snapshot_id();
            if best.is_none_or(|current| current.snapshot_id() < id)
                && id <= base_snapshot_id
                && !invalid.get(id)
            {
                best = Some(record);
            }
            fallback = fallback.or(Some(record));
        }
        (best.or(fallback).cloned(), best.is_some())
    }

    // Marks the records that no open snapshot can read as free for reuse,
    // retiring their values, and reports whether the chain still keeps more
    // than one record.
    pub(crate) fn overwrite_unused(&self) -> bool {
        let reuse_limit =
            lowest_pinned_snapshot().unwrap_or_else(crate::snapshot_v2::peek_next_snapshot_id);
        let mut retired: SmallVec<[Box<dyn RecordValue>; 2]> = SmallVec::new();
        let mut retained_records = 0;
        let records = self.records.borrow();
        let mut valid_record: Option<&Rc<StateRecord>> = None;
        for record in records.iter() {
            let current_id = record.snapshot_id();
            if current_id == INVALID_SNAPSHOT_ID {
                continue;
            }
            if current_id >= reuse_limit {
                retained_records += 1;
                continue;
            }
            let to_overwrite = match valid_record {
                None => {
                    valid_record = Some(record);
                    retained_records += 1;
                    continue;
                }
                Some(valid) if current_id < valid.snapshot_id() => record,
                Some(valid) => {
                    valid_record = Some(record);
                    valid
                }
            };
            to_overwrite.set_snapshot_id(INVALID_SNAPSHOT_ID);
            retired.extend(to_overwrite.clear_value());
        }
        // A destructor may write this state: none runs while the chain is
        // borrowed.
        drop(records);
        for value in retired {
            retire_record_value(value);
        }
        retained_records > 1
    }

    // Clears the values of the records after `record`, the global snapshot's
    // record at its current id, that it hides from every open snapshot, and
    // marks them unused; their values go to `cleared`, for the caller to drop
    // once the write is published. A snapshot pinned at an id reads the
    // newest record below that id, so the newest record below the lowest pin
    // stays.
    pub(crate) fn clear_hidden_after(
        &self,
        record: &StateRecord,
        cleared: &mut SmallVec<[Box<dyn RecordValue>; 2]>,
    ) {
        let reuse_limit = lowest_pinned_snapshot();
        let record_hides_all = reuse_limit.is_none_or(|limit| record.snapshot_id() < limit);
        let records = self.records.borrow();
        let Some(at) = records
            .iter()
            .position(|link| std::ptr::eq(&**link, record))
        else {
            return;
        };
        let mut kept: Option<&Rc<StateRecord>> = None;
        for node in &records[at + 1..] {
            let id = node.snapshot_id();
            if node.is_tombstone()
                || id == PREEXISTING_SNAPSHOT_ID
                || reuse_limit.is_some_and(|limit| id >= limit)
            {
                continue;
            }
            let hidden = if record_hides_all {
                node
            } else {
                match kept {
                    None => {
                        kept = Some(node);
                        continue;
                    }
                    Some(newest) if newest.snapshot_id() < id => {
                        kept = Some(node);
                        newest
                    }
                    Some(_) => node,
                }
            };
            cleared.extend(hidden.clear_value());
            hidden.set_tombstone(true);
        }
    }

    // The id of each record, and whether it is a tombstone.
    pub(crate) fn ids(&self) -> Vec<(SnapshotId, bool)> {
        self.records
            .borrow()
            .iter()
            .map(|record| (record.snapshot_id(), record.is_tombstone()))
            .collect()
    }

    // Panics when the chain of the state `id` is empty or holds a record
    // twice.
    #[cold]
    #[inline(never)]
    pub(crate) fn check_integrity(
        &self,
        id: ObjectId,
        caller: &str,
        snapshot_context: Option<SnapshotId>,
    ) {
        let records = self.records.borrow();
        assert!(
            !records.is_empty(),
            "SnapshotMutableState::{caller} found no record for state {id:?} (snapshot_context={snapshot_context:?})"
        );
        let mut seen: HashSet<usize> = HashSet::default();
        let mut ids = Vec::with_capacity(records.len());
        for record in records.iter() {
            assert!(
                seen.insert(Rc::as_ptr(record) as usize),
                "SnapshotMutableState::{caller} detected duplicate record {:p} for state {id:?} (snapshot_context={snapshot_context:?}, chain_ids={ids:?})",
                Rc::as_ptr(record),
            );
            ids.push(record.snapshot_id());
        }
    }
}
