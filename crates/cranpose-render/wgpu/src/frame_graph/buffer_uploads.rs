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
        if bytes.is_empty() {
            return 0;
        }
        let encoder = encoder.unwrap_or_else(|| before_passes_in(&mut self.before_passes, device));
        let belt = self.belt.get_or_insert_with(|| {
            wgpu::util::StagingBelt::new(device.clone(), STAGING_CHUNK_BYTES)
        });
        // A write larger than a chunk would get a chunk of its own size,
        // which later writes of other sizes rarely fit: the belt kept every
        // such chunk, so pieces of one chunk keep all chunks reusable.
        let mut piece_offset = offset;
        for piece in bytes.chunks(STAGING_CHUNK_BYTES as usize) {
            let Some(size) = wgpu::BufferSize::new(piece.len() as u64) else {
                continue;
            };
            belt.write_buffer(encoder, destination, piece_offset, size)
                .copy_from_slice(piece);
            piece_offset += size.get();
        }
        let written = bytes.len() as u64;
        self.frame_bytes += written;
        written
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

/// What a frame's writes that land before its passes go through.
pub(crate) trait BeforePassWriter {
    /// Writes `bytes` at `offset` of `destination` for every pass of the
    /// frame's submit to read, and returns how many bytes it wrote.
    fn write_before_passes(
        &self,
        belt: &mut BufferUploads,
        destination: &wgpu::Buffer,
        offset: u64,
        bytes: &[u8],
    ) -> u64;
}

/// Through the staging belt, into the encoder the device makes for the
/// copies submitted ahead of the frame's passes.
impl BeforePassWriter for wgpu::Device {
    fn write_before_passes(
        &self,
        belt: &mut BufferUploads,
        destination: &wgpu::Buffer,
        offset: u64,
        bytes: &[u8],
    ) -> u64 {
        belt.write(self, None, destination, offset, bytes)
    }
}

/// Through the queue, whose writes land before every command buffer of its
/// next submit. The web's frame encoder writes so. On the web a mapped belt
/// chunk is a JS `ArrayBuffer`: each write copies wasm memory into it, and
/// the unmap copies it again. `writeBuffer` reads the bytes from wasm memory
/// once.
impl BeforePassWriter for wgpu::Queue {
    fn write_before_passes(
        &self,
        _belt: &mut BufferUploads,
        destination: &wgpu::Buffer,
        offset: u64,
        bytes: &[u8],
    ) -> u64 {
        if bytes.is_empty() {
            return 0;
        }
        self.write_buffer(destination, offset, bytes);
        bytes.len() as u64
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
