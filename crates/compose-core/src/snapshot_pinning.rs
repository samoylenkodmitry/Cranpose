/// Snapshot pinning system to prevent premature garbage collection of state records.
///
/// This module implements a pinning table that tracks which snapshot IDs need to remain
/// alive. When a snapshot is created, it "pins" the lowest snapshot ID that it depends on,
/// preventing state records from those snapshots from being garbage collected.
///
/// Uses SnapshotDoubleIndexHeap for O(log N) pin/unpin and O(1) lowest queries.
/// Based on Jetpack Compose's pinning mechanism (Snapshot.kt:714-722, 1954).
use crate::snapshot_double_index_heap::SnapshotDoubleIndexHeap;
use crate::snapshot_id_set::{SnapshotId, SnapshotIdSet};
use std::cell::RefCell;

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

/// The global pinning table that tracks pinned snapshots using a min-heap.
struct PinningTable {
    /// Min-heap of pinned snapshot IDs for O(1) lowest queries
    heap: SnapshotDoubleIndexHeap,
}

impl PinningTable {
    fn new() -> Self {
        Self {
            heap: SnapshotDoubleIndexHeap::new(),
        }
    }

    /// Add a pin for the given snapshot ID, returning a handle.
    ///
    /// Time complexity: O(log N)
    fn add(&mut self, snapshot_id: SnapshotId) -> PinHandle {
        let heap_handle = self.heap.add(snapshot_id);
        // Heap handles start at 0, but we reserve 0 as INVALID for PinHandle
        // So we offset by 1: heap handle 0 → PinHandle(1), etc.
        PinHandle(heap_handle + 1)
    }

    /// Remove a pin by handle.
    ///
    /// Time complexity: O(log N)
    fn remove(&mut self, handle: PinHandle) -> bool {
        if !handle.is_valid() {
            return false;
        }

        // Convert PinHandle back to heap handle (subtract 1)
        let heap_handle = handle.0 - 1;

        // Verify handle is within bounds
        if heap_handle < usize::MAX {
            self.heap.remove(heap_handle);
            true
        } else {
            false
        }
    }

    /// Get the lowest pinned snapshot ID, or None if nothing is pinned.
    ///
    /// Time complexity: O(1)
    fn lowest_pinned(&self) -> Option<SnapshotId> {
        if self.heap.is_empty() {
            None
        } else {
            // Use 0 as default (will never be returned since heap is non-empty)
            Some(self.heap.lowest_or_default(0))
        }
    }

    /// Get the count of pins (for testing).
    #[cfg(test)]
    fn pin_count(&self) -> usize {
        self.heap.len()
    }
}

thread_local! {
    // Global pinning table protected by a mutex.
    #[allow(clippy::missing_const_for_thread_local)]
    static PINNING_TABLE: RefCell<PinningTable> = RefCell::new(PinningTable::new());
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
    // Pin the lowest snapshot ID that this snapshot depends on
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
#[cfg(test)]
pub fn pin_count() -> usize {
    PINNING_TABLE.with(|cell| cell.borrow().pin_count())
}

/// Reset the pinning table (for testing).
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

    // Helper to ensure tests start with clean state
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

        // Release first pin
        release_pinning(handle1);
        assert_eq!(pin_count(), 1);
        assert_eq!(lowest_pinned_snapshot(), Some(5));

        // Release second pin
        release_pinning(handle2);
        assert_eq!(pin_count(), 0);
        assert_eq!(lowest_pinned_snapshot(), None);
    }

    #[test]
    fn test_duplicate_pins() {
        setup();

        // Pin the same snapshot ID twice
        let invalid = SnapshotIdSet::new().set(10);
        let handle1 = track_pinning(20, &invalid);
        let handle2 = track_pinning(25, &invalid);

        assert_eq!(pin_count(), 2);
        assert_eq!(lowest_pinned_snapshot(), Some(10));

        // Releasing one doesn't unpin completely
        release_pinning(handle1);
        assert_eq!(pin_count(), 1);
        assert_eq!(lowest_pinned_snapshot(), Some(10));

        // Releasing second one unpins completely
        release_pinning(handle2);
        assert_eq!(pin_count(), 0);
        assert_eq!(lowest_pinned_snapshot(), None);
    }

    #[test]
    fn test_pin_ordering() {
        setup();

        // Add pins in non-sorted order
        let invalid1 = SnapshotIdSet::new().set(30);
        let _handle1 = track_pinning(40, &invalid1);

        let invalid2 = SnapshotIdSet::new().set(10);
        let _handle2 = track_pinning(20, &invalid2);

        let invalid3 = SnapshotIdSet::new().set(20);
        let _handle3 = track_pinning(30, &invalid3);

        // Lowest should still be 10
        assert_eq!(lowest_pinned_snapshot(), Some(10));
    }

    #[test]
    fn test_release_invalid_handle() {
        setup();

        // Releasing an invalid handle should not crash
        release_pinning(PinHandle::INVALID);
        assert_eq!(pin_count(), 0);
    }

    #[test]
    fn test_empty_invalid_set() {
        setup();

        // Empty invalid set means snapshot depends on nothing older
        let invalid = SnapshotIdSet::new();
        let handle = track_pinning(100, &invalid);

        // Should pin snapshot 100 itself (lowest returns the upper bound if empty)
        assert_eq!(pin_count(), 1);
        assert_eq!(lowest_pinned_snapshot(), Some(100));

        release_pinning(handle);
    }

    #[test]
    fn test_lowest_from_invalid_set() {
        setup();

        // Create an invalid set with multiple IDs
        let invalid = SnapshotIdSet::new().set(5).set(10).set(15).set(20);
        let handle = track_pinning(25, &invalid);

        // Should pin the lowest ID from the invalid set
        assert_eq!(lowest_pinned_snapshot(), Some(5));

        release_pinning(handle);
    }

    #[test]
    fn test_concurrent_snapshots() {
        setup();

        // Simulate multiple concurrent snapshots
        let handles: Vec<_> = (0..10)
            .map(|i| {
                let invalid = SnapshotIdSet::new().set(i * 10);
                track_pinning(i * 10 + 5, &invalid)
            })
            .collect();

        assert_eq!(pin_count(), 10);
        assert_eq!(lowest_pinned_snapshot(), Some(0));

        // Release all
        for handle in handles {
            release_pinning(handle);
        }

        assert_eq!(pin_count(), 0);
        assert_eq!(lowest_pinned_snapshot(), None);
    }

    #[test]
    fn test_heap_handle_based_removal() {
        setup();

        // Test that we can remove pins using just the handle, without knowing the snapshot ID
        let invalid1 = SnapshotIdSet::new().set(42);
        let invalid2 = SnapshotIdSet::new().set(17);
        let invalid3 = SnapshotIdSet::new().set(99);

        let h1 = track_pinning(50, &invalid1);
        let h2 = track_pinning(25, &invalid2);
        let h3 = track_pinning(100, &invalid3);

        assert_eq!(pin_count(), 3);
        assert_eq!(lowest_pinned_snapshot(), Some(17));

        // Remove middle value using only handle
        release_pinning(h1);
        assert_eq!(pin_count(), 2);
        assert_eq!(lowest_pinned_snapshot(), Some(17));

        // Remove lowest using only handle
        release_pinning(h2);
        assert_eq!(pin_count(), 1);
        assert_eq!(lowest_pinned_snapshot(), Some(99));

        release_pinning(h3);
        assert!(pin_count() == 0);
    }
}
