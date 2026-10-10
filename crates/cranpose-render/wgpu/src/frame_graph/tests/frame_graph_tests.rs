use super::{
    FrameTextureDescriptor, MIN_RETAINED_TRANSIENT_BYTES, MIN_UPLOAD_BUFFER_BYTES, UploadPlacement,
    WgpuFrameGraph, WgpuFrameGraphExecutor, build_pass_schedule, place_upload, ring_outlives_frame,
};
use crate::{idle_pool::IDLE_FRAMES, offscreen::OffscreenTarget};

#[test]
fn a_transient_texture_no_frame_reuses_is_dropped_after_the_idle_frames() {
    let (_lock, device, _queue) = super::upload_test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let descriptor = FrameTextureDescriptor::render_attachment("idle test", 8, 8, format);
    let mut executor = WgpuFrameGraphExecutor::default();
    executor.return_cached_transient(descriptor, OffscreenTarget::new(&device, format, 8, 8));
    for _ in 0..IDLE_FRAMES {
        executor.end_transient_frame();
    }
    assert_eq!(executor.retained_texture_count(), 1);
    executor.end_transient_frame();
    assert_eq!(executor.retained_texture_count(), 0);
    assert_eq!(executor.retained_texture_bytes(), 0);
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn exact(width: u32, height: u32) -> FrameTextureDescriptor {
    FrameTextureDescriptor::render_attachment("exact test", width, height, FORMAT)
}

/// Acquires `descriptor` from the executor's pool, returning the texture and
/// whether the pool had to create it.
fn acquire(
    executor: &mut WgpuFrameGraphExecutor,
    device: &wgpu::Device,
    descriptor: FrameTextureDescriptor,
) -> (crate::offscreen::OffscreenTarget, bool) {
    let target = executor.transient_textures.acquire(device, descriptor);
    let (_, news) = executor.transient_textures.take_counts();
    (target, news > 0)
}

#[test]
fn exact_requests_reuse_only_matching_dimensions() {
    let (_lock, device, _queue) = super::upload_test_device();
    let mut executor = WgpuFrameGraphExecutor::default();
    executor.return_cached_transient(exact(96, 64), OffscreenTarget::new(&device, FORMAT, 96, 64));
    executor.return_cached_transient(exact(64, 40), OffscreenTarget::new(&device, FORMAT, 64, 40));
    let (target, created) = acquire(&mut executor, &device, exact(64, 40));
    assert!(!created, "the matching size is reused");
    assert_eq!((target.width, target.height), (64, 40));
    let (_, created) = acquire(&mut executor, &device, exact(60, 36));
    assert!(
        created,
        "a larger pooled texture cannot serve a smaller request"
    );
    let (target, created) = acquire(&mut executor, &device, exact(96, 64));
    assert!(!created, "the remaining matching size is reused");
    assert_eq!((target.width, target.height), (96, 64));
}

#[test]
fn an_exact_request_takes_only_its_own_size() {
    let (_lock, device, _queue) = super::upload_test_device();
    let mut executor = WgpuFrameGraphExecutor::default();
    executor.return_cached_transient(exact(64, 40), OffscreenTarget::new(&device, FORMAT, 64, 40));
    let (target, created) = acquire(&mut executor, &device, exact(60, 36));
    assert!(
        created,
        "a whole-texture reader must not get a larger texture"
    );
    assert_eq!((target.width, target.height), (60, 36));
    let (_, created) = acquire(&mut executor, &device, exact(64, 40));
    assert!(!created);
}

#[test]
fn equal_area_does_not_make_different_dimensions_reusable() {
    let (_lock, device, _queue) = super::upload_test_device();
    let mut executor = WgpuFrameGraphExecutor::default();
    executor.return_cached_transient(
        exact(128, 64),
        OffscreenTarget::new(&device, FORMAT, 128, 64),
    );
    let (_, created) = acquire(&mut executor, &device, exact(64, 128));
    assert!(
        created,
        "matching total texels do not make dimensions interchangeable"
    );
}

#[test]
fn a_texture_goes_back_to_the_pool_at_its_own_size() {
    let (_lock, device, _queue) = super::upload_test_device();
    let mut executor = WgpuFrameGraphExecutor::default();
    executor.return_cached_transient(exact(80, 50), OffscreenTarget::new(&device, FORMAT, 96, 64));
    let (target, created) = acquire(&mut executor, &device, exact(96, 64));
    assert!(
        !created,
        "the pool records the target's 96x64 dimensions, not the stale descriptor"
    );
    assert_eq!((target.width, target.height), (96, 64));
}

#[test]
fn the_pool_keeps_a_frame_s_textures_past_the_floor() {
    let (_lock, device, _queue) = super::upload_test_device();
    let mut executor = WgpuFrameGraphExecutor::default();
    let side = 2048;
    let texture_bytes = u64::from(side) * u64::from(side) * 4;
    let count = MIN_RETAINED_TRANSIENT_BYTES / texture_bytes + 2;
    let descriptors: Vec<_> = (0..count)
        .map(|index| exact(side, side - index as u32))
        .collect();
    let first: Vec<_> = descriptors
        .iter()
        .map(|descriptor| acquire(&mut executor, &device, *descriptor).0)
        .collect();
    for (descriptor, target) in descriptors.iter().zip(first) {
        executor.return_cached_transient(*descriptor, target);
    }
    executor.end_transient_frame();
    assert!(executor.retained_texture_bytes() > MIN_RETAINED_TRANSIENT_BYTES);
    for descriptor in &descriptors {
        let (_, created) = acquire(&mut executor, &device, *descriptor);
        assert!(
            !created,
            "the second frame reuses every texture the first frame used"
        );
    }
}

#[test]
fn a_texture_a_cache_returns_counts_toward_the_frame_s_working_set() {
    let (_lock, device, _queue) = super::upload_test_device();
    let mut executor = WgpuFrameGraphExecutor::default();
    let side = 2048;
    let texture_bytes = u64::from(side) * u64::from(side) * 4;
    let frame_count = MIN_RETAINED_TRANSIENT_BYTES / texture_bytes;
    let frame: Vec<_> = (0..frame_count)
        .map(|index| exact(side, side - index as u32))
        .collect();
    let cached = exact(side, side - frame_count as u32);
    let acquired: Vec<_> = frame
        .iter()
        .map(|descriptor| acquire(&mut executor, &device, *descriptor).0)
        .collect();
    for (descriptor, target) in frame.iter().zip(acquired) {
        executor.transient_textures.release(*descriptor, target);
    }
    executor.return_cached_transient(
        cached,
        OffscreenTarget::new(&device, FORMAT, cached.width, cached.height),
    );
    executor.end_transient_frame();
    for descriptor in frame.iter().chain(std::iter::once(&cached)) {
        let (_, created) = acquire(&mut executor, &device, *descriptor);
        assert!(
            !created,
            "the frame used its own textures and the one its cache held; the pool keeps all"
        );
    }
}

#[test]
fn a_copied_ring_outlives_the_frames_that_fill_a_quarter_of_it() {
    let capacity = 16 * MIN_UPLOAD_BUFFER_BYTES;
    let copied = super::UploadMode::Copied;
    assert!(ring_outlives_frame(capacity, capacity / 4, copied));
    assert!(ring_outlives_frame(capacity, capacity, copied));
    assert!(!ring_outlives_frame(capacity, capacity / 4 - 1, copied));
    assert!(!ring_outlives_frame(capacity, 1, copied));
}

#[test]
fn a_mapped_ring_outlives_the_frames_that_fill_half_of_it() {
    let capacity = 16 * MIN_UPLOAD_BUFFER_BYTES;
    let mapped = super::UploadMode::Mapped;
    assert!(ring_outlives_frame(capacity, capacity / 2, mapped));
    assert!(!ring_outlives_frame(capacity, capacity / 2 - 1, mapped));
}

#[test]
fn a_ring_at_the_floor_and_a_ring_after_an_empty_frame_stay() {
    for mode in [super::UploadMode::Copied, super::UploadMode::Mapped] {
        assert!(ring_outlives_frame(MIN_UPLOAD_BUFFER_BYTES, 1, mode));
        assert!(ring_outlives_frame(16 * MIN_UPLOAD_BUFFER_BYTES, 0, mode));
    }
}

/// The modes the test device offers: the copied one always, the mapped
/// one when the device maps its primary buffers.
fn upload_modes(device: &wgpu::Device) -> Vec<super::UploadMode> {
    let mut modes = vec![super::UploadMode::Copied];
    if super::UploadMode::for_device(device) == super::UploadMode::Mapped {
        modes.push(super::UploadMode::Mapped);
    }
    modes
}

/// Runs the frame's copies and then reads 16 bytes at each source's
/// offset, in order.
fn read_sources(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    belt: &mut super::BufferUploads,
    sources: &[(wgpu::Buffer, u64)],
) -> Vec<u8> {
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16 * sources.len() as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    for (index, (source, offset)) in sources.iter().enumerate() {
        encoder.copy_buffer_to_buffer(source, *offset, &readback, index as u64 * 16, 16);
    }
    belt.flush_queued(device, queue);
    belt.finish();
    let copies = belt.take_before_passes().map(wgpu::CommandEncoder::finish);
    let submission = queue.submit(copies.into_iter().chain([encoder.finish()]));
    belt.recall();
    super::read_uploaded_bytes(device, &readback, submission)
}

/// Stages the ring's copied bytes through `writer` and ends its frame.
fn stage_through(
    writer: &impl super::BeforePassWriter,
    ring: &mut super::UploadRing,
    belt: &mut super::BufferUploads,
) {
    ring.stage_pending(&mut |buffer, offset, bytes| {
        super::write_before_passes(writer, belt, buffer, offset, bytes)
    });
    ring.finish_frame();
}

#[test]
fn frame_uploads_preserve_bytes_across_growth_and_frames() {
    let (_lock, device, queue) = super::upload_test_device();
    for mode in upload_modes(&device) {
        check_staged_uploads(&device, &queue, mode, &device);
    }
    // The web's frame encoder writes through the queue.
    check_staged_uploads(&device, &queue, super::UploadMode::Copied, &queue);
}

fn check_staged_uploads(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mode: super::UploadMode,
    writer: &impl super::BeforePassWriter,
) {
    let mut belt = super::BufferUploads::default();
    let mut ring = super::UploadRing::new(
        wgpu::BufferUsages::COPY_SRC,
        "Upload Growth Test",
        256,
        true,
        mode,
    );
    let large = vec![2; MIN_UPLOAD_BUFFER_BYTES as usize * 4 + 4];
    let small = [1; 16];
    let last = [3; 16];
    let mut sources = Vec::new();
    for bytes in [&small[..], &large, &last[..]] {
        let (generation, offset) = ring.stage(device, bytes.len() as u64, bytes);
        sources.push((ring.generations[generation].buffer.clone(), offset));
    }
    stage_through(writer, &mut ring, &mut belt);
    assert_eq!(
        read_sources(device, queue, &mut belt, &sources),
        [&small[..], &large[..16], &last[..]].concat(),
        "{mode:?}"
    );
    ring.recall();
    // A second frame fits one buffer; the third writes the same buffer:
    // a copied ring kept it, a mapped ring got it back once the GPU
    // was done with it.
    let next = vec![4; 300_000];
    let mut buffers = Vec::new();
    for _ in 0..2 {
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("idle device");
        ring.reset();
        let (generation, offset) = ring.stage(device, 16, &next);
        let buffer = ring.generations[generation].buffer.clone();
        stage_through(writer, &mut ring, &mut belt);
        assert_eq!(
            read_sources(device, queue, &mut belt, &[(buffer.clone(), offset)]),
            next[..16],
            "{mode:?}"
        );
        ring.recall();
        buffers.push(buffer);
    }
    assert_eq!(
        buffers[0], buffers[1],
        "{mode:?}: the third frame writes the second frame's buffer"
    );
}

#[test]
fn vertex_uploads_written_as_they_come_land_whole_across_growth() {
    let (_lock, device, queue) = super::upload_test_device();
    for mode in upload_modes(&device) {
        check_written_uploads(&device, &queue, mode, &device, false);
    }
    // The web's frame encoder writes through the queue, alone on WebGL and
    // gathered into one write in a browser's WebGPU.
    for batched in [false, true] {
        check_written_uploads(&device, &queue, super::UploadMode::Copied, &queue, batched);
    }
}

fn check_written_uploads(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mode: super::UploadMode,
    writer: &impl super::BeforePassWriter,
    batched: bool,
) {
    // The frame's rings, readable back for the test.
    let readable = wgpu::BufferUsages::COPY_SRC;
    let mut allocators = super::FrameUploadAllocators {
        rings: Some([
            super::UploadRing::new(readable, "Uniforms", 256, true, mode),
            super::UploadRing::new(
                readable,
                "Vertices",
                wgpu::COPY_BUFFER_ALIGNMENT,
                false,
                mode,
            ),
            super::UploadRing::new(
                readable,
                "Indices",
                wgpu::COPY_BUFFER_ALIGNMENT,
                false,
                mode,
            ),
        ]),
        ..super::FrameUploadAllocators::default()
    };
    allocators.buffers.batch_queued(batched);
    let odd: Vec<u8> = (1..=13).collect();
    let large = vec![2; MIN_UPLOAD_BUFFER_BYTES as usize * 2];
    let last = [3; 16];
    let spec = super::UploadAllocatorSpec::vertex("Direct Upload Test", 0);
    let uploads: Vec<_> = [&odd[..], &large, &last[..]]
        .into_iter()
        .map(|bytes| allocators.upload_buffer(spec, device, bytes, writer))
        .collect();
    let stats = allocators.encode_pending(writer);
    allocators.buffers.finish();
    allocators.finish_frame();
    assert_eq!(
        stats.upload_writes, 3,
        "{mode:?}: each upload is written once"
    );
    let sources: Vec<_> = uploads
        .iter()
        .map(|upload| (upload.buffer.clone(), upload.offset))
        .collect();
    let mut odd_word = odd;
    odd_word.extend([0, 0, 0]);
    assert_eq!(
        read_sources(device, queue, &mut allocators.buffers, &sources),
        [&odd_word[..], &large[..16], &last[..]].concat(),
        "{mode:?}"
    );
    allocators.recall();
}

#[test]
fn an_upload_lands_aligned_after_the_last_one() {
    assert_eq!(
        place_upload(100, 64, 64, 256, Some(4096)),
        UploadPlacement::At(256)
    );
    assert_eq!(
        place_upload(0, 12, 0, 4, Some(4096)),
        UploadPlacement::At(0)
    );
    assert_eq!(
        place_upload(12, 12, 0, 4, Some(4096)),
        UploadPlacement::At(12)
    );
}

#[test]
fn an_upload_past_the_buffer_opens_one_at_least_twice_as_large() {
    assert_eq!(
        place_upload(99_950, 64, 64, 256, Some(100_000)),
        UploadPlacement::Grow(200_000)
    );
    assert_eq!(
        place_upload(0, 64, 64, 256, None),
        UploadPlacement::Grow(MIN_UPLOAD_BUFFER_BYTES)
    );
    assert_eq!(
        place_upload(4000, 64, 64, 256, Some(4096)),
        UploadPlacement::Grow(MIN_UPLOAD_BUFFER_BYTES)
    );
    assert_eq!(
        place_upload(
            0,
            3 * MIN_UPLOAD_BUFFER_BYTES,
            64,
            256,
            Some(MIN_UPLOAD_BUFFER_BYTES)
        ),
        UploadPlacement::Grow(3 * MIN_UPLOAD_BUFFER_BYTES)
    );
}

#[test]
fn a_binding_wider_than_its_bytes_reserves_the_binding() {
    assert_eq!(
        place_upload(98_000, 16, 1024, 64, Some(100_000)),
        UploadPlacement::At(98_048)
    );
    assert_eq!(
        place_upload(99_000, 16, 1024, 64, Some(100_000)),
        UploadPlacement::Grow(200_000)
    );
}

#[test]
fn pass_schedule_orders_reads_after_last_writer() {
    let mut graph = WgpuFrameGraph::new(None);
    let target = graph.import_surface("surface");
    graph.add_fallible_command_pass(Some("writer"), &[], &[target], |_| Ok(()));
    graph.add_fallible_command_pass(Some("independent"), &[], &[], |_| Ok(()));
    graph.add_fallible_command_pass(Some("reader"), &[target], &[], |_| Ok(()));

    let order = build_pass_schedule(&graph.passes).expect("valid pass schedule");
    let writer_index = order
        .iter()
        .position(|index| *index == 0)
        .expect("writer pass should be scheduled");
    let reader_index = order
        .iter()
        .position(|index| *index == 2)
        .expect("reader pass should be scheduled");

    assert!(writer_index < reader_index);
}

#[test]
fn pass_schedule_keeps_later_writes_after_earlier_reads() {
    let mut graph = WgpuFrameGraph::new(None);
    let target = graph.import_surface("surface");
    let dependency = graph.import_surface("dependency");
    graph.add_fallible_command_pass(Some("dependency writer"), &[], &[dependency], |_| Ok(()));
    graph.add_fallible_command_pass(
        Some("target reader"),
        &[target, dependency],
        &[],
        |_| Ok(()),
    );
    graph.add_fallible_command_pass(Some("target writer"), &[], &[target], |_| Ok(()));

    let order = build_pass_schedule(&graph.passes).expect("valid pass schedule");
    let reader_index = order
        .iter()
        .position(|index| *index == 1)
        .expect("reader pass should be scheduled");
    let writer_index = order
        .iter()
        .position(|index| *index == 2)
        .expect("writer pass should be scheduled");

    assert!(reader_index < writer_index);
}

#[test]
fn transient_texture_descriptor_clamps_and_accounts_bytes() {
    let descriptor =
        FrameTextureDescriptor::render_attachment("scratch", 0, 2, wgpu::TextureFormat::Bgra8Unorm);

    assert_eq!(descriptor.width, 1);
    assert_eq!(descriptor.height, 2);
    assert_eq!(descriptor.estimated_bytes(), 8);
}

#[test]
fn frame_graph_records_imported_texture_resources() {
    let mut graph = WgpuFrameGraph::new(None);
    let handle = graph.import_surface("surface");

    assert_eq!(handle.0, 0);
    assert_eq!(graph.resources.textures[0].label, "surface");
}

#[test]
fn command_nodes_declare_wgpu_pass_count() {
    let mut graph = WgpuFrameGraph::new(None);
    let source = graph.import_surface("source");
    let dest = graph.import_surface("dest");

    graph.add_fallible_command_pass(Some("copy"), &[source], &[dest], |_| Ok(()));

    assert_eq!(graph.node_count(), 1);
    assert_eq!(graph.declared_pass_count(), 1);
}
