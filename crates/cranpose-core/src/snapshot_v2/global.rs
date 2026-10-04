use super::*;

/// The global mutable snapshot.
///
/// This is a special singleton snapshot that represents the global state.
/// All non-nested snapshots implicitly depend on the global snapshot.
///
/// # Thread Safety
/// Contains `Cell<T>` which is not `Send`/`Sync`. This is safe because snapshots
/// are stored in thread-local storage and never shared across threads. The `Rc`
/// is used for cheap cloning within a single thread, not for cross-thread sharing.
pub struct GlobalSnapshot {
    pub(super) state: SnapshotState,
    nested_count: Cell<usize>,
}

impl GlobalSnapshot {
    /// Create a new global snapshot.
    ///
    /// The global snapshot does NOT pin because it always represents the current state
    /// and reads the latest records. Pinning would prevent garbage collection.
    pub fn new(id: SnapshotId, invalid: SnapshotIdSet) -> Rc<Self> {
        Rc::new(Self {
            state: SnapshotState::new_with_pinning(id, invalid, None, None, false, false),
            nested_count: Cell::new(0),
        })
    }

    /// Get or create the global snapshot instance.
    pub fn get_or_create() -> Rc<Self> {
        GLOBAL_SNAPSHOT.with(|cell| {
            let mut snapshot = cell.borrow_mut();
            if let Some(global) = snapshot.as_ref() {
                return Rc::clone(global);
            }

            let id = with_runtime(|runtime| runtime.global_snapshot_id());
            let invalid = super::runtime::open_snapshots();
            let global = GlobalSnapshot::new(id, invalid);
            *snapshot = Some(Rc::clone(&global));
            global
        })
    }

    /// Advances the global snapshot to `new_id`.
    ///
    /// A global write commits and notifies apply observers on its own, so the
    /// snapshot keeps no written state past the advance. A state its owner
    /// releases is then freed together with its value.
    pub fn advance(&self, new_id: SnapshotId) {
        let invalid = super::runtime::advance_global_snapshot(new_id);
        self.state.id.set(new_id);
        self.state.invalid.replace(invalid);
        // Drop the recorded states outside the borrow, then hand the emptied
        // map back so the next write reuses its capacity.
        let mut modified = self.state.modified.take();
        modified.clear();
        let mut slot = self.state.modified.borrow_mut();
        if slot.is_empty() {
            std::mem::swap(&mut *slot, &mut modified);
        }
    }
}

thread_local! {
    static GLOBAL_SNAPSHOT: RefCell<Option<Rc<GlobalSnapshot>>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn clear_global_snapshot_for_tests() {
    GLOBAL_SNAPSHOT.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

impl GlobalSnapshot {
    pub fn snapshot_id(&self) -> SnapshotId {
        self.state.id.get()
    }

    pub fn invalid(&self) -> SnapshotIdSet {
        self.state.invalid.borrow().clone()
    }

    pub fn read_only(&self) -> bool {
        false
    }

    pub fn enter<T>(&self, f: impl FnOnce() -> T) -> T {
        enter_snapshot_scope(AnySnapshot::Global(Self::get_or_create()), f)
    }

    pub fn take_nested_snapshot(
        &self,
        read_observer: Option<ReadObserver>,
    ) -> Rc<ReadonlySnapshot> {
        ReadonlySnapshot::new(
            self.state.id.get(),
            self.state.invalid.borrow().clone(),
            read_observer,
        )
    }

    pub fn has_pending_changes(&self) -> bool {
        !self.state.modified.borrow().is_empty()
    }

    pub fn pending_children(&self) -> Vec<SnapshotId> {
        self.state.pending_children()
    }

    pub fn has_pending_children(&self) -> bool {
        self.state.has_pending_children()
    }

    pub fn dispose(&self) {}

    pub fn record_read(&self, state: &dyn StateObject) {
        self.state.record_read(state);
    }

    pub fn record_write(&self, state: Rc<dyn StateObject>) {
        self.state.record_write(state, self.state.id.get());
    }

    pub fn close(&self) {}

    pub fn is_disposed(&self) -> bool {
        false
    }

    pub fn apply(&self) -> SnapshotApplyResult {
        SnapshotApplyResult::Success
    }

    pub fn take_nested_mutable_snapshot(
        &self,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
    ) -> Rc<MutableSnapshot> {
        let base_parent_id = self.state.id.get();

        let (new_id, child_invalid, new_global_invalid) = super::runtime::with_runtime(
            super::runtime::SnapshotRuntime::take_new_snapshot_advancing_global,
        );

        let new_global_id = super::runtime::with_runtime(|runtime| runtime.global_snapshot_id());
        self.state.id.set(new_global_id);
        self.state.invalid.replace(new_global_invalid);

        let child = MutableSnapshot::from_parts(
            new_id,
            child_invalid,
            read_observer,
            write_observer,
            base_parent_id,
            true,
        );

        self.nested_count.set(self.nested_count.get() + 1);
        self.state.add_pending_child(new_id);

        child.set_on_dispose(clear_nested_child_on_dispose(
            &Self::get_or_create(),
            new_id,
        ));

        child
    }
}

impl NestedMutableHost for GlobalSnapshot {
    fn snapshot_state(&self) -> &SnapshotState {
        &self.state
    }

    fn nested_count(&self) -> &Cell<usize> {
        &self.nested_count
    }
}

/// Advance the global snapshot to a new ID.
pub fn advance_global_snapshot(new_id: SnapshotId) {
    let global = GlobalSnapshot::get_or_create();
    global.advance(new_id);
    super::maybe_check_and_overwrite_unused_records_locked(new_id);
}

#[cfg(test)]
pub fn global_snapshot_id() -> SnapshotId {
    let global = GlobalSnapshot::get_or_create();
    global.snapshot_id()
}

#[cfg(test)]
#[path = "tests/global_tests.rs"]
mod tests;
