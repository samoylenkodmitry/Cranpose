use crate::snapshot_id_set::SnapshotId;

const INITIAL_CAPACITY: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SnapshotDoubleIndexHeapDebugStats {
    pub len: usize,
    pub values_cap: usize,
    pub index_cap: usize,
    pub handles_len: usize,
    pub handles_cap: usize,
}

/// A min-heap that maintains snapshot IDs and allows O(1) access to the minimum value.
///
/// Uses handle-based removal so callers don't need to track array indices.
#[derive(Debug)]
pub struct SnapshotDoubleIndexHeap {
    size: usize,

    values: Vec<SnapshotId>,

    index: Vec<usize>,

    handles: Vec<usize>,

    first_free_handle: usize,
}

impl SnapshotDoubleIndexHeap {
    /// Creates a new empty heap with default capacity
    pub fn new() -> Self {
        Self::with_capacity(INITIAL_CAPACITY)
    }

    /// Creates a new empty heap with specified initial capacity
    pub fn with_capacity(capacity: usize) -> Self {
        let mut handles = Vec::with_capacity(capacity);
        for i in 0..capacity {
            handles.push(i + 1);
        }

        Self {
            size: 0,
            values: Vec::with_capacity(capacity),
            index: Vec::with_capacity(capacity),
            handles,
            first_free_handle: 0,
        }
    }

    /// Returns the number of elements in the heap
    #[inline]
    pub fn len(&self) -> usize {
        self.size
    }

    /// Returns true if the heap is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Returns the minimum snapshot ID, or the default value if empty
    #[inline]
    pub fn lowest_or_default(&self, default: SnapshotId) -> SnapshotId {
        if self.size > 0 {
            self.values[0]
        } else {
            default
        }
    }

    /// Adds a snapshot ID to the heap and returns a handle for later removal
    ///
    /// Time complexity: O(log N)
    pub fn add(&mut self, value: SnapshotId) -> usize {
        self.ensure_capacity(self.size + 1);

        let i = self.size;
        self.size += 1;

        let handle = self.allocate_handle();

        if i >= self.values.len() {
            self.values.push(value);
            self.index.push(handle);
        } else {
            self.values[i] = value;
            self.index[i] = handle;
        }

        self.handles[handle] = i;

        self.shift_up(i);

        handle
    }

    /// Removes the element associated with the given handle
    ///
    /// Time complexity: O(log N)
    pub fn remove(&mut self, handle: usize) {
        let i = self.handles[handle];

        self.swap(i, self.size - 1);
        self.size -= 1;

        self.shift_up(i);
        self.shift_down(i);

        self.free_handle(handle);
    }

    pub fn debug_stats(&self) -> SnapshotDoubleIndexHeapDebugStats {
        SnapshotDoubleIndexHeapDebugStats {
            len: self.size,
            values_cap: self.values.capacity(),
            index_cap: self.index.capacity(),
            handles_len: self.handles.len(),
            handles_cap: self.handles.capacity(),
        }
    }

    fn ensure_capacity(&mut self, capacity: usize) {
        if capacity <= self.values.capacity() {
            return;
        }

        let new_capacity = capacity.max(self.values.capacity() * 2);

        self.values.reserve(new_capacity - self.values.capacity());
        self.index.reserve(new_capacity - self.index.capacity());

        let old_len = self.handles.len();
        self.handles.reserve(new_capacity - old_len);
        for i in old_len..new_capacity {
            self.handles.push(i + 1);
        }
    }

    fn allocate_handle(&mut self) -> usize {
        let handle = self.first_free_handle;

        if handle >= self.handles.len() {
            let new_size = self.handles.len().max(1) * 2;
            for i in self.handles.len()..new_size {
                self.handles.push(i + 1);
            }
        }

        self.first_free_handle = self.handles[handle];
        handle
    }

    fn free_handle(&mut self, handle: usize) {
        self.handles[handle] = self.first_free_handle;
        self.first_free_handle = handle;
    }

    fn swap(&mut self, i: usize, j: usize) {
        if i >= self.size || j >= self.size {
            return;
        }

        self.values.swap(i, j);

        self.index.swap(i, j);

        let handle_i = self.index[i];
        let handle_j = self.index[j];
        self.handles[handle_i] = i;
        self.handles[handle_j] = j;
    }

    fn shift_up(&mut self, mut i: usize) {
        if i >= self.size {
            return;
        }

        let value = self.values[i];

        while i > 0 {
            let parent = (i - 1) / 2;

            if self.values[parent] <= value {
                break;
            }

            self.swap(i, parent);
            i = parent;
        }
    }

    fn shift_down(&mut self, mut i: usize) {
        if i >= self.size {
            return;
        }

        let value = self.values[i];
        let half = self.size / 2;

        while i < half {
            let mut child = 2 * i + 1;
            let right = child + 1;

            if right < self.size && self.values[right] < self.values[child] {
                child = right;
            }

            if value <= self.values[child] {
                break;
            }

            self.swap(i, child);
            i = child;
        }
    }
}

