use super::*;

fn weight(item: &u64) -> u64 {
    *item
}

#[test]
fn take_hands_out_the_oldest_match_and_leaves_the_rest() {
    let mut pool = IdlePool::default();
    for item in [4, 7, 8] {
        pool.put(item, 8, u64::MAX, weight);
    }
    assert_eq!(pool.take(|item| item % 2 == 0), Some(4));
    assert_eq!(pool.take(|item| *item > 100), None);
    assert_eq!(pool.iter().copied().collect::<Vec<_>>(), vec![7, 8]);
    assert_eq!(pool.len(), 2);
}

#[test]
fn put_drops_the_oldest_past_the_count_or_the_weight() {
    let mut pool = IdlePool::default();
    for item in [1, 2, 3] {
        pool.put(item, 2, u64::MAX, weight);
    }
    assert_eq!(pool.iter().copied().collect::<Vec<_>>(), vec![2, 3]);
    pool.put(10, 8, 12, weight);
    assert_eq!(
        pool.iter().copied().collect::<Vec<_>>(),
        vec![10],
        "the oldest leave until the rest weigh no more than the budget"
    );
    pool.put(50, 8, 12, weight);
    assert_eq!(
        pool.iter().copied().collect::<Vec<_>>(),
        vec![50],
        "the newest stays even when it alone is over the budget"
    );
}

#[test]
fn end_frame_drops_what_no_frame_took_for_the_idle_frames() {
    let mut pool = IdlePool::default();
    pool.put(1, 8, u64::MAX, weight);
    pool.put(2, 8, u64::MAX, weight);
    for _ in 0..IDLE_FRAMES {
        let reused = pool.take(|item| *item == 2);
        assert_eq!(reused, Some(2));
        pool.put(2, 8, u64::MAX, weight);
        pool.end_frame();
    }
    assert_eq!(pool.len(), 2, "an item idle for the idle frames stays");
    pool.end_frame();
    assert_eq!(
        pool.iter().copied().collect::<Vec<_>>(),
        vec![2],
        "one frame more drops it; the item handed back every frame stays"
    );
}
