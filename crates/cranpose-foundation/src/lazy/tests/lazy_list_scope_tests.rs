use std::cell::Cell;

use super::*;

#[test]
fn key_overflow_warning_suppression_has_no_process_global_state() {
    let source = include_str!("../lazy_list_scope.rs");
    let user_logged = ["USER_OVERFLOW", "_LOGGED"].concat();
    let index_logged = ["INDEX_OVERFLOW", "_LOGGED"].concat();
    let atomic_bool = ["Atomic", "Bool"].concat();

    assert!(
        !source.contains(&user_logged)
            && !source.contains(&index_logged)
            && !source.contains(&atomic_bool),
        "lazy-list key overflow diagnostics must not use process-global suppression state"
    );
}

#[test]
fn test_single_item() {
    let mut content = LazyListIntervalContent::new();
    let called = Rc::new(Cell::new(false));
    let called_clone = Rc::clone(&called);

    content.item_keyed(Some(42), None, move || {
        called_clone.set(true);
    });

    assert_eq!(content.item_count(), 1);
    assert_eq!(content.get_key(0), LazyLayoutKey::User(42));

    content.invoke_content(0);
    assert!(called.get());
}

#[test]
fn test_multiple_items() {
    let mut content = LazyListIntervalContent::new();

    content.items(LazyItems::new(5).key(|i| (i * 10) as u64), |_i| {});

    assert_eq!(content.item_count(), 5);
    assert_eq!(content.get_key(0), LazyLayoutKey::User(0));
    assert_eq!(content.get_key(1), LazyLayoutKey::User(10));
    assert_eq!(content.get_key(4), LazyLayoutKey::User(40));
}

#[test]
fn test_mixed_intervals() {
    let mut content = LazyListIntervalContent::new();

    content.item_keyed(Some(100), None, || {});

    content.items(LazyItems::new(3).key(|i| i as u64), |_| {});

    content.item_keyed(Some(200), None, || {});

    assert_eq!(content.item_count(), 5);
    assert_eq!(content.get_key(0), LazyLayoutKey::User(100));
    assert_eq!(content.get_key(1), LazyLayoutKey::User(0));
    assert_eq!(content.get_key(2), LazyLayoutKey::User(1));
    assert_eq!(content.get_key(3), LazyLayoutKey::User(2));
    assert_eq!(content.get_key(4), LazyLayoutKey::User(200));
}

#[test]
fn test_with_interval() {
    let mut content = LazyListIntervalContent::new();
    content.items(5, |_| {});

    let result = content.with_interval(3, |local_idx, interval| (local_idx, interval.count));

    assert_eq!(result, Some((3, 5)));
}

#[test]
fn test_user_keys_dont_collide_with_default_keys() {
    let mut content = LazyListIntervalContent::new();

    content.item_keyed(Some(0), None, || {});
    content.item(|| {});
    content.item_keyed(Some(1), None, || {});

    assert_eq!(content.get_key(0), LazyLayoutKey::User(0));
    assert_eq!(content.get_key(1), LazyLayoutKey::Index(1));
    assert_eq!(content.get_key(2), LazyLayoutKey::User(1));

    assert_ne!(content.get_key(0), content.get_key(1));
    assert_ne!(content.get_key(2), content.get_key(1));

    assert_ne!(
        content.get_key(0).to_slot_id(),
        content.get_key(1).to_slot_id()
    );
}

#[test]
fn test_slot_id_collision_prevention() {
    let user_key = LazyLayoutKey::User(0);
    let index_key = LazyLayoutKey::Index(0);

    assert_ne!(user_key.to_slot_id(), index_key.to_slot_id());

    assert_eq!(user_key.to_slot_id(), 0);
    assert_eq!(index_key.to_slot_id(), 1u64 << 62);

    assert!(user_key.to_slot_id() < (1u64 << 62));
    assert!(index_key.to_slot_id() >= (1u64 << 62));
    assert!(index_key.to_slot_id() < (2u64 << 62));

    let user_max = LazyLayoutKey::User((1u64 << 62) - 1);
    assert!(
        user_max.to_slot_id() < (1u64 << 62),
        "User keys stay in user range"
    );
    assert_eq!(user_max.to_slot_id(), (1u64 << 62) - 1);

    let index_large = LazyLayoutKey::Index(((1u64 << 62) - 1) as usize);
    assert!(
        index_large.to_slot_id() >= (1u64 << 62),
        "Index keys stay in index range"
    );
    assert!(
        index_large.to_slot_id() < (2u64 << 62),
        "Index keys below reserved range"
    );
}

#[test]
fn test_user_key_overflow_is_stable_and_tagged() {
    let user_max = LazyLayoutKey::User(u64::MAX);
    let slot = user_max.to_slot_id();
    assert_eq!(slot, user_max.to_slot_id());
    assert!(slot < (1u64 << 62));
}

#[test]
fn test_index_key_overflow_is_stable_and_tagged() {
    let index_max = LazyLayoutKey::Index(usize::MAX);
    let slot = index_max.to_slot_id();
    assert_eq!(slot, index_max.to_slot_id());
    assert!(slot >= (1u64 << 62));
    assert!(slot < (2u64 << 62));
}

#[test]
fn test_user_key_high_bits_influence_slot_id() {
    let key_low = LazyLayoutKey::User(0x0000_0000_0000_0001);
    let key_high = LazyLayoutKey::User(0x4000_0000_0000_0001);
    assert_ne!(
        key_low.to_slot_id(),
        key_high.to_slot_id(),
        "High bits are mixed into the slot id to avoid truncation collisions"
    );
}

