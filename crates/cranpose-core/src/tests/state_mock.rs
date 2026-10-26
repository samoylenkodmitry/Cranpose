use std::{
    any::Any,
    cell::{Cell, RefCell},
    rc::Rc,
};

use crate::{
    snapshot_id_set::{SnapshotId, SnapshotIdSet},
    state::{ObjectId, PREEXISTING_SNAPSHOT_ID, StateObject, StateRecord},
};

pub(crate) struct MockState {
    id: Cell<ObjectId>,
    value: i32,
    head: RefCell<Rc<StateRecord>>,
    readable: bool,
}

impl MockState {
    pub(crate) fn with_id(id: usize) -> Self {
        Self {
            id: Cell::new(ObjectId(id)),
            value: 0,
            head: RefCell::new(StateRecord::new(PREEXISTING_SNAPSHOT_ID, (), None)),
            readable: true,
        }
    }

    pub(crate) fn unreadable(id: usize, writer: SnapshotId) -> Self {
        Self {
            head: RefCell::new(StateRecord::new(writer, (), None)),
            readable: false,
            ..Self::with_id(id)
        }
    }

    pub(crate) fn tracked(value: i32) -> Rc<Self> {
        let state = Rc::new(Self {
            value,
            head: RefCell::new(StateRecord::new(SnapshotId(1), value, None)),
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

    fn first_record(&self) -> Rc<StateRecord> {
        Rc::clone(&self.head.borrow())
    }

    fn try_readable_record(&self, _: SnapshotId, _: &SnapshotIdSet) -> Option<Rc<StateRecord>> {
        self.readable.then(|| self.first_record())
    }

    fn readable_record(&self, snapshot_id: SnapshotId, invalid: &SnapshotIdSet) -> Rc<StateRecord> {
        self.try_readable_record(snapshot_id, invalid)
            .expect("apply must use a fallible parent readable-record lookup")
    }

    fn prepend_state_record(&self, record: Rc<StateRecord>) {
        *self.head.borrow_mut() = record;
    }

    fn promote_record(&self, _child_id: SnapshotId) -> Result<(), &'static str> {
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
