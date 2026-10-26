use super::*;

pub(crate) struct SnapshotState {
    pub(crate) id: Cell<SnapshotId>,
    pub(crate) invalid: RefCell<SnapshotIdSet>,
    pub(crate) pin_handle: Cell<PinHandle>,
    pub(crate) disposed: Cell<bool>,
    pub(crate) read_observer: RefCell<Option<ReadObserver>>,
    pub(crate) write_observer: RefCell<Option<WriteObserver>>,
    #[expect(clippy::type_complexity, reason = "modified states by id")]
    pub(crate) modified: RefCell<HashMap<ObjectId, (Rc<dyn StateObject>, SnapshotId)>>,
    on_dispose: RefCell<Option<Box<dyn FnOnce()>>>,
    runtime_tracking: RuntimeTracking,
    pending_children: RefCell<HashSet<SnapshotId>>,
}

// Whether the runtime counts a snapshot as open, so that disposing it closes
// it there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeTracking {
    Tracked,
    Untracked,
}

// Whether a snapshot pins the records it reads until it is disposed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pinning {
    Pinned,
    Unpinned,
}

impl SnapshotState {
    pub(crate) fn new(
        id: SnapshotId,
        invalid: SnapshotIdSet,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
        runtime_tracking: RuntimeTracking,
    ) -> Self {
        Self::new_with_pinning(
            id,
            invalid,
            read_observer,
            write_observer,
            runtime_tracking,
            Pinning::Pinned,
        )
    }

    pub(crate) fn new_with_pinning(
        id: SnapshotId,
        invalid: SnapshotIdSet,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
        runtime_tracking: RuntimeTracking,
        pinning: Pinning,
    ) -> Self {
        let pin_handle = if pinning == Pinning::Pinned {
            snapshot_pinning::track_pinning(id, &invalid)
        } else {
            PinHandle::INVALID
        };
        Self {
            id: Cell::new(id),
            invalid: RefCell::new(invalid),
            pin_handle: Cell::new(pin_handle),
            disposed: Cell::new(false),
            read_observer: RefCell::new(read_observer),
            write_observer: RefCell::new(write_observer),
            modified: RefCell::new(HashMap::default()),
            on_dispose: RefCell::new(None),
            runtime_tracking,
            pending_children: RefCell::new(HashSet::default()),
        }
    }

    pub(crate) fn record_read(&self, state: &dyn StateObject) {
        if let Some(observer) = self.read_observer.borrow().as_ref() {
            observer(state);
        }
    }

    pub(crate) fn record_write(&self, state: Rc<dyn StateObject>, writer_id: SnapshotId) {
        let state_id = state.object_id();

        let mut modified = self.modified.borrow_mut();

        match modified.entry(state_id) {
            std::collections::hash_map::Entry::Vacant(e) => {
                if let Some(observer) = self.write_observer.borrow().as_ref() {
                    observer(&*state);
                }
                e.insert((state, writer_id));
            }
            std::collections::hash_map::Entry::Occupied(mut e) => {
                e.insert((state, writer_id));
            }
        }
    }

    pub(crate) fn dispose(&self) {
        if !self.disposed.replace(true) {
            let pin_handle = self.pin_handle.get();
            snapshot_pinning::release_pinning(pin_handle);
            if let Some(cb) = self.on_dispose.borrow_mut().take() {
                cb();
            }
            if self.runtime_tracking == RuntimeTracking::Tracked {
                close_snapshot(self.id.get());
            }
        }
    }

    pub(crate) fn add_pending_child(&self, id: SnapshotId) {
        self.pending_children.borrow_mut().insert(id);
    }

    pub(crate) fn remove_pending_child(&self, id: SnapshotId) {
        self.pending_children.borrow_mut().remove(&id);
    }

    pub(crate) fn has_pending_children(&self) -> bool {
        !self.pending_children.borrow().is_empty()
    }

    pub(crate) fn pending_children(&self) -> Vec<SnapshotId> {
        self.pending_children.borrow().iter().copied().collect()
    }

    pub(crate) fn set_on_dispose<F>(&self, f: F)
    where
        F: FnOnce() + 'static,
    {
        *self.on_dispose.borrow_mut() = Some(Box::new(f));
    }
}
