use super::*;

#[test]
fn test_empty_heap() {
    let heap = SnapshotDoubleIndexHeap::new();
    assert_eq!(heap.len(), 0);
    assert!(heap.is_empty());
    assert_eq!(heap.lowest_or_default(SnapshotId(999)), SnapshotId(999));
}

#[test]
fn test_add_single_element() {
    let mut heap = SnapshotDoubleIndexHeap::new();
    let handle = heap.add(SnapshotId(42));

    assert_eq!(heap.len(), 1);
    assert!(!heap.is_empty());
    assert_eq!(heap.lowest_or_default(SnapshotId(999)), SnapshotId(42));

    heap.remove(handle);
    assert_eq!(heap.len(), 0);
    assert_eq!(heap.lowest_or_default(SnapshotId(999)), SnapshotId(999));
}

#[test]
fn test_add_multiple_maintains_min() {
    let mut heap = SnapshotDoubleIndexHeap::new();

    heap.add(SnapshotId(50));
    assert_eq!(heap.lowest_or_default(SnapshotId(0)), SnapshotId(50));

    heap.add(SnapshotId(30));
    assert_eq!(heap.lowest_or_default(SnapshotId(0)), SnapshotId(30));

    heap.add(SnapshotId(70));
    assert_eq!(heap.lowest_or_default(SnapshotId(0)), SnapshotId(30));

    heap.add(SnapshotId(10));
    assert_eq!(heap.lowest_or_default(SnapshotId(0)), SnapshotId(10));

    assert_eq!(heap.len(), 4);
}

#[test]
fn test_remove_maintains_heap_invariant() {
    let mut heap = SnapshotDoubleIndexHeap::new();

    let h1 = heap.add(SnapshotId(50));
    let h2 = heap.add(SnapshotId(30));
    let h3 = heap.add(SnapshotId(70));
    let h4 = heap.add(SnapshotId(10));

    assert_eq!(heap.lowest_or_default(SnapshotId(0)), SnapshotId(10));

    heap.remove(h4);
    assert_eq!(heap.lowest_or_default(SnapshotId(0)), SnapshotId(30));
    assert_eq!(heap.len(), 3);

    heap.remove(h1);
    assert_eq!(heap.lowest_or_default(SnapshotId(0)), SnapshotId(30));
    assert_eq!(heap.len(), 2);

    heap.remove(h2);
    assert_eq!(heap.lowest_or_default(SnapshotId(0)), SnapshotId(70));
    assert_eq!(heap.len(), 1);

    heap.remove(h3);
    assert!(heap.is_empty());
    assert_eq!(heap.lowest_or_default(SnapshotId(999)), SnapshotId(999));
}

#[test]
fn test_heap_invariant_after_operations() {
    let mut heap = SnapshotDoubleIndexHeap::new();

    let values = vec![100, 20, 80, 5, 60, 15, 90, 3, 40];
    let mut handles = Vec::new();

    for &v in &values {
        handles.push(heap.add(SnapshotId(v)));
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
    assert_eq!(heap.lowest_or_default(SnapshotId(0)), SnapshotId(3));

    heap.remove(handles[3]);
    verify_heap_invariant(&heap);
    assert_eq!(heap.lowest_or_default(SnapshotId(0)), SnapshotId(3));

    heap.remove(handles[7]);
    verify_heap_invariant(&heap);
    assert_eq!(heap.lowest_or_default(SnapshotId(0)), SnapshotId(15));

    heap.remove(handles[1]);
    verify_heap_invariant(&heap);
}

#[test]
fn test_handle_reuse() {
    let mut heap = SnapshotDoubleIndexHeap::new();

    let h1 = heap.add(SnapshotId(1));
    let h2 = heap.add(SnapshotId(2));
    let h3 = heap.add(SnapshotId(3));

    heap.remove(h2);
    heap.remove(h1);

    let h4 = heap.add(SnapshotId(4));
    let h5 = heap.add(SnapshotId(5));

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
        handles.push(heap.add(SnapshotId(i)));
    }

    assert_eq!(heap.len(), 20);
    assert_eq!(heap.lowest_or_default(SnapshotId(999)), SnapshotId(0));

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
        handles.push(heap.add(SnapshotId(i * 7 % 97)));
    }

    for i in (0..handles.len()).step_by(2) {
        heap.remove(handles[i]);
    }

    assert_eq!(heap.len(), 50);

    for i in 100..150 {
        handles.push(heap.add(SnapshotId(i * 3 % 89)));
    }

    assert_eq!(heap.len(), 100);

    let _ = heap.lowest_or_default(SnapshotId(0));
}