impl Default for SnapshotDoubleIndexHeap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_heap() {
        let heap = SnapshotDoubleIndexHeap::new();
        assert_eq!(heap.len(), 0);
        assert!(heap.is_empty());
        assert_eq!(heap.lowest_or_default(999), 999);
    }

    #[test]
    fn test_add_single_element() {
        let mut heap = SnapshotDoubleIndexHeap::new();
        let handle = heap.add(42);

        assert_eq!(heap.len(), 1);
        assert!(!heap.is_empty());
        assert_eq!(heap.lowest_or_default(999), 42);

        heap.remove(handle);
        assert_eq!(heap.len(), 0);
        assert_eq!(heap.lowest_or_default(999), 999);
    }

    #[test]
    fn test_add_multiple_maintains_min() {
        let mut heap = SnapshotDoubleIndexHeap::new();

        heap.add(50);
        assert_eq!(heap.lowest_or_default(0), 50);

        heap.add(30);
        assert_eq!(heap.lowest_or_default(0), 30);

        heap.add(70);
        assert_eq!(heap.lowest_or_default(0), 30);

        heap.add(10);
        assert_eq!(heap.lowest_or_default(0), 10);

        assert_eq!(heap.len(), 4);
    }

    #[test]
    fn test_remove_maintains_heap_invariant() {
        let mut heap = SnapshotDoubleIndexHeap::new();

        let h1 = heap.add(50);
        let h2 = heap.add(30);
        let h3 = heap.add(70);
        let h4 = heap.add(10);

        assert_eq!(heap.lowest_or_default(0), 10);

        heap.remove(h4);
        assert_eq!(heap.lowest_or_default(0), 30);
        assert_eq!(heap.len(), 3);

        heap.remove(h1);
        assert_eq!(heap.lowest_or_default(0), 30);
        assert_eq!(heap.len(), 2);

        heap.remove(h2);
        assert_eq!(heap.lowest_or_default(0), 70);
        assert_eq!(heap.len(), 1);

        heap.remove(h3);
        assert!(heap.is_empty());
        assert_eq!(heap.lowest_or_default(999), 999);
    }

    #[test]
    fn test_heap_invariant_after_operations() {
        let mut heap = SnapshotDoubleIndexHeap::new();

        let values = vec![100, 20, 80, 5, 60, 15, 90, 3, 40];
        let mut handles = Vec::new();

        for &v in &values {
            handles.push(heap.add(v));
        }

        fn verify_heap_invariant(heap: &SnapshotDoubleIndexHeap) {
            for i in 0..heap.size {
                let left_child = 2 * i + 1;
                let right_child = 2 * i + 2;

                if left_child < heap.size {
                    assert!(
                        heap.values[i] <= heap.values[left_child],
                        "Parent {} > left child {} at positions {}, {}",
                        heap.values[i],
                        heap.values[left_child],
                        i,
                        left_child
                    );
                }

                if right_child < heap.size {
                    assert!(
                        heap.values[i] <= heap.values[right_child],
                        "Parent {} > right child {} at positions {}, {}",
                        heap.values[i],
                        heap.values[right_child],
                        i,
                        right_child
                    );
                }
            }
        }

        verify_heap_invariant(&heap);
        assert_eq!(heap.lowest_or_default(0), 3);

        heap.remove(handles[3]);
        verify_heap_invariant(&heap);
        assert_eq!(heap.lowest_or_default(0), 3);

        heap.remove(handles[7]);
        verify_heap_invariant(&heap);
        assert_eq!(heap.lowest_or_default(0), 15);

        heap.remove(handles[1]);
        verify_heap_invariant(&heap);
    }

    #[test]
    fn test_handle_reuse() {
        let mut heap = SnapshotDoubleIndexHeap::new();

        let h1 = heap.add(1);
        let h2 = heap.add(2);
        let h3 = heap.add(3);

        heap.remove(h2);
        heap.remove(h1);

        let h4 = heap.add(4);
        let h5 = heap.add(5);

        assert_eq!(heap.len(), 3);
        heap.remove(h3);
        heap.remove(h4);
        heap.remove(h5);
        assert!(heap.is_empty());
    }

    #[test]
    fn test_capacity_growth() {
        let mut heap = SnapshotDoubleIndexHeap::with_capacity(2);

        let mut handles = Vec::new();
        for i in 0..20 {
            handles.push(heap.add(i));
        }

        assert_eq!(heap.len(), 20);
        assert_eq!(heap.lowest_or_default(999), 0);

        for handle in handles {
            heap.remove(handle);
        }

        assert!(heap.is_empty());
    }

    #[test]
    fn test_stress_random_operations() {
        let mut heap = SnapshotDoubleIndexHeap::new();
        let mut handles = Vec::new();

        for i in 0..100 {
            handles.push(heap.add(i * 7 % 97));
        }

        for i in (0..handles.len()).step_by(2) {
            heap.remove(handles[i]);
        }

        assert_eq!(heap.len(), 50);

        for i in 100..150 {
            handles.push(heap.add(i * 3 % 89));
        }

        assert_eq!(heap.len(), 100);

        let _ = heap.lowest_or_default(0);
    }
}
