use super::*;

#[test]
fn an_unseen_id_has_no_slot_and_generation_zero() {
    let index = StableIndex::default();

    assert_eq!(index.slot(NodeId(7)), None);
    assert_eq!(index.generation(NodeId(7)), 0);
    assert_eq!(index.live(), 0);
    assert_eq!(index.occupied(), 0);
    assert_eq!(index.capacity(), 0);
}

#[test]
fn a_fresh_id_takes_its_slot_at_generation_zero() {
    let mut index = StableIndex::default();

    index.insert_fresh(NodeId(3), 11);

    assert_eq!(index.slot(NodeId(3)), Some(11));
    assert_eq!(index.generation(NodeId(3)), 0);
    assert_eq!(index.live(), 1);
    assert_eq!(index.occupied(), 1);
    assert_eq!(index.capacity(), PAGE_LEN);
}

#[test]
fn a_fresh_insert_resets_a_generation_left_behind() {
    let mut index = StableIndex::default();
    index.insert_fresh(NodeId(5), 0);
    index.release(NodeId(5));

    index.insert_fresh(NodeId(5), 2);

    assert_eq!(index.generation(NodeId(5)), 0);
}

#[test]
fn releasing_counts_a_generation_and_setting_a_slot_keeps_it() {
    let mut index = StableIndex::default();
    index.insert_fresh(NodeId(9), 4);

    index.release(NodeId(9));
    assert_eq!(index.slot(NodeId(9)), None);
    assert_eq!(index.generation(NodeId(9)), 1);
    assert_eq!(index.live(), 0);
    assert_eq!(
        index.occupied(),
        1,
        "a retired generation stays until pruned"
    );

    index.set_slot(NodeId(9), 6);
    assert_eq!(index.slot(NodeId(9)), Some(6));
    assert_eq!(index.generation(NodeId(9)), 1);
    assert_eq!(index.live(), 1);
    assert_eq!(index.occupied(), 1);
}

#[test]
fn releasing_an_unseen_id_starts_its_generation_at_one() {
    let mut index = StableIndex::default();

    index.release(NodeId(12));

    assert_eq!(index.generation(NodeId(12)), 1);
    assert_eq!(index.occupied(), 1);
}

#[test]
fn retain_retired_forgets_only_generations_without_a_slot_it_does_not_keep() {
    let mut index = StableIndex::default();
    index.insert_fresh(NodeId(1), 0);
    index.insert_fresh(NodeId(2), 1);
    index.insert_fresh(NodeId(3), 2);
    index.release(NodeId(2));
    index.release(NodeId(3));

    index.retain_retired(|id| id == NodeId(3));

    assert_eq!(index.slot(NodeId(1)), Some(0), "a live id is never pruned");
    assert_eq!(
        index.generation(NodeId(2)),
        0,
        "a dropped id's generation is forgotten"
    );
    assert_eq!(
        index.generation(NodeId(3)),
        1,
        "a kept id's generation stays"
    );
    assert_eq!(index.live(), 1);
    assert_eq!(index.occupied(), 2);
}

#[test]
fn a_page_frees_when_its_last_id_retires_and_is_reused_for_a_new_range() {
    let mut index = StableIndex::default();
    let ids: Vec<NodeId> = (0..PAGE_LEN).map(NodeId).collect();
    for &id in &ids {
        index.insert_fresh(id, id.0);
    }
    assert_eq!(index.capacity(), PAGE_LEN);

    for &id in &ids {
        index.release(id);
    }
    index.retain_retired(|_| false);
    assert_eq!(index.capacity(), 0, "an empty page is freed");
    assert_eq!(index.occupied(), 0);
    assert!(index.spare.is_some(), "the freed page waits as the spare");

    index.insert_fresh(NodeId(PAGE_LEN * 3), 0);
    assert!(index.spare.is_none(), "a new range takes the spare page");
    assert_eq!(index.capacity(), PAGE_LEN);
    assert_eq!(index.slot(NodeId(PAGE_LEN * 3)), Some(0));
    assert_eq!(
        index.slot(NodeId(0)),
        None,
        "the reused page holds nothing of the old range"
    );
}

#[test]
fn ids_far_apart_share_nothing_and_free_their_directories() {
    let mut index = StableIndex::default();
    let far = 900_000_000;
    index.insert_fresh(NodeId(1), 0);
    index.insert_fresh(NodeId(far), 1);

    assert_eq!(index.slot(NodeId(1)), Some(0));
    assert_eq!(index.slot(NodeId(far)), Some(1));
    assert_eq!(index.slot(NodeId(far - 1)), None);
    assert_eq!(index.capacity(), 2 * PAGE_LEN);

    index.release(NodeId(far));
    index.retain_retired(|_| false);
    assert_eq!(index.slot(NodeId(far)), None);
    assert_eq!(index.generation(NodeId(far)), 0);
    assert!(
        index.directories[split(NodeId(far)).0].is_none(),
        "an empty directory is freed"
    );
    assert_eq!(index.capacity(), PAGE_LEN);
}
