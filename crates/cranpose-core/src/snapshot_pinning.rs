use std::cell::RefCell;

use crate::{
    snapshot_double_index_heap::{SnapshotDoubleIndexHeap, SnapshotDoubleIndexHeapDebugStats},
    snapshot_id_set::{SnapshotId, SnapshotIdSet},
};

/// A handle to a pinned snapshot. Dropping this handle releases the pin.
///
/// Internally stores a heap handle for O(log N) removal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PinHandle(usize);

impl PinHandle {
    /// Invalid pin handle constant (0 is reserved as invalid).
    pub const INVALID: PinHandle = PinHandle(0);

    /// Check if this handle is valid (non-zero).
    pub fn is_valid(&self) -> bool {
        self.0 != 0
    }
}

struct PinningTable {
    heap: SnapshotDoubleIndexHeap,
}

impl PinningTable {
    fn new() -> Self {
        Self {
            heap: SnapshotDoubleIndexHeap::new(),
        }
    }

    fn add(&mut self, snapshot_id: SnapshotId) -> PinHandle {
        let heap_handle = self.heap.add(snapshot_id);
        PinHandle(heap_handle + 1)
    }

    fn remove(&mut self, handle: PinHandle) -> bool {
        if !handle.is_valid() {
            return false;
        }

        let heap_handle = handle.0 - 1;

        if heap_handle < usize::MAX {
            self.heap.remove(heap_handle);
            true
        } else {
            false
        }
    }

    fn lowest_pinned(&self) -> Option<SnapshotId> {
        if self.heap.is_empty() {
            None
        } else {
            Some(self.heap.lowest_or_default(0))
        }
    }

    fn pin_count(&self) -> usize {
        self.heap.len()
    }

    fn debug_stats(&self) -> SnapshotPinningDebugStats {
        SnapshotPinningDebugStats {
            pin_count: self.pin_count(),
            lowest_pinned_snapshot: self.lowest_pinned(),
            heap: self.heap.debug_stats(),
        }
    }
}

