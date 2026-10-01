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

fn region(width: u32, height: u32) -> FrameTextureDescriptor {
    FrameTextureDescriptor::region_attachment("region test", width, height, FORMAT)
}

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
fn a_region_request_takes_the_smallest_pooled_texture_that_holds_it() {
    let (_lock, device, _queue) = super::upload_test_device();
    let mut executor = WgpuFrameGraphExecutor::default();
    executor.return_cached_transient(
        region(96, 64),
        OffscreenTarget::new(&device, FORMAT, 96, 64),
    );
    executor.return_cached_transient(
        region(64, 40),
        OffscreenTarget::new(&device, FORMAT, 64, 40),
    );
    let (target, created) = acquire(&mut executor, &device, region(60, 36));
    assert!(!created, "a pooled texture holds the request");
    assert_eq!((target.width, target.height), (64, 40));
    let (target, created) = acquire(&mut executor, &device, region(56, 30));
    assert!(!created, "the 96x64 texture is within four times 56x30");
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
fn a_region_request_leaves_a_texture_many_times_its_area() {
    let (_lock, device, _queue) = super::upload_test_device();
    let mut executor = WgpuFrameGraphExecutor::default();
    executor.return_cached_transient(
        region(128, 128),
        OffscreenTarget::new(&device, FORMAT, 128, 128),
    );
    let (_, created) = acquire(&mut executor, &device, region(60, 60));
    assert!(
        created,
        "a quarter of 128x128 is 64x64, so 60x60 takes its own texture"
    );
    let (_, created) = acquire(&mut executor, &device, region(64, 64));
    assert!(!created, "64x64 is a quarter of 128x128 and still served");
}

#[test]
fn a_texture_goes_back_to_the_pool_at_its_own_size() {
    let (_lock, device, _queue) = super::upload_test_device();
    let mut executor = WgpuFrameGraphExecutor::default();
    executor.return_cached_transient(
        region(96, 64),
        OffscreenTarget::new(&device, FORMAT, 96, 64),
    );
    let (target, _) = acquire(&mut executor, &device, region(80, 50));
    executor.return_cached_transient(region(80, 50), target);
    let (target, created) = acquire(&mut executor, &device, exact(96, 64));
    assert!(
        !created,
        "the pool records the 96x64 texture, not the 80x50 request"
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
fn a_ring_outlives_the_frames_that_fill_a_quarter_of_it() {
    let capacity = 16 * MIN_UPLOAD_BUFFER_BYTES;
    assert!(ring_outlives_frame(capacity, capacity / 4));
    assert!(ring_outlives_frame(capacity, capacity));
    assert!(!ring_outlives_frame(capacity, capacity / 4 - 1));
    assert!(!ring_outlives_frame(capacity, 1));
}

#[test]
fn a_ring_at_the_floor_and_a_ring_after_an_empty_frame_stay() {
    assert!(ring_outlives_frame(MIN_UPLOAD_BUFFER_BYTES, 1));
    assert!(ring_outlives_frame(16 * MIN_UPLOAD_BUFFER_BYTES, 0));
}

#[test]
fn frame_uploads_preserve_bytes_across_growth_and_reset() {
    let (_lock, device, queue) = super::upload_test_device();
    let mut ring = super::UploadRing::new(
        wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        "Upload Growth Test",
        256,
    );
    let large = vec![2; MIN_UPLOAD_BUFFER_BYTES as usize * 4 + 4];
    let small = [1; 16];
    let last = [3; 16];
    let mut sources = Vec::new();
    for bytes in [&small[..], &large, &last[..]] {
        let (generation, offset) = ring.upload(&device, bytes.len() as u64, bytes);
        sources.push((ring.generations[generation].buffer.clone(), offset));
    }
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut uploads = super::BufferUploads::default();
    ring.stage_pending(&mut |buffer, offset, bytes| super::FrameCommandStats {
        upload_bytes: uploads.write(&device, &mut encoder, buffer, offset, bytes),
        ..Default::default()
    });
    ring.reset();
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 48,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    for (index, (source, offset)) in sources.into_iter().enumerate() {
        encoder.copy_buffer_to_buffer(&source, offset, &readback, index as u64 * 16, 16);
    }
    uploads.finish();
    let submission = queue.submit([encoder.finish()]);
    uploads.recall();
    assert_eq!(
        super::read_uploaded_bytes(&device, &readback, submission),
        [&small[..], &large[..16], &last[..]].concat()
    );
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
