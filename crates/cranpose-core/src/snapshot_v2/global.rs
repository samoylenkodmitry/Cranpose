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
            state: SnapshotState::new_with_pinning(
                id,
                invalid,
                None,
                None,
                RuntimeTracking::Untracked,
                Pinning::Unpinned,
            ),
            nested_count: Cell::new(0),
        })
    }

    /// Get or create the global snapshot instance.
    pub fn get_or_create() -> Rc<Self> {
        crate::thread_state::snapshots(|state| {
            let mut snapshot = state.global.borrow_mut();
            if let Some(global) = snapshot.as_ref() {
                return Rc::clone(global);
            }

            let id = with_runtime(|runtime| runtime.global_snapshot_id());
            let invalid = runtime::open_snapshots();
            let global = Self::new(id, invalid);
            *snapshot = Some(Rc::clone(&global));
            global
        })
    }

    /// Advances to `new_id` and lets go of the records no snapshot reads
    /// any more, when the runtime is due to look for them.
    fn advance_releasing(&self, new_id: SnapshotId) {
        self.advance(new_id);
        maybe_check_and_overwrite_unused_records_locked(new_id);
    }

    /// Advances the global snapshot to `new_id`.
    ///
    /// A global write commits and notifies apply observers on its own, so the
    /// snapshot keeps no written state past the advance. A state its owner
    /// releases is then freed together with its value.
    pub fn advance(&self, new_id: SnapshotId) {
        let invalid = runtime::advance_global_snapshot(new_id);
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

    /// Returns the id of this snapshot. Ids grow with every snapshot taken.
    pub fn snapshot_id(&self) -> SnapshotId {
        self.state.id.get()
    }

    /// Returns the ids of snapshots whose writes this snapshot does not see.
    pub fn invalid(&self) -> SnapshotIdSet {
        self.state.invalid.borrow().clone()
    }

    /// Returns `false`: the global snapshot accepts writes.
    pub fn read_only(&self) -> bool {
        false
    }

    /// Runs `f` with this snapshot as the current snapshot of the thread and returns its result.
    /// The previous current snapshot is restored when `f` returns.
    pub fn enter<T>(&self, f: impl FnOnce() -> T) -> T {
        enter_snapshot_scope(AnySnapshot::Global(Self::get_or_create()), f)
    }

    /// Takes a read-only snapshot of the current global state. Reads inside it call
    /// `read_observer`.
    pub fn take_nested_snapshot(
        &self,
        read_observer: Option<ReadObserver>,
    ) -> Rc<ReadonlySnapshot> {
        // A global write replaces in place a value the global snapshot wrote
        // at its current id. The snapshot takes an id of its own and the
        // global moves past it, so what the global writes or creates from
        // here on stays out of the snapshot.
        let snapshot_id = allocate_record_id();
        let invalid = self.state.invalid.borrow().clear(self.state.id.get());
        self.advance_releasing(allocate_record_id());
        ReadonlySnapshot::new(snapshot_id, invalid, read_observer)
    }

    /// Returns `true` when a write was recorded and the global snapshot has not advanced since.
    pub fn has_pending_changes(&self) -> bool {
        !self.state.modified.borrow().is_empty()
    }

    /// Returns the ids of nested mutable snapshots taken from this snapshot and not yet disposed.
    pub fn pending_children(&self) -> Vec<SnapshotId> {
        self.state.pending_children()
    }

    /// Returns `true` when a nested mutable snapshot taken from this snapshot is still open.
    pub fn has_pending_children(&self) -> bool {
        self.state.has_pending_children()
    }

    /// Does nothing: the global snapshot is never disposed.
    pub fn dispose(&self) {}

    /// Calls the read observer, if any, with `state`.
    pub fn record_read(&self, state: &dyn StateObject) {
        self.state.record_read(state);
    }

    /// Records `state` as written in the global snapshot. The write observer, if any, is called the
    /// first time a state object is recorded.
    pub fn record_write(&self, state: Rc<dyn StateObject>) {
        self.state.record_write(state, self.state.id.get());
    }

    /// Returns `false`: the global snapshot is never disposed.
    pub fn is_disposed(&self) -> bool {
        false
    }

    /// Does nothing and returns `Success`: the global snapshot has nothing to apply.
    pub fn apply(&self) -> SnapshotApplyResult {
        SnapshotApplyResult::Success
    }

    /// Takes a mutable snapshot whose `apply` publishes its writes to the global state. The global
    /// snapshot moves to a new id, so the writes of the new snapshot stay isolated until then.
    pub fn take_nested_mutable_snapshot(
        &self,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
    ) -> Rc<MutableSnapshot> {
        let base_parent_id = self.state.id.get();

        let (new_id, child_invalid, new_global_invalid) =
            with_runtime(SnapshotRuntime::take_new_snapshot_advancing_global);

        let new_global_id = with_runtime(|runtime| runtime.global_snapshot_id());
        self.state.id.set(new_global_id);
        self.state.invalid.replace(new_global_invalid);

        let child = MutableSnapshot::from_parts(
            new_id,
            child_invalid,
            read_observer,
            write_observer,
            base_parent_id,
            RuntimeTracking::Tracked,
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

#[cfg(test)]
pub(crate) fn clear_global_snapshot_for_tests() {
    crate::thread_state::snapshots(|state| {
        *state.global.borrow_mut() = None;
    });
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
    GlobalSnapshot::get_or_create().advance_releasing(new_id);
}

#[cfg(test)]
pub(super) fn global_snapshot_id() -> SnapshotId {
    let global = GlobalSnapshot::get_or_create();
    global.snapshot_id()
}

#[cfg(test)]
#[path = "tests/global_tests.rs"]
mod tests;