thread_local! {
    static PINNING_TABLE: RefCell<PinningTable> = RefCell::new(PinningTable::new());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SnapshotPinningDebugStats {
    pub pin_count: usize,
    pub lowest_pinned_snapshot: Option<SnapshotId>,
    pub heap: SnapshotDoubleIndexHeapDebugStats,
}

/// Pin a snapshot and its invalid set, returning a handle.
///
/// This should be called when a snapshot is created to ensure that state records
/// from the pinned snapshot and all its dependencies remain valid.
///
/// # Arguments
/// * `snapshot_id` - The ID of the snapshot being created
/// * `invalid` - The set of invalid snapshot IDs for this snapshot
///
/// # Returns
/// A pin handle that should be released when the snapshot is disposed.
///
/// # Time Complexity
/// O(log N) where N is the number of pinned snapshots
pub fn track_pinning(snapshot_id: SnapshotId, invalid: &SnapshotIdSet) -> PinHandle {
    let pinned_id = invalid.lowest(snapshot_id);

    PINNING_TABLE.with(|cell| cell.borrow_mut().add(pinned_id))
}

/// Release a pinned snapshot.
///
/// # Arguments
/// * `handle` - The pin handle returned by `track_pinning`
///
/// This must be called while holding the appropriate lock (sync).
///
/// # Time Complexity
/// O(log N) where N is the number of pinned snapshots
pub fn release_pinning(handle: PinHandle) {
    if !handle.is_valid() {
        return;
    }

    PINNING_TABLE.with(|cell| {
        cell.borrow_mut().remove(handle);
    });
}

/// Get the lowest currently pinned snapshot ID.
///
/// This is used to determine which state records can be safely garbage collected.
/// Any state records from snapshots older than this ID are still potentially in use.
///
/// # Time Complexity
/// O(1)
pub fn lowest_pinned_snapshot() -> Option<SnapshotId> {
    PINNING_TABLE.with(|cell| cell.borrow().lowest_pinned())
}

/// Get the current count of pinned snapshots (for testing).
/// Get the current count of pinned snapshots (for testing/debugging).
pub fn pin_count() -> usize {
    PINNING_TABLE.with(|cell| cell.borrow().pin_count())
}

pub fn debug_snapshot_pinning_stats() -> SnapshotPinningDebugStats {
    PINNING_TABLE.with(|cell| cell.borrow().debug_stats())
}

#[cfg(test)]
pub fn reset_pinning_table() {
    PINNING_TABLE.with(|cell| {
        let mut table = cell.borrow_mut();
        table.heap = SnapshotDoubleIndexHeap::new();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() {
        reset_pinning_table();
    }

    #[test]
    fn test_invalid_handle() {
        let handle = PinHandle::INVALID;
        assert!(!handle.is_valid());
        assert_eq!(handle.0, 0);
    }

    #[test]
    fn test_valid_handle() {
        setup();
        let invalid = SnapshotIdSet::new().set(10);
        let handle = track_pinning(20, &invalid);
        assert!(handle.is_valid());
        assert!(handle.0 > 0);
    }

    #[test]
    fn test_track_and_release() {
        setup();

        let invalid = SnapshotIdSet::new().set(10);
        let handle = track_pinning(20, &invalid);

        assert_eq!(pin_count(), 1);
        assert_eq!(lowest_pinned_snapshot(), Some(10));

        release_pinning(handle);
        assert_eq!(pin_count(), 0);
        assert_eq!(lowest_pinned_snapshot(), None);
    }

    #[test]
    fn test_multiple_pins() {
        setup();

        let invalid1 = SnapshotIdSet::new().set(10);
        let handle1 = track_pinning(20, &invalid1);

        let invalid2 = SnapshotIdSet::new().set(5).set(15);
        let handle2 = track_pinning(30, &invalid2);

        assert_eq!(pin_count(), 2);
        assert_eq!(lowest_pinned_snapshot(), Some(5));

        release_pinning(handle1);
        assert_eq!(pin_count(), 1);
        assert_eq!(lowest_pinned_snapshot(), Some(5));

        release_pinning(handle2);
        assert_eq!(pin_count(), 0);
        assert_eq!(lowest_pinned_snapshot(), None);
    }

    #[test]
    fn test_duplicate_pins() {
        setup();

        let invalid = SnapshotIdSet::new().set(10);
        let handle1 = track_pinning(20, &invalid);
        let handle2 = track_pinning(25, &invalid);

        assert_eq!(pin_count(), 2);
        assert_eq!(lowest_pinned_snapshot(), Some(10));

        release_pinning(handle1);
        assert_eq!(pin_count(), 1);
        assert_eq!(lowest_pinned_snapshot(), Some(10));

        release_pinning(handle2);
        assert_eq!(pin_count(), 0);
        assert_eq!(lowest_pinned_snapshot(), None);
    }

    #[test]
    fn test_pin_ordering() {
        setup();

        let invalid1 = SnapshotIdSet::new().set(30);
        let _handle1 = track_pinning(40, &invalid1);

        let invalid2 = SnapshotIdSet::new().set(10);
        let _handle2 = track_pinning(20, &invalid2);

        let invalid3 = SnapshotIdSet::new().set(20);
        let _handle3 = track_pinning(30, &invalid3);

        assert_eq!(lowest_pinned_snapshot(), Some(10));
    }

    #[test]
    fn test_release_invalid_handle() {
        setup();

        release_pinning(PinHandle::INVALID);
        assert_eq!(pin_count(), 0);
    }

    #[test]
    fn test_empty_invalid_set() {
        setup();

        let invalid = SnapshotIdSet::new();
        let handle = track_pinning(100, &invalid);

        assert_eq!(pin_count(), 1);
        assert_eq!(lowest_pinned_snapshot(), Some(100));

        release_pinning(handle);
    }

    #[test]
    fn test_lowest_from_invalid_set() {
        setup();

        let invalid = SnapshotIdSet::new().set(5).set(10).set(15).set(20);
        let handle = track_pinning(25, &invalid);

        assert_eq!(lowest_pinned_snapshot(), Some(5));

        release_pinning(handle);
    }

    #[test]
    fn test_concurrent_snapshots() {
        setup();

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let invalid = SnapshotIdSet::new().set(i * 10);
                track_pinning(i * 10 + 5, &invalid)
            })
            .collect();

        assert_eq!(pin_count(), 10);
        assert_eq!(lowest_pinned_snapshot(), Some(0));

        for handle in handles {
            release_pinning(handle);
        }

        assert_eq!(pin_count(), 0);
        assert_eq!(lowest_pinned_snapshot(), None);
    }

    #[test]
    fn test_heap_handle_based_removal() {
        setup();

        let invalid1 = SnapshotIdSet::new().set(42);
        let invalid2 = SnapshotIdSet::new().set(17);
        let invalid3 = SnapshotIdSet::new().set(99);

        let h1 = track_pinning(50, &invalid1);
        let h2 = track_pinning(25, &invalid2);
        let h3 = track_pinning(100, &invalid3);

        assert_eq!(pin_count(), 3);
        assert_eq!(lowest_pinned_snapshot(), Some(17));

        release_pinning(h1);
        assert_eq!(pin_count(), 2);
        assert_eq!(lowest_pinned_snapshot(), Some(17));

        release_pinning(h2);
        assert_eq!(pin_count(), 1);
        assert_eq!(lowest_pinned_snapshot(), Some(99));

        release_pinning(h3);
        assert!(pin_count() == 0);
    }
}
