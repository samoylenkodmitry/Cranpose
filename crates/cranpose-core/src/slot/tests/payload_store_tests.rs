use std::any::TypeId;

use super::*;
use crate::{
    AnchorId,
    slot::{PayloadAnchor, PayloadKind},
};

fn payload(id: usize) -> PayloadRecord {
    PayloadRecord {
        owner: AnchorId::new(1),
        anchor: PayloadAnchor::new(id, 1),
        type_id: TypeId::of::<usize>(),
        type_name: std::any::type_name::<usize>,
        source: crate::slot::BRANCH_PATH_ROOT,
        kind: PayloadKind::Remember,
        value: Box::new(id),
        fresh: None,
    }
}

fn ids(payloads: impl IntoIterator<Item = impl std::borrow::Borrow<PayloadRecord>>) -> Vec<usize> {
    payloads
        .into_iter()
        .map(|payload| payload.borrow().anchor.id())
        .collect()
}

fn store_with(count: usize) -> PayloadStore {
    let mut store = PayloadStore::default();
    for id in 0..count {
        store.insert_item(id, payload(id));
    }
    store
}

#[test]
fn insert_keeps_table_order_and_reads_by_index() {
    let mut store = store_with(3);
    store.insert_item(0, payload(9));

    assert_eq!(store.len(), 4);
    assert_eq!(store.item_count(), 4);
    assert_eq!(ids(store.iter()), [9, 0, 1, 2]);
    assert_eq!(store.get(0).map(|payload| payload.anchor.id()), Some(9));
    assert_eq!(store.item(3).map(|payload| payload.anchor.id()), Some(2));
    assert!(store.get(4).is_none());

    if let Some(payload) = store.get_mut(1) {
        payload.kind = PayloadKind::Effect;
    }
    assert_eq!(
        store.get(1).map(|payload| payload.kind),
        Some(PayloadKind::Effect)
    );
    assert!(store.get_mut(4).is_none());
    assert_eq!(store.validate_integrity(), Ok(()));
}

#[test]
fn insert_moves_one_order_entry_per_later_payload() {
    let mut store = store_with(100);
    let before = store.shift_bytes();

    store.insert_item(10, payload(100));

    assert_eq!(store.shift_bytes() - before, 90 * mem::size_of::<u32>());
}

#[test]
fn removed_slots_are_reused_without_growing_storage() {
    let mut store = store_with(4);

    let removed = store.remove_items(1..3);
    assert_eq!(ids(removed), [1, 2]);
    assert_eq!(ids(store.iter()), [0, 3]);
    assert_eq!(store.validate_integrity(), Ok(()));

    let slot_len = store.slots.len();
    store.insert_item(1, payload(5));
    store.insert_item(1, payload(6));

    assert_eq!(store.slots.len(), slot_len);
    assert!(store.free.is_empty());
    assert_eq!(ids(store.iter()), [0, 6, 5, 3]);
    assert_eq!(store.validate_integrity(), Ok(()));
}

#[test]
fn insert_items_restores_records_in_order() {
    let mut store = store_with(3);
    let removed = store.remove_items(0..2);

    store.insert_items(1, removed);

    assert_eq!(ids(store.iter()), [2, 0, 1]);
    assert_eq!(store.validate_integrity(), Ok(()));
}

#[test]
fn rotate_items_right_reorders_only_the_range() {
    let mut store = store_with(4);

    store.rotate_items_right(1..4, 1);
    assert_eq!(ids(store.iter()), [0, 3, 1, 2]);

    store.rotate_items_right(2..9, 1);
    assert_eq!(ids(store.iter()), [0, 3, 1, 2]);
    assert_eq!(store.validate_integrity(), Ok(()));
}

#[test]
fn range_reads_table_order_and_ignores_out_of_bounds_ranges() {
    let store = store_with(5);

    assert_eq!(ids(store.range(1..4)), [1, 2, 3]);
    assert_eq!(store.range(4..9).count(), 0);
}

#[test]
fn for_each_mut_visits_in_table_order() {
    let mut store = store_with(3);
    store.rotate_items_right(0..3, 1);
    let mut visited = Vec::new();

    store.for_each_mut(|payload| {
        visited.push(payload.anchor.id());
        payload.kind = PayloadKind::Internal;
    });

    assert_eq!(visited, [2, 0, 1]);
    assert!(
        store
            .iter()
            .all(|payload| payload.kind == PayloadKind::Internal)
    );
}

