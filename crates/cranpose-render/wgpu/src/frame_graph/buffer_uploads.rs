const STAGING_CHUNK_BYTES: u64 = 256 * 1024;
const SHRINK_FACTOR: u64 = 4;

#[derive(Default)]
pub(crate) struct BufferUploads {
    belt: Option<wgpu::util::StagingBelt>,
    frame_bytes: u64,
    retained_workload: u64,
}

impl BufferUploads {
    pub(crate) fn write(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        destination: &wgpu::Buffer,
        offset: u64,
        bytes: &[u8],
    ) -> u64 {
        let Some(size) = wgpu::BufferSize::new(bytes.len() as u64) else {
            return 0;
        };
        self.belt
            .get_or_insert_with(|| {
                wgpu::util::StagingBelt::new(device.clone(), STAGING_CHUNK_BYTES)
            })
            .write_buffer(encoder, destination, offset, size)
            .copy_from_slice(bytes);
        self.frame_bytes += size.get();
        size.get()
    }

    pub(crate) fn finish(&mut self) {
        if let Some(belt) = &mut self.belt {
            belt.finish();
        }
    }

    pub(crate) fn recall(&mut self) {
        if let Some(belt) = &mut self.belt {
            belt.recall();
        }
    }

    pub(crate) fn reset(&mut self) {
        self.finish();
        self.recall();
        if self.frame_bytes.saturating_mul(SHRINK_FACTOR) < self.retained_workload {
            self.belt = None;
            self.retained_workload = self.frame_bytes;
        } else {
            self.retained_workload = self.retained_workload.max(self.frame_bytes);
        }
        self.frame_bytes = 0;
    }
}

#[cfg(test)]
mod tests {
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
}
