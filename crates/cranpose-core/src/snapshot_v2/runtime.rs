use std::cell::{Cell, RefCell};

use super::*;

const PREEXISTING_SNAPSHOT_ID: SnapshotId = 1;

const INITIAL_GLOBAL_SNAPSHOT_ID: SnapshotId = PREEXISTING_SNAPSHOT_ID + 1;

thread_local! {
    static SNAPSHOT_RUNTIME: RefCell<Option<SnapshotRuntime>> = const { RefCell::new(None) };
}

thread_local! {
    static RUNTIME_LOCK_DEPTH: Cell<usize> = const { Cell::new(0) };
}

struct RuntimeLockGuard;

impl RuntimeLockGuard {
    fn enter() -> Self {
        RUNTIME_LOCK_DEPTH.with(|cell| cell.set(cell.get() + 1));
        Self
    }
}

impl Drop for RuntimeLockGuard {
    fn drop(&mut self) {
        RUNTIME_LOCK_DEPTH.with(|cell| {
            let depth = cell.get();
            debug_assert!(depth > 0, "runtime lock depth underflow");
            cell.set(depth.saturating_sub(1));
        });
    }
}

pub(crate) fn with_runtime<T>(f: impl FnOnce(&mut SnapshotRuntime) -> T) -> T {
    let _scope = RuntimeLockGuard::enter();
    SNAPSHOT_RUNTIME.with(|runtime_cell| {
        let mut runtime = runtime_cell.borrow_mut();
        f(runtime.get_or_insert_with(SnapshotRuntime::new))
    })
}

#[cfg(test)]
pub(crate) fn runtime_lock_depth() -> usize {
    RUNTIME_LOCK_DEPTH.with(|cell| cell.get())
}

pub(crate) fn allocate_snapshot() -> (SnapshotId, SnapshotIdSet) {
    with_runtime(|runtime| runtime.allocate_snapshot())
}

pub(crate) fn close_snapshot(id: SnapshotId) {
    with_runtime(|runtime| runtime.close_snapshot(id))
}

pub(crate) fn allocate_record_id() -> SnapshotId {
    with_runtime(|runtime| runtime.allocate_record_id())
}

pub(crate) fn peek_next_snapshot_id() -> SnapshotId {
    with_runtime(|runtime| runtime.peek_next_snapshot_id())
}

pub(crate) fn advance_global_snapshot(new_id: SnapshotId) -> SnapshotIdSet {
    with_runtime(|runtime| runtime.advance_global_snapshot(new_id))
}

pub(crate) fn open_snapshots() -> SnapshotIdSet {
    with_runtime(|runtime| runtime.open_snapshots())
}

#[cfg(test)]
pub(crate) struct TestRuntimeGuard;

#[cfg(test)]
pub(crate) fn reset_runtime_for_tests() -> TestRuntimeGuard {
    with_runtime(|runtime| runtime.reset_for_tests());
    super::clear_last_writes();
    super::global::clear_global_snapshot_for_tests();
    super::clear_unused_record_cleanup_for_tests();
    TestRuntimeGuard
}

#[derive(Debug)]
pub(crate) struct SnapshotRuntime {
    next_snapshot_id: SnapshotId,
    open_snapshots: SnapshotIdSet,
    global_snapshot_id: SnapshotId,
}

impl SnapshotRuntime {
    fn new() -> Self {
        let mut open = SnapshotIdSet::new();
        open = open.set(INITIAL_GLOBAL_SNAPSHOT_ID);
        Self {
            next_snapshot_id: INITIAL_GLOBAL_SNAPSHOT_ID + 1,
            open_snapshots: open,
            global_snapshot_id: INITIAL_GLOBAL_SNAPSHOT_ID,
        }
    }

    pub(crate) fn global_snapshot_id(&self) -> SnapshotId {
        self.global_snapshot_id
    }

    pub(crate) fn open_snapshots(&self) -> SnapshotIdSet {
        self.open_snapshots.clone()
    }

