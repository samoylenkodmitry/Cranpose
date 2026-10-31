use std::{any::Any, cell::Cell, rc::Rc};

use smallvec::smallvec;

use crate::{
    snapshot_id_set::{SnapshotId, SnapshotIdSet},
    state::{ObjectId, PREEXISTING_SNAPSHOT_ID, RecordChain, StateObject, StateRecord},
};

pub(crate) struct MockState {
    id: Cell<ObjectId>,
    value: i32,
    records: RecordChain,
    readable: bool,
}

impl MockState {
    pub(crate) fn with_id(id: usize) -> Self {
        Self {
            id: Cell::new(ObjectId(id)),
            value: 0,
            records: RecordChain::new(smallvec![StateRecord::new(PREEXISTING_SNAPSHOT_ID, ())]),
            readable: true,
        }
    }

    pub(crate) fn unreadable(id: usize, writer: SnapshotId) -> Self {
        Self {
            records: RecordChain::new(smallvec![StateRecord::new(writer, ())]),
            readable: false,
            ..Self::with_id(id)
        }
    }

    pub(crate) fn tracked(value: i32) -> Rc<Self> {
        let state = Rc::new(Self {
            value,
            records: RecordChain::new(smallvec![StateRecord::new(SnapshotId(1), value)]),
            ..Self::with_id(0)
        });
        state.id.set(ObjectId::new(&state));
        state
    }

    pub(crate) fn value(&self) -> i32 {
        self.value
    }
}

impl StateObject for MockState {
    fn object_id(&self) -> ObjectId {
        self.id.get()
    }

    fn records(&self) -> &RecordChain {
        &self.records
    }

    fn try_readable_record(&self, _: SnapshotId, _: &SnapshotIdSet) -> Option<Rc<StateRecord>> {
        self.readable.then(|| self.records.head())
    }

    fn readable_record(&self, snapshot_id: SnapshotId, invalid: &SnapshotIdSet) -> Rc<StateRecord> {
        self.try_readable_record(snapshot_id, invalid)
            .expect("apply must use a fallible parent readable-record lookup")
    }

    fn promote_record(&self, _child_id: SnapshotId) -> Result<(), &'static str> {
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
