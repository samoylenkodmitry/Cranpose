use super::*;
use crate::frame_graph::upload_test_device;

fn quads(count: usize) -> Vec<GlyphInstance> {
    vec![bytemuck::Zeroable::zeroed(); count]
}

fn copied_arena() -> GlyphRunArena {
    GlyphRunArena::new(UploadMode::Copied)
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
    let mut arena = copied_arena();
    assert!(arena.insert(&device, quads(0)).is_none());
    assert!(arena.chunks.is_empty());
    assert!(arena.staged_instances.is_empty());
}

#[test]
fn a_dropped_runs_quads_stay_taken_until_the_next_frame() {
    let (_lock, device, _queue) = upload_test_device();
    let mut arena = copied_arena();
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
    let mut arena = copied_arena();
    let small = arena
        .insert(&device, quads(MIN_CHUNK_QUADS as usize))
        .expect("a run");
    let next = arena.insert(&device, quads(1)).expect("a run");
    let capacities: Vec<u32> = arena.chunks.iter().map(|c| c.spans.capacity()).collect();
    assert_eq!(capacities, [MIN_CHUNK_QUADS, MIN_CHUNK_QUADS * 2]);

    let huge = arena
        .insert(&device, quads(MAX_COPIED_CHUNK_QUADS as usize + 1))
        .expect("a run");
    assert_eq!(arena.chunks.len(), 3);
    assert_eq!(huge.instances(), 0..MAX_COPIED_CHUNK_QUADS + 1);

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

fn filled(count: usize, byte: u8) -> Vec<GlyphInstance> {
    let mut quads = quads(count);
    bytemuck::cast_slice_mut::<_, u8>(&mut quads).fill(byte);
    quads
}

/// Runs one frame of `arena` in a frame graph pass: `body` inserts runs,
/// the frame is staged, and each of `read`'s spans is copied out after.
fn arena_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    arena: &mut GlyphRunArena,
    body: impl FnOnce(&mut GlyphRunArena) -> Vec<GlyphRunSpan>,
) -> Vec<(GlyphRunSpan, Vec<u8>)> {
    arena.begin_frame();
    let mut spans = Vec::new();
    let mut readbacks = Vec::new();
    let mut executor = crate::frame_graph::WgpuFrameGraphExecutor::new();
    let mut graph = crate::frame_graph::WgpuFrameGraph::new(None);
    graph.add_fallible_command_pass(Some("glyph runs"), &[], &[], |context| {
        spans = body(arena);
        arena.stage_pending(device, context);
        for span in &spans {
            let bytes = instance_offset(span.instances().len() as u32);
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: bytes,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            context.encoder.copy_buffer_to_buffer(
                span.instance_buffer(),
                instance_offset(span.instances().start),
                &readback,
                0,
                bytes,
            );
            readbacks.push(readback);
        }
        Ok(())
    });
    let submission = executor
        .execute_recorded_graph(device, queue, graph)
        .expect("the frame submits")
        .submission;
    spans
        .into_iter()
        .zip(readbacks)
        .map(|(span, readback)| {
            let read =
                crate::frame_graph::read_uploaded_bytes(device, &readback, submission.clone());
            (span, read)
        })
        .collect()
}

#[test]
fn a_mapped_arena_writes_each_run_whole_and_leaves_earlier_frames_runs_alone() {
    let (_lock, device, queue) = upload_test_device();
    if UploadMode::for_device(&device) != UploadMode::Mapped {
        return;
    }
    let mut arena = GlyphRunArena::new(UploadMode::Mapped);
    let first = arena_frame(&device, &queue, &mut arena, |arena| {
        [(300, 1), (40, 2)]
            .map(|(count, byte)| arena.insert(&device, filled(count, byte)).expect("a run"))
            .into_iter()
            .collect()
    });
    for ((_, read), byte) in first.iter().zip([1, 2]) {
        assert!(read.iter().all(|read| *read == byte), "a run lands whole");
    }
    let [(dropped, _), (kept, _)] = <[_; 2]>::try_from(first).ok().expect("two runs");
    drop(dropped);
    let mut kept = Some(kept);
    // Later frames write new runs while the first frame's chunk holds a
    // run its draws read: the kept run's quads must stay as written.
    for frame in 0..3u8 {
        let read = arena_frame(&device, &queue, &mut arena, |arena| {
            let mut spans: Vec<GlyphRunSpan> = kept.take().into_iter().collect();
            spans.push(
                arena
                    .insert(&device, filled(500, 10 + frame))
                    .expect("a run"),
            );
            spans
        });
        assert!(
            read[0].1.iter().all(|byte| *byte == 2),
            "frame {frame}: the kept run"
        );
        assert!(
            read[1].1.iter().all(|byte| *byte == 10 + frame),
            "frame {frame}: the new run"
        );
        let mut runs = read.into_iter().map(|(span, _)| span);
        kept = runs.next();
    }
}
