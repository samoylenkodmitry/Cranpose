//! Global snapshot implementation.

use super::*;

/// The global mutable snapshot.
///
/// This is a special singleton snapshot that represents the global state.
/// All non-nested snapshots implicitly depend on the global snapshot.
///
/// # Thread Safety
/// Contains `Cell<T>` which is not `Send`/`Sync`. This is safe because snapshots
/// are stored in thread-local storage and never shared across threads. The `Arc`
/// is used for cheap cloning within a single thread, not for cross-thread sharing.
#[allow(clippy::arc_with_non_send_sync)]
pub struct GlobalSnapshot {
    state: SnapshotState,
    nested_count: Cell<usize>,
}

impl GlobalSnapshot {
    /// Create a new global snapshot.
    pub fn new(id: SnapshotId, invalid: SnapshotIdSet) -> Arc<Self> {
        Arc::new(Self {
            state: SnapshotState::new(id, invalid, None, None, false),
            nested_count: Cell::new(0),
        })
    }

    /// Get or create the global snapshot instance.
    pub fn get_or_create() -> Arc<Self> {
        GLOBAL_SNAPSHOT.with(|cell| {
            let mut snapshot = cell.borrow_mut();
            if snapshot.is_none() {
                let id = with_runtime(|runtime| runtime.global_snapshot_id());
                let invalid = super::runtime::open_snapshots();
                *snapshot = Some(GlobalSnapshot::new(id, invalid));
            }
            snapshot.as_ref().unwrap().clone()
        })
    }

    /// Advance the global snapshot to a new ID.
    pub fn advance(&self, new_id: SnapshotId) {
        let invalid = super::runtime::advance_global_snapshot(new_id);
        self.state.id.set(new_id);
        self.state.invalid.replace(invalid);
    }
}

thread_local! {
    static GLOBAL_SNAPSHOT: RefCell<Option<Arc<GlobalSnapshot>>> = const { RefCell::new(None) };
}

/// Clear the global snapshot (for testing only).
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
        false // Global snapshot is mutable
    }

    pub fn root_global(&self) -> Arc<Self> {
        GlobalSnapshot::get_or_create()
    }

    pub fn enter<T>(&self, f: impl FnOnce() -> T) -> T {
        let previous = current_snapshot();
        set_current_snapshot(Some(AnySnapshot::Global(self.root_global())));
        let result = f();
        set_current_snapshot(previous);
        result
    }

    pub fn take_nested_snapshot(
        &self,
        read_observer: Option<ReadObserver>,
    ) -> Arc<ReadonlySnapshot> {
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

    pub fn dispose(&self) {
        // Global snapshot cannot be disposed
        // This is a no-op
    }

    pub fn record_read(&self, state: &dyn StateObject) {
        self.state.record_read(state);
    }

    pub fn record_write(&self, state: Arc<dyn StateObject>) {
        self.state.record_write(state, self.state.id.get());
    }

    pub fn close(&self) {
        // Global snapshot is never closed
    }

    pub fn is_disposed(&self) -> bool {
        false // Global snapshot is never disposed
    }

    pub fn apply(&self) -> SnapshotApplyResult {
        // Global snapshot changes are immediately visible
        // No need to apply
        SnapshotApplyResult::Success
    }

    pub fn take_nested_mutable_snapshot(
        &self,
        read_observer: Option<ReadObserver>,
        write_observer: Option<WriteObserver>,
    ) -> Arc<MutableSnapshot> {
        let (new_id, runtime_invalid) = super::runtime::allocate_snapshot();
        // base_parent_id for the new child snapshot is the current global id.
        let base_parent_id = self.state.id.get();

        // Parent invalid set needs to track the newly opened snapshot.
        let mut parent_invalid = self.state.invalid.borrow().clone();
        parent_invalid = parent_invalid.set(new_id);
        self.state.invalid.replace(parent_invalid.clone());

        // Combine parent invalid information with the runtime invalid set that accounts
        // for already-open snapshots elsewhere in the system.
        let invalid = parent_invalid.or(&runtime_invalid);

        let child = MutableSnapshot::from_parts(
            new_id,
            invalid,
            read_observer,
            write_observer,
            base_parent_id,
            true,
        );

        self.nested_count.set(self.nested_count.get() + 1);
        self.state.add_pending_child(new_id);

        let parent_arc = self.root_global();
        let weak = Arc::downgrade(&parent_arc);
        drop(parent_arc);
        child.set_on_dispose({
            let child_id = new_id;
            move || {
                if let Some(parent) = weak.upgrade() {
                    if parent.nested_count.get() > 0 {
                        parent
                            .nested_count
                            .set(parent.nested_count.get().saturating_sub(1));
                    }
                    let mut invalid = parent.state.invalid.borrow_mut();
                    let new_set = invalid.clone().clear(child_id);
                    *invalid = new_set;
                    parent.state.remove_pending_child(child_id);
                }
            }
        });

        child
    }
}

