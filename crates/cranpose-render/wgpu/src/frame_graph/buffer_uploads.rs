const STAGING_CHUNK_BYTES: u64 = 256 * 1024;
const SHRINK_FACTOR: u64 = 4;

#[derive(Default)]
pub(crate) struct BufferUploads {
    belt: Option<wgpu::util::StagingBelt>,
    before_passes: Option<wgpu::CommandEncoder>,
    frame_bytes: u64,
    retained_workload: u64,
}

impl BufferUploads {
    pub(crate) fn write(
        &mut self,
        device: &wgpu::Device,
        encoder: Option<&mut wgpu::CommandEncoder>,
        destination: &wgpu::Buffer,
        offset: u64,
        bytes: &[u8],
    ) -> u64 {
        let Some(size) = wgpu::BufferSize::new(bytes.len() as u64) else {
            return 0;
        };
        let encoder = encoder.unwrap_or_else(|| before_passes_in(&mut self.before_passes, device));
        self.belt
            .get_or_insert_with(|| {
                wgpu::util::StagingBelt::new(device.clone(), STAGING_CHUNK_BYTES)
            })
            .write_buffer(encoder, destination, offset, size)
            .copy_from_slice(bytes);
        self.frame_bytes += size.get();
        size.get()
    }

    pub(crate) fn before_passes(&mut self, device: &wgpu::Device) -> &mut wgpu::CommandEncoder {
        before_passes_in(&mut self.before_passes, device)
    }

    pub(crate) fn take_before_passes(&mut self) -> Option<wgpu::CommandEncoder> {
        self.before_passes.take()
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
        self.before_passes = None;
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

fn before_passes_in<'a>(
    before_passes: &'a mut Option<wgpu::CommandEncoder>,
    device: &wgpu::Device,
) -> &'a mut wgpu::CommandEncoder {
    before_passes.get_or_insert_with(|| {
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Frame Buffer Uploads"),
        })
    })
}

#[cfg(test)]
#[path = "tests/buffer_uploads_tests.rs"]
mod tests;
