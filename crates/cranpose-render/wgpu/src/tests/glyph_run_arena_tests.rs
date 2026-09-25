use super::*;
use crate::frame_graph::{take_upload_write_calls, upload_test_device};

fn quads(count: usize) -> Vec<[Vertex; VERTICES_PER_QUAD]> {
    vec![[bytemuck::Zeroable::zeroed(); VERTICES_PER_QUAD]; count]
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
fn quad_indices_draw_two_triangles_over_four_corners() {
    assert_eq!(quad_indices(0), [0, 1, 2, 2, 1, 3]);
    assert_eq!(quad_indices(3), [12, 13, 14, 14, 13, 15]);
}

#[test]
fn a_frames_runs_reach_the_gpu_in_one_write() {
    let (_lock, device, queue) = upload_test_device();
    let mut arena = GlyphRunArena::default();
    let first = arena.insert(&device, quads(3)).expect("a run of quads");
    let second = arena.insert(&device, quads(5)).expect("a run of quads");
    assert_eq!(first.indices(), 0..18);
    assert_eq!(second.indices(), 18..48);
    assert!(first.vertex_buffer() == second.vertex_buffer());
    assert!(arena.index_buffer().is_some());

    take_upload_write_calls();
    let stats = arena.flush(&queue);
    assert_eq!(take_upload_write_calls(), 1);
    assert_eq!(stats.upload_bytes, vertex_offset(8));
    assert_eq!(arena.flush(&queue).upload_bytes, 0, "a flush writes once");
}

#[test]
fn an_arena_takes_no_run_without_quads() {
    let (_lock, device, _queue) = upload_test_device();
    let mut arena = GlyphRunArena::default();
    assert!(arena.insert(&device, quads(0)).is_none());
    assert!(arena.index_buffer().is_none());
    assert!(arena.staged_vertices.is_empty());
}

#[test]
fn a_dropped_runs_quads_stay_taken_until_the_next_frame() {
    let (_lock, device, _queue) = upload_test_device();
    let mut arena = GlyphRunArena::default();
    let dropped = arena.insert(&device, quads(1)).expect("a run");
    let kept = arena.insert(&device, quads(1)).expect("a run");
    assert_eq!(dropped.indices(), 0..6);
    drop(dropped);
    let same_frame = arena.insert(&device, quads(1)).expect("a run");
    assert_eq!(
        same_frame.indices(),
        12..18,
        "a draw recorded this frame may still read the dropped quads"
    );

    arena.begin_frame();
    let next_frame = arena.insert(&device, quads(1)).expect("a run");
    assert_eq!(next_frame.indices(), 0..6);
    assert_eq!(arena.chunks.len(), 1);
    assert_eq!(kept.indices(), 6..12);
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
    assert!(arena.index_quads >= MIN_CHUNK_QUADS * 2);

    let huge = arena
        .insert(&device, quads(MAX_CHUNK_QUADS as usize + 1))
        .expect("a run");
    assert_eq!(arena.chunks.len(), 3);
    assert_eq!(huge.indices().end, (MAX_CHUNK_QUADS + 1) * INDICES_PER_QUAD);

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

#[test]
fn staged_runs_of_a_released_chunk_are_skipped() {
    let (_lock, device, queue) = upload_test_device();
    let mut arena = GlyphRunArena::default();
    let huge = arena
        .insert(&device, quads(MAX_CHUNK_QUADS as usize + 1))
        .expect("a run");
    drop(huge);
    arena.begin_frame();
    assert!(arena.chunks.is_empty(), "an oversized chunk is never kept");
    take_upload_write_calls();
    assert_eq!(arena.flush(&queue).upload_bytes, 0);
    assert_eq!(take_upload_write_calls(), 0);
}