#[test]
fn drain_rev_yields_reverse_table_order_and_empties_the_store() {
    let mut store = store_with(4);
    drop(store.remove_items(1..2));
    let mut drained = Vec::new();

    store.drain_rev(|payload| drained.push(payload.anchor.id()));

    assert_eq!(drained, [3, 2, 0]);
    assert_eq!(store.len(), 0);
    assert!(store.slots.is_empty());
    assert!(store.free.is_empty());
    assert_eq!(store.validate_integrity(), Ok(()));
}

#[test]
fn compact_renumbers_slots_densely_in_table_order() {
    let mut store = store_with(6);
    drop(store.remove_items(0..3));
    store.rotate_items_right(0..3, 1);

    store.compact();

    assert_eq!(ids(store.iter()), [5, 3, 4]);
    assert_eq!(store.order, [0, 1, 2]);
    assert_eq!(store.slots.len(), 3);
    assert_eq!(store.capacity(), 3);
    assert!(store.free.is_empty());
    assert_eq!(store.validate_integrity(), Ok(()));
}

#[test]
fn heap_bytes_counts_order_slots_and_free_list() {
    let mut store = store_with(8);
    drop(store.remove_items(0..2));

    assert_eq!(
        store.heap_bytes(),
        (store.order.capacity() + store.free.capacity()) * mem::size_of::<u32>()
            + store.slots.capacity() * mem::size_of::<Option<PayloadRecord>>()
    );
}

#[test]
fn validate_integrity_reports_vacant_order_entry() {
    let mut store = store_with(2);
    store.slots[1] = None;

    assert_eq!(
        store.validate_integrity(),
        Err(SlotInvariantError::PayloadStoreMismatch {
            detail: "payload order entry must name an occupied slot",
            expected: 1,
            actual: 1,
        })
    );
}

#[test]
fn validate_integrity_reports_duplicate_order_entry() {
    let mut store = store_with(2);
    store.order[1] = 0;

    assert_eq!(
        store.validate_integrity(),
        Err(SlotInvariantError::PayloadStoreMismatch {
            detail: "payload slot must appear once in the order",
            expected: 1,
            actual: 0,
        })
    );
}

#[test]
fn validate_integrity_reports_occupied_free_slot() {
    let mut store = store_with(2);
    store.free.push(0);

    assert_eq!(
        store.validate_integrity(),
        Err(SlotInvariantError::PayloadStoreMismatch {
            detail: "free payload slot must be vacant",
            expected: 0,
            actual: 0,
        })
    );
}

#[test]
fn validate_integrity_reports_duplicate_free_slot() {
    let mut store = store_with(2);
    drop(store.remove_items(1..2));
    store.free.push(1);

    assert_eq!(
        store.validate_integrity(),
        Err(SlotInvariantError::PayloadStoreMismatch {
            detail: "free payload slot must appear once",
            expected: 1,
            actual: 1,
        })
    );
}

#[test]
fn validate_integrity_reports_unordered_occupied_slot() {
    let mut store = store_with(2);
    store.order.pop();

    assert_eq!(
        store.validate_integrity(),
        Err(SlotInvariantError::PayloadStoreMismatch {
            detail: "every payload slot must be ordered or free",
            expected: 2,
            actual: 1,
        })
    );
}

#[test]
fn validate_integrity_reports_leaked_vacant_slot() {
    let mut store = store_with(2);
    drop(store.remove_items(1..2));
    store.free.clear();

    assert_eq!(
        store.validate_integrity(),
        Err(SlotInvariantError::PayloadStoreMismatch {
            detail: "every payload slot must be ordered or free",
            expected: 2,
            actual: 1,
        })
    );
}

#[test]
fn vec_and_store_segment_items_agree() {
    let mut vec = (0..6).map(payload).collect::<Vec<_>>();
    let mut store = store_with(6);

    for items in [
        &mut vec as &mut dyn SegmentItems<Item = PayloadRecord>,
        &mut store,
    ] {
        let removed = items.remove_items(1..3);
        items.insert_item(0, payload(7));
        items.rotate_items_right(2..5, 2);
        items.insert_items(items.item_count(), removed);
    }

    let vec_ids = ids(&vec);
    assert_eq!(vec_ids, ids(store.iter()));
    assert_eq!(vec.item_count(), store.item_count());
    assert_eq!(
        vec.item(2).map(|payload| payload.anchor.id()),
        store.item(2).map(|payload| payload.anchor.id())
    );
}