#[test]
fn test_items_slice() {
    let mut content = LazyListIntervalContent::new();
    let data = vec!["Apple", "Banana", "Cherry"];
    let items_visited = Rc::new(RefCell::new(Vec::new()));
    let items_clone = items_visited.clone();

    content.items_slice(&data, move |item: &&str| {
        items_clone.borrow_mut().push((*item).to_string());
    });

    assert_eq!(content.item_count(), 3);

    for i in 0..3 {
        content.invoke_content(i);
    }

    let visited = items_visited.borrow();
    assert_eq!(*visited, vec!["Apple", "Banana", "Cherry"]);
}

#[test]
fn test_items_indexed() {
    let mut content = LazyListIntervalContent::new();
    let data = vec![
        "Apple".to_string(),
        "Banana".to_string(),
        "Cherry".to_string(),
    ];
    let items_visited = Rc::new(RefCell::new(Vec::new()));
    let items_clone = items_visited.clone();

    content.items_indexed(data, move |index, item: &String| {
        items_clone.borrow_mut().push((index, item.clone()));
    });

    assert_eq!(content.item_count(), 3);

    for i in 0..3 {
        content.invoke_content(i);
    }

    let visited = items_visited.borrow();
    assert_eq!(
        *visited,
        vec![
            (0, "Apple".to_string()),
            (1, "Banana".to_string()),
            (2, "Cherry".to_string())
        ]
    );
}

#[test]
fn test_items_indexed_slice() {
    let mut content = LazyListIntervalContent::new();
    let data = vec!["Apple", "Banana", "Cherry"];
    let items_visited = Rc::new(RefCell::new(Vec::new()));
    let items_clone = items_visited.clone();

    content.items_indexed(data.as_slice(), move |index, item: &&str| {
        items_clone.borrow_mut().push((index, (*item).to_string()));
    });

    assert_eq!(content.item_count(), 3);

    for i in 0..3 {
        content.invoke_content(i);
    }

    let visited = items_visited.borrow();
    assert_eq!(
        *visited,
        vec![
            (0, "Apple".to_string()),
            (1, "Banana".to_string()),
            (2, "Cherry".to_string())
        ]
    );
}

#[test]
fn test_items_slice_rc() {
    let mut content = LazyListIntervalContent::new();
    let data: Rc<[String]> = Rc::from(vec!["Apple".into(), "Banana".into()]);
    let items_visited = Rc::new(RefCell::new(Vec::new()));
    let items_clone = items_visited.clone();

    content.items_slice_rc(Rc::clone(&data), move |item: &String| {
        items_clone.borrow_mut().push(item.clone());
    });

    assert_eq!(content.item_count(), 2);

    for i in 0..2 {
        content.invoke_content(i);
    }

    let visited = items_visited.borrow();
    assert_eq!(*visited, vec!["Apple", "Banana"]);
}

#[test]
fn test_items_indexed_rc() {
    let mut content = LazyListIntervalContent::new();
    let data: Rc<[String]> = Rc::from(vec!["Apple".into(), "Banana".into()]);
    let items_visited = Rc::new(RefCell::new(Vec::new()));
    let items_clone = items_visited.clone();

    content.items_indexed_rc(Rc::clone(&data), move |index, item: &String| {
        items_clone.borrow_mut().push((index, item.clone()));
    });

    assert_eq!(content.item_count(), 2);

    for i in 0..2 {
        content.invoke_content(i);
    }

    let visited = items_visited.borrow();
    assert_eq!(
        *visited,
        vec![(0, "Apple".to_string()), (1, "Banana".to_string())]
    );
}

#[test]
fn test_items_with_provider() {
    let mut content = LazyListIntervalContent::new();
    let data = ["Apple", "Banana", "Cherry"];
    let items_visited = Rc::new(RefCell::new(Vec::new()));
    let items_clone = items_visited.clone();

    content.items_with_provider(
        data.len(),
        move |index| data.get(index).copied(),
        move |item: &str| {
            items_clone.borrow_mut().push(item.to_string());
        },
    );

    assert_eq!(content.item_count(), 3);

    for i in 0..3 {
        content.invoke_content(i);
    }

    let visited = items_visited.borrow();
    assert_eq!(*visited, vec!["Apple", "Banana", "Cherry"]);
}

#[test]
fn test_items_indexed_with_provider() {
    let mut content = LazyListIntervalContent::new();
    let data = ["Apple", "Banana", "Cherry"];
    let items_visited = Rc::new(RefCell::new(Vec::new()));
    let items_clone = items_visited.clone();

    content.items_indexed_with_provider(
        data.len(),
        move |index| data.get(index).copied(),
        move |index, item: &str| {
            items_clone.borrow_mut().push((index, item.to_string()));
        },
    );

    assert_eq!(content.item_count(), 3);

    for i in 0..3 {
        content.invoke_content(i);
    }

    let visited = items_visited.borrow();
    assert_eq!(
        *visited,
        vec![
            (0, "Apple".to_string()),
            (1, "Banana".to_string()),
            (2, "Cherry".to_string())
        ]
    );
}

#[test]
fn test_large_list_cache_works() {
    let mut content = LazyListIntervalContent::new();

    content.items(LazyItems::new(20_000).key(|i| (i * 7) as u64), |_| {});

    let key_19999 = content.get_key(19999);
    assert_eq!(key_19999, LazyLayoutKey::User(19999 * 7));

    let slot_id = key_19999.to_slot_id();
    let found_index = content.get_index_by_slot_id(slot_id);
    assert_eq!(found_index, Some(19999));

    let key_10000 = content.get_key(10000);
    let slot_id_mid = key_10000.to_slot_id();
    let found_mid = content.get_index_by_slot_id(slot_id_mid);
    assert_eq!(found_mid, Some(10000));
}