/// Advance the global snapshot to a new ID.
pub fn advance_global_snapshot(new_id: SnapshotId) {
    let global = GlobalSnapshot::get_or_create();
    global.advance(new_id);
    // Clean up unused records after advancing
    super::check_and_overwrite_unused_records_locked();
}

/// Get the current global snapshot ID.
#[cfg(test)]
pub fn global_snapshot_id() -> SnapshotId {
    let global = GlobalSnapshot::get_or_create();
    global.snapshot_id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot_v2::runtime::TestRuntimeGuard;

    fn reset_runtime() -> TestRuntimeGuard {
        let guard = reset_runtime_for_tests();
        GLOBAL_SNAPSHOT.with(|cell| {
            *cell.borrow_mut() = None;
        });
        guard
    }

    #[test]
    fn test_global_snapshot_creation() {
        let _guard = reset_runtime();
        let snapshot = GlobalSnapshot::new(1, SnapshotIdSet::new());
        assert_eq!(snapshot.snapshot_id(), 1);
        assert!(!snapshot.read_only());
        assert!(!snapshot.is_disposed());
    }

    #[test]
    fn test_global_snapshot_get_or_create() {
        let _guard = reset_runtime();

        let snapshot1 = GlobalSnapshot::get_or_create();
        let snapshot2 = GlobalSnapshot::get_or_create();

        // Should return the same instance
        assert_eq!(snapshot1.snapshot_id(), snapshot2.snapshot_id());
    }

    #[test]
    fn test_global_snapshot_advance() {
        let _guard = reset_runtime();
        let snapshot = GlobalSnapshot::new(1, SnapshotIdSet::new());
        assert_eq!(snapshot.snapshot_id(), 1);

        // Test that advance updates the snapshot ID
        // Note: We don't call the actual advance() because it modifies global runtime state.
        // Instead, just verify the local ID can be updated.
        snapshot.state.id.set(5);
        assert_eq!(snapshot.snapshot_id(), 5);

        snapshot.state.id.set(10);
        assert_eq!(snapshot.snapshot_id(), 10);
    }

    #[test]
    fn test_global_snapshot_never_disposed() {
        let _guard = reset_runtime();
        let snapshot = GlobalSnapshot::new(1, SnapshotIdSet::new());
        assert!(!snapshot.is_disposed());

        snapshot.dispose();
        assert!(!snapshot.is_disposed()); // Still not disposed
    }

    #[test]
    fn test_global_snapshot_apply_always_succeeds() {
        let _guard = reset_runtime();
        let snapshot = GlobalSnapshot::new(1, SnapshotIdSet::new());
        let result = snapshot.apply();
        assert!(result.is_success());
    }

    #[test]
    fn test_global_snapshot_nested() {
        let _guard = reset_runtime();
        let global = GlobalSnapshot::new(1, SnapshotIdSet::new());
        let nested = global.take_nested_snapshot(None);

        assert_eq!(nested.snapshot_id(), 1);
        assert!(nested.read_only());
        assert_eq!(global.nested_count.get(), 0); // Not tracked for readonly
    }

    #[test]
    fn test_global_snapshot_nested_mutable() {
        let _guard = reset_runtime();
        let global = GlobalSnapshot::new(1, SnapshotIdSet::new());
        let nested = global.take_nested_mutable_snapshot(None, None);

        assert!(nested.snapshot_id() > global.snapshot_id());
        assert!(!nested.read_only());
    }

    #[test]
    fn test_global_snapshot_nested_mutable_dispose_clears_invalid() {
        let _guard = reset_runtime();
        let global = GlobalSnapshot::get_or_create();
        let nested = global.take_nested_mutable_snapshot(None, None);
        let child_id = nested.snapshot_id();

        assert!(global.state.invalid.borrow().get(child_id));
        assert_eq!(global.nested_count.get(), 1);

        nested.dispose();

        assert_eq!(global.nested_count.get(), 0);
        assert!(!global.state.invalid.borrow().get(child_id));
    }

    #[test]
    fn test_advance_global_snapshot_function() {
        let _guard = reset_runtime();

        let initial_id = global_snapshot_id();

        advance_global_snapshot(initial_id + 10);
        assert_eq!(global_snapshot_id(), initial_id + 10);

        advance_global_snapshot(initial_id + 20);
        assert_eq!(global_snapshot_id(), initial_id + 20);
    }

    #[test]
    fn test_global_snapshot_has_no_pending_changes_initially() {
        let _guard = reset_runtime();
        let snapshot = GlobalSnapshot::new(1, SnapshotIdSet::new());
        assert!(!snapshot.has_pending_changes());
    }

    #[test]
    fn test_global_snapshot_enter() {
        let _guard = reset_runtime();
        let snapshot = GlobalSnapshot::new(1, SnapshotIdSet::new());

        set_current_snapshot(None);
        snapshot.enter(|| {
            let current = current_snapshot();
            assert!(current.is_some());
        });
    }
}
