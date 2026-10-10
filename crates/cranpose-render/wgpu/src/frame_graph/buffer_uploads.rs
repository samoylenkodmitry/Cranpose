const STAGING_CHUNK_BYTES: u64 = 256 * 1024;
const SHRINK_FACTOR: u64 = 4;

#[derive(Default)]
pub(crate) struct BufferUploads {
    belt: Option<wgpu::util::StagingBelt>,
    before_passes: Option<wgpu::CommandEncoder>,
    frame_bytes: u64,
    retained_workload: u64,
    #[cfg(any(target_arch = "wasm32", test))]
    queued: QueuedWrites,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Default)]
pub(crate) struct QueuedWrites {
    batched: bool,
    bytes: Vec<u8>,
    copies: Vec<QueuedCopy>,
    buffer: Option<wgpu::Buffer>,
    retained_workload: u64,
}

#[cfg(any(target_arch = "wasm32", test))]
struct QueuedCopy {
    destination: wgpu::Buffer,
    offset: u64,
    at: u64,
    len: u64,
}

#[cfg(any(target_arch = "wasm32", test))]
impl QueuedWrites {
    pub(crate) fn stage(&mut self, destination: &wgpu::Buffer, offset: u64, bytes: &[u8]) -> u64 {
        if bytes.is_empty() {
            return 0;
        }
        let at = self.bytes.len() as u64;
        self.bytes.extend_from_slice(bytes);
        match self.copies.last_mut() {
            Some(last)
                if last.destination == *destination
                    && last.offset + last.len == offset
                    && last.at + last.len == at =>
            {
                last.len += bytes.len() as u64;
            }
            _ => self.copies.push(QueuedCopy {
                destination: destination.clone(),
                offset,
                at,
                len: bytes.len() as u64,
            }),
        }
        bytes.len() as u64
    }

    pub(crate) fn flush(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        if self.copies.is_empty() {
            return;
        }
        let len = self.bytes.len() as u64;
        let fits = self.buffer.as_ref().is_some_and(|buffer| {
            buffer.size() >= len && len.saturating_mul(SHRINK_FACTOR) >= self.retained_workload
        });
        if !fits {
            self.retained_workload = len;
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Frame Queued Writes"),
                size: align_up(len, STAGING_CHUNK_BYTES),
                usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        self.retained_workload = self.retained_workload.max(len);
        let Some(buffer) = self.buffer.as_ref() else {
            return;
        };
        queue.write_buffer(buffer, 0, &self.bytes);
        for copy in self.copies.drain(..) {
            encoder.copy_buffer_to_buffer(
                buffer,
                copy.at,
                &copy.destination,
                copy.offset,
                copy.len,
            );
        }
        self.bytes.clear();
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn align_up(value: u64, step: u64) -> u64 {
    value.div_ceil(step).max(1) * step
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

    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn batch_queued(&mut self, batched: bool) {
        self.queued.batched = batched;
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn flush_queued(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.queued.copies.is_empty() {
            return;
        }
        let Self {
            before_passes,
            queued,
            ..
        } = self;
        queued.flush(device, queue, before_passes_in(before_passes, device));
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
/// the unmap copies it again, while `writeBuffer` reads the bytes from wasm
/// memory once. In a browser's WebGPU the frame's writes are gathered into
/// one, which [`BufferUploads::flush_queued`] copies into place ahead of the
/// frame's passes: Chrome passes a write of more than 4 MiB through shared
/// memory of its own at memory speed, while smaller writes take turns in its
/// command transfer buffer, where a frame's megabytes written in pieces cost
/// the page's thread milliseconds. WebGL copies into no index buffer from
/// another buffer, so there each write goes alone.
#[cfg(any(target_arch = "wasm32", test))]
impl BeforePassWriter for wgpu::Queue {
    fn write_before_passes(
        &self,
        belt: &mut BufferUploads,
        destination: &wgpu::Buffer,
        offset: u64,
        bytes: &[u8],
    ) -> u64 {
        if belt.queued.batched {
            return belt.queued.stage(destination, offset, bytes);
        }
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