    pub(crate) fn advance_global_snapshot(&mut self, new_id: SnapshotId) -> SnapshotIdSet {
        let old_id = self.global_snapshot_id;
        if new_id <= old_id {
            let mut open = SnapshotIdSet::new();
            open = open.set(new_id);
            self.open_snapshots = open;
            self.global_snapshot_id = new_id;
            return self.open_snapshots.clone();
        }

        self.open_snapshots = self.open_snapshots.clear(old_id);
        self.open_snapshots = self.open_snapshots.set(new_id);
        self.global_snapshot_id = new_id;
        self.open_snapshots.clone()
    }

    pub(crate) fn allocate_snapshot(&mut self) -> (SnapshotId, SnapshotIdSet) {
        let invalid = self.open_snapshots.clone();
        let id = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        self.open_snapshots = self.open_snapshots.set(id);
        (id, invalid)
    }

    pub(crate) fn close_snapshot(&mut self, id: SnapshotId) {
        self.open_snapshots = self.open_snapshots.clear(id);
    }

    pub(crate) fn allocate_record_id(&mut self) -> SnapshotId {
        let id = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        id
    }

    pub(crate) fn take_new_snapshot_advancing_global(
        &mut self,
    ) -> (SnapshotId, SnapshotIdSet, SnapshotIdSet) {
        let old_global_id = self.global_snapshot_id;

        let child_invalid = self.open_snapshots.clear(old_global_id);

        let child_id = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        self.open_snapshots = self.open_snapshots.set(child_id);

        let new_global_id = self.next_snapshot_id;
        self.next_snapshot_id += 1;

        self.open_snapshots = self.open_snapshots.clear(old_global_id);

        self.global_snapshot_id = new_global_id;

        let new_global_invalid = self.open_snapshots.clone();

        self.open_snapshots = self.open_snapshots.set(new_global_id);

        (child_id, child_invalid, new_global_invalid)
    }

    pub(crate) fn peek_next_snapshot_id(&self) -> SnapshotId {
        self.next_snapshot_id
    }

    #[cfg(test)]
    pub(crate) fn reset_for_tests(&mut self) {
        *self = SnapshotRuntime::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state_marks_global_snapshot_open() {
        let _guard = reset_runtime_for_tests();
        with_runtime(|runtime| {
            assert_eq!(runtime.global_snapshot_id(), INITIAL_GLOBAL_SNAPSHOT_ID);
            assert!(runtime.open_snapshots().get(INITIAL_GLOBAL_SNAPSHOT_ID));
        });
    }

    #[test]
    fn test_allocate_snapshot_marks_it_open() {
        let _guard = reset_runtime_for_tests();
        let (id, invalid) = allocate_snapshot();
        assert!(invalid.get(INITIAL_GLOBAL_SNAPSHOT_ID));
        assert!(!invalid.get(id));
        with_runtime(|runtime| {
            assert!(runtime.open_snapshots().get(id));
        });
    }

    #[test]
    fn test_close_snapshot_clears_open_flag() {
        let _guard = reset_runtime_for_tests();
        let (id, _) = allocate_snapshot();
        with_runtime(|runtime| {
            assert!(runtime.open_snapshots().get(id));
        });
        close_snapshot(id);
        with_runtime(|runtime| {
            assert!(!runtime.open_snapshots().get(id));
        });
    }

    #[test]
    fn test_advance_global_snapshot_updates_open_set() {
        let _guard = reset_runtime_for_tests();
        let new_id = INITIAL_GLOBAL_SNAPSHOT_ID + 1;
        let open = advance_global_snapshot(new_id);
        assert!(open.get(new_id));
        assert!(!open.get(INITIAL_GLOBAL_SNAPSHOT_ID));
        with_runtime(|runtime| {
            assert_eq!(runtime.global_snapshot_id(), new_id);
            assert!(runtime.open_snapshots().get(new_id));
        });
    }
}
