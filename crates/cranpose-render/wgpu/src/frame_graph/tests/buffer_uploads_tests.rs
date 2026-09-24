use super::*;

#[test]
fn discarded_copies_and_reused_staging_keep_destination_offsets() {
    let (_lock, device, queue) = super::super::upload_test_device();
    let destination = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut uploads = BufferUploads::default();
    let mut expected = [0u8; 16];
    for frame in 1..=24u8 {
        let mut abandoned = device.create_command_encoder(&Default::default());
        uploads.write(&device, &mut abandoned, &destination, 0, &[255; 4]);
        drop(abandoned);
        uploads.reset();

        let mut encoder = device.create_command_encoder(&Default::default());
        let offset = 4 + usize::from(frame % 3) * 4;
        expected[offset..offset + 4].fill(frame);
        uploads.write(
            &device,
            &mut encoder,
            &destination,
            offset as u64,
            &[frame; 4],
        );
        encoder.copy_buffer_to_buffer(&destination, 0, &readback, 0, 16);
        uploads.finish();
        let submission = queue.submit([encoder.finish()]);
        uploads.recall();
        assert_eq!(
            super::super::read_uploaded_bytes(&device, &readback, submission),
            expected
        );
        uploads.reset();
    }
}
