use cranpose_foundation::lazy::{LazyItems, LazyLayoutKey, LazyListIntervalContent, LazyListScope};

fn keyed(count: usize) -> LazyListIntervalContent {
    let mut content = LazyListIntervalContent::new();
    content.items(
        LazyItems::new(count).key(|index| (index * 10) as u64),
        |_| {},
    );
    content
}

#[test]
fn a_user_key_and_an_index_key_are_told_apart() {
    assert!(LazyLayoutKey::User(3).is_user_key());
    assert!(
        !LazyLayoutKey::Index(3).is_user_key(),
        "a positional key claimed to be one the caller named"
    );
    assert_ne!(LazyLayoutKey::User(3), LazyLayoutKey::Index(3));
    assert_ne!(
        LazyLayoutKey::User(3).to_slot_id(),
        LazyLayoutKey::Index(3).to_slot_id()
    );
}

#[test]
fn an_item_run_reports_the_functions_it_was_given_and_no_others() {
    let plain = LazyItems::new(4);
    assert!(plain.key_fn().is_none(), "an unkeyed run invented a key");
    assert!(plain.content_type_fn().is_none());

    let keyed = LazyItems::new(4).key(|index| index as u64);
    assert!(keyed.key_fn().is_some());
    assert!(
        keyed.content_type_fn().is_none(),
        "naming an identity must not also name a reuse class"
    );

    let classed = LazyItems::new(4)
        .key(|index| index as u64)
        .content_type(|_| 7);
    assert!(classed.key_fn().is_some());
    assert!(classed.content_type_fn().is_some());
}

#[test]
fn a_key_is_found_at_the_index_it_sits_at() {
    let content = keyed(5);
    for index in 0..5 {
        assert_eq!(
            content.get_index_by_key(LazyLayoutKey::User((index * 10) as u64)),
            Some(index),
            "key {} was not found at {index}",
            index * 10
        );
    }
    assert_eq!(
        content.get_index_by_key(LazyLayoutKey::User(999)),
        None,
        "a key nobody used was found somewhere"
    );
}

#[test]
fn a_key_lookup_is_the_same_answer_above_and_below_the_cache_threshold() {
    for count in [8usize, 200] {
        let content = keyed(count);
        let target = count - 2;
        assert_eq!(
            content.get_index_by_key(LazyLayoutKey::User((target * 10) as u64)),
            Some(target),
            "a list of {count} answered differently"
        );
    }
}

#[test]
fn a_ranged_key_lookup_only_looks_inside_its_range() {
    let content = keyed(10);
    let key = LazyLayoutKey::User(70);

    assert_eq!(content.get_index_by_key_in_range(key, 5..10), Some(7));
    assert_eq!(
        content.get_index_by_key_in_range(key, 0..5),
        None,
        "the lookup answered from outside the range it was given"
    );
}

#[test]
fn a_ranged_lookup_clamps_a_range_that_runs_past_the_list() {
    let content = keyed(10);
    let key = LazyLayoutKey::User(90);
    assert_eq!(content.get_index_by_key_in_range(key, 0..10_000), Some(9));
    assert_eq!(content.get_index_by_key_in_range(key, 50..10_000), None);
}

#[test]
fn a_ranged_slot_id_lookup_agrees_with_the_key_it_came_from() {
    let content = keyed(10);
    let key = LazyLayoutKey::User(40);
    assert_eq!(
        content.get_index_by_slot_id_in_range(key.to_slot_id(), 0..10),
        content.get_index_by_key_in_range(key, 0..10),
        "the slot id and the key it was derived from found different items"
    );
}
