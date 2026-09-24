use super::{
    FrameTextureDescriptor, MIN_UPLOAD_BUFFER_BYTES, UploadPlacement, WgpuFrameGraph,
    build_pass_schedule, place_upload, ring_outlives_frame,
};

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
fn frame_uploads_grow_preserve_bytes_and_release_oversized_generations() {
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
    assert_eq!(ring.generations.len(), 3);
    ring.flush(&queue);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 48,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    for (index, (source, offset)) in sources.into_iter().enumerate() {
        encoder.copy_buffer_to_buffer(&source, offset, &readback, index as u64 * 16, 16);
    }
    let submission = queue.submit([encoder.finish()]);
    assert_eq!(
        super::read_uploaded_bytes(&device, &readback, submission),
        [&small[..], &large[..16], &last[..]].concat()
    );
    assert_eq!(ring.generations.len(), 1);
    ring.upload(&device, 16, &small);
    ring.flush(&queue);
    assert!(
        ring.generations.is_empty(),
        "a small frame must release the oversized generation"
    );
    ring.upload(&device, 16, &last);
    assert_eq!(ring.generations[0].capacity, MIN_UPLOAD_BUFFER_BYTES);
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
