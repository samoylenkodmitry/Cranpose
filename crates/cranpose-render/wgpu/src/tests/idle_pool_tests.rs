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

#[test]
fn a_frames_items_wait_for_the_next_frame_and_older_ones_are_capped() {
    let mut pool = IdlePool::default();
    pool.put(1, 64, u64::MAX, weight);
    pool.end_frame();
    for item in 10..30 {
        pool.put(item, 64, u64::MAX, weight);
    }
    pool.end_frame_keeping(4);
    assert_eq!(
        pool.len(),
        21,
        "every item of the frame waits for the next frame; one earlier item is under the cap"
    );
    assert_eq!(pool.take(|item| *item == 10), Some(10));
    pool.end_frame_keeping(4);
    assert_eq!(
        pool.iter().copied().collect::<Vec<_>>(),
        vec![26, 27, 28, 29],
        "past their next frame, only the newest items stay"
    );
}

/// Fills `scratch` with `len` items as one use of a frame.
fn use_scratch(scratch: &mut FrameScratch<u64>, len: u64) {
    scratch.clear();
    scratch.extend(0..len);
}

#[test]
fn frame_scratch_gives_back_a_startup_burst_once_it_is_idle_frames_old() {
    let mut scratch = FrameScratch::default();
    use_scratch(&mut scratch, 4096);
    scratch.end_frame();
    let burst = scratch.capacity();
    for _ in 0..IDLE_FRAMES {
        use_scratch(&mut scratch, 1000);
        use_scratch(&mut scratch, 100);
        scratch.end_frame();
        assert_eq!(
            scratch.capacity(),
            burst,
            "the burst holds for the idle frames"
        );
    }
    use_scratch(&mut scratch, 1000);
    use_scratch(&mut scratch, 100);
    scratch.end_frame();
    assert!(
        (1000..2000).contains(&scratch.capacity()),
        "then the capacity follows the busiest use of the frames since, not the last use: {}",
        scratch.capacity()
    );
}

#[test]
fn frame_scratch_never_shrinks_while_demand_moves_within_twice_its_peak() {
    let mut scratch = FrameScratch::default();
    let mut capacities = Vec::new();
    for frame in 0..IDLE_FRAMES * 3 {
        use_scratch(&mut scratch, if frame % 7 == 0 { 1500 } else { 800 });
        scratch.end_frame();
        capacities.push(scratch.capacity());
    }
    let grown = capacities[0];
    assert!(
        capacities.iter().all(|capacity| *capacity == grown),
        "a frame a little over the peak grows once and nothing shrinks back: {capacities:?}"
    );
}
