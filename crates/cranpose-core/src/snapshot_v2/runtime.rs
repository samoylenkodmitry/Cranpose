use std::cell::Cell;

use super::*;

const PREEXISTING_SNAPSHOT_ID: SnapshotId = 1;

const INITIAL_GLOBAL_SNAPSHOT_ID: SnapshotId = PREEXISTING_SNAPSHOT_ID + 1;

struct RuntimeLockGuard<'a>(&'a Cell<usize>);

impl<'a> RuntimeLockGuard<'a> {
    fn enter(depth: &'a Cell<usize>) -> Self {
        depth.set(depth.get() + 1);
        Self(depth)
    }
}

impl Drop for RuntimeLockGuard<'_> {
    fn drop(&mut self) {
        let depth = self.0.get();
        debug_assert!(depth > 0, "runtime lock depth underflow");
        self.0.set(depth.saturating_sub(1));
    }
}

pub(crate) fn with_runtime<T>(f: impl FnOnce(&mut SnapshotRuntime) -> T) -> T {
    crate::thread_state::snapshots(|state| {
        let _scope = RuntimeLockGuard::enter(&state.runtime_lock_depth);
        let mut runtime = state.runtime.borrow_mut();
        f(runtime.get_or_insert_with(|| {
            crate::thread_state::watch_thread_exit();
            SnapshotRuntime::new()
        }))
    })
}

#[cfg(test)]
pub(crate) fn runtime_lock_depth() -> usize {
    crate::thread_state::snapshots(|state| state.runtime_lock_depth.get())
}

pub(crate) fn allocate_snapshot() -> (SnapshotId, SnapshotIdSet) {
    with_runtime(SnapshotRuntime::allocate_snapshot)
}

pub(crate) fn close_snapshot(id: SnapshotId) {
    with_runtime(|runtime| runtime.close_snapshot(id));
}

pub(crate) fn allocate_record_id() -> SnapshotId {
    with_runtime(SnapshotRuntime::allocate_record_id)
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
    with_runtime(SnapshotRuntime::reset_for_tests);
    clear_last_writes();
    global::clear_global_snapshot_for_tests();
    clear_unused_record_cleanup_for_tests();
    TestRuntimeGuard
}

#[derive(Debug)]
pub(crate) struct SnapshotRuntime {
    next_snapshot_id: SnapshotId,
    open_snapshots: SnapshotIdSet,
    global_snapshot_id: SnapshotId,
}

impl SnapshotRuntime {
    pub(crate) fn new() -> Self {
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
        *self = Self::new();
    }
}

#[cfg(test)]
#[path = "tests/runtime_tests.rs"]
mod tests;
