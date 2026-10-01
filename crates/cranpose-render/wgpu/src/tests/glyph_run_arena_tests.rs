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
    assert!(!spans.is_unused());
    spans.release(3..6);
    assert_eq!(spans.allocate(9), Some(0), "0..9 merged across both sides");
    spans.release(0..9);
    spans.release(9..12);
    assert!(spans.is_unused());
    assert_eq!(spans.allocate(12), Some(0));
}

#[test]
fn a_span_allocator_ignores_empty_and_foreign_spans() {
    let mut spans = SpanAllocator::new(4);
    assert_eq!(spans.allocate(4), Some(0));
    spans.release(2..2);
    spans.release(3..9);
    assert_eq!(spans.allocate(1), None);
    assert!(SpanAllocator::new(0).is_unused());
}

#[test]
fn an_arena_takes_no_run_without_quads() {
    let (_lock, device, _queue) = upload_test_device();
    let mut arena = GlyphRunArena::default();
    assert!(arena.insert(&device, quads(0)).is_none());
    assert!(arena.chunks.is_empty());
    assert!(arena.staged_instances.is_empty());
}

#[test]
fn a_dropped_runs_quads_stay_taken_until_the_next_frame() {
    let (_lock, device, _queue) = upload_test_device();
    let mut arena = GlyphRunArena::default();
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

    arena.begin_frame();
    let next_frame = arena.insert(&device, quads(1)).expect("a run");
    assert_eq!(next_frame.instances(), 0..1);
    assert_eq!(arena.chunks.len(), 1);
    assert_eq!(kept.instances(), 1..2);
}

#[test]
fn chunks_double_and_empty_ones_are_released() {
    let (_lock, device, _queue) = upload_test_device();
    let mut arena = GlyphRunArena::default();
    let small = arena
        .insert(&device, quads(MIN_CHUNK_QUADS as usize))
        .expect("a run");
    let next = arena.insert(&device, quads(1)).expect("a run");
    let capacities: Vec<u32> = arena.chunks.iter().map(|c| c.spans.capacity()).collect();
    assert_eq!(capacities, [MIN_CHUNK_QUADS, MIN_CHUNK_QUADS * 2]);

    let huge = arena
        .insert(&device, quads(MAX_CHUNK_QUADS as usize + 1))
        .expect("a run");
    assert_eq!(arena.chunks.len(), 3);
    assert_eq!(huge.instances(), 0..MAX_CHUNK_QUADS + 1);

    drop(small);
    drop(huge);
    arena.begin_frame();
    assert_eq!(
        arena.chunks.len(),
        1,
        "empty chunks go while one holds a run"
    );
    drop(next);
    arena.begin_frame();
    assert_eq!(
        arena.chunks.len(),
        1,
        "the last empty chunk stays for new runs"
    );
    assert!(arena.chunks[0].spans.is_unused());
}
