use super::*;
use crate::frame_graph::upload_test_device;

fn quads(count: usize) -> Vec<GlyphInstance> {
    vec![bytemuck::Zeroable::zeroed(); count]
}

#[test]
fn a_span_allocator_takes_the_first_range_that_fits() {
    let mut spans = SpanAllocator::new(10);
    assert_eq!(spans.capacity(), 10);
    assert_eq!(spans.allocate(4), Some(0));
    assert_eq!(spans.allocate(4), Some(4));
    assert_eq!(spans.allocate(4), None);
    spans.release(0..4);
    assert_eq!(spans.allocate(3), Some(0));
    assert_eq!(spans.allocate(2), Some(8));
    assert_eq!(spans.allocate(1), Some(3));
    assert_eq!(spans.allocate(1), None);
    assert_eq!(spans.allocate(0), None);
}

#[test]
fn a_released_span_merges_with_the_ranges_it_touches() {
    let mut spans = SpanAllocator::new(12);
    for start in [0, 3, 6, 9] {
        assert_eq!(spans.allocate(3), Some(start));
    }
    spans.release(0..3);
    spans.release(6..9);
    assert_eq!(spans.used_end(), 12);
    spans.release(3..6);
    assert_eq!(spans.allocate(9), Some(0), "0..9 merged across both sides");
    spans.release(0..9);
    spans.release(9..12);
    assert_eq!(spans.used_end(), 0);
    assert_eq!(spans.allocate(12), Some(0));
}

#[test]
fn a_span_allocator_ignores_empty_and_foreign_spans() {
    let mut spans = SpanAllocator::new(4);
    assert_eq!(spans.allocate(4), Some(0));
    spans.release(2..2);
    spans.release(3..9);
    assert_eq!(spans.allocate(1), None);
    assert_eq!(SpanAllocator::new(0).used_end(), 0);
}

#[test]
fn a_span_allocator_resizes_at_its_free_end_and_never_below_its_spans() {
    let mut spans = SpanAllocator::new(0);
    assert_eq!(spans.used_end(), 0);
    spans.resize(8);
    assert_eq!(spans.allocate(6), Some(0));
    spans.resize(16);
    assert_eq!(
        spans.allocate(10),
        Some(6),
        "the new quads join the free end"
    );
    spans.release(6..16);
    spans.resize(4);
    assert_eq!(spans.capacity(), 6, "no shorter than the quads in use");
    assert_eq!(spans.allocate(1), None);
    spans.release(0..6);
    spans.resize(2);
    assert_eq!((spans.capacity(), spans.used_end()), (2, 0));
    assert_eq!(spans.allocate(2), Some(0));
}

#[test]
fn an_arena_takes_no_run_without_quads() {
    let (_lock, device, _queue) = upload_test_device();
    let mut arena = GlyphRunArena::new(&device, None);
    assert!(arena.insert(&device, quads(0)).is_none());
    assert!(arena.store.is_none());
    assert!(arena.staged_instances.is_empty());
}

#[test]
fn a_dropped_runs_quads_stay_taken_until_the_next_frame() {
    let (_lock, device, _queue) = upload_test_device();
    let mut arena = GlyphRunArena::new(&device, None);
    let dropped = arena.insert(&device, quads(1)).expect("a run");
    let kept = arena.insert(&device, quads(1)).expect("a run");
    assert_eq!(dropped.instances(), 0..1);
    drop(dropped);
    let same_frame = arena.insert(&device, quads(1)).expect("a run");
    assert_eq!(
        same_frame.instances(),
        2..3,
        "a draw recorded this frame may still read the dropped quads"
    );

    arena.begin_frame(&device, 0);
    let next_frame = arena.insert(&device, quads(1)).expect("a run");
    assert_eq!(next_frame.instances(), 0..1);
    assert_eq!(kept.instances(), 1..2);
}

#[test]
fn the_buffer_takes_the_room_its_runs_need_and_gives_back_twice_that() {
    let (_lock, device, _queue) = upload_test_device();
    let mut arena = GlyphRunArena::new(&device, None);
    arena.begin_frame(&device, 0);
    assert!(
        arena.store.is_none(),
        "an arena no run needs holds no buffer"
    );
    arena.begin_frame(&device, 10_000);
    assert_eq!(
        arena.spans.capacity(),
        13 * MIN_QUADS,
        "a quarter more than the last frame's runs, in whole steps"
    );
    arena.begin_frame(&device, 12_000);
    assert_eq!(
        arena.spans.capacity(),
        13 * MIN_QUADS,
        "demand inside the room keeps the buffer"
    );
    let kept = arena.insert(&device, quads(9_000)).expect("a run");
    let dropped = arena.insert(&device, quads(9_000)).expect("a run");
    assert_eq!(
        arena.spans.capacity(),
        26 * MIN_QUADS,
        "a frame's runs past its room double it"
    );
    assert_eq!(
        arena
            .left
            .iter()
            .map(|left| (left.quads, left.staged))
            .collect::<Vec<_>>(),
        [(9_000, 1)],
        "the buffer left moves the quads in use when it was left"
    );

    drop(dropped);
    arena.begin_frame(&device, 12_000);
    assert_eq!(
        arena.spans.capacity(),
        26 * MIN_QUADS,
        "a buffer under twice its room stays"
    );
    drop(kept);
    arena.begin_frame(&device, 0);
    assert_eq!(arena.spans.capacity(), MIN_QUADS);
    assert!(arena.store.is_some());
}
