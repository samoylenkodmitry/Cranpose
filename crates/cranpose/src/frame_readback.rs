use std::{sync::mpsc, time::Duration};

use crate::embedded_view::EmbeddedViewError;

const BYTES_PER_PIXEL: u32 = 4;
const READBACK_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct FrameTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    readback: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_row_bytes: u32,
}

impl FrameTarget {
    pub(crate) fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Embed Frame Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let padded_row_bytes = padded_row_bytes(width);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Embed Frame Readback"),
            size: u64::from(padded_row_bytes) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self {
            texture,
            view,
            readback,
            width,
            height,
            padded_row_bytes,
        }
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub(crate) fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    pub(crate) fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    pub(crate) fn read_into(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pixels: &mut Vec<u8>,
    ) -> Result<(), EmbeddedViewError> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Embed Frame Readback Encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row_bytes),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        let submission = queue.submit(std::iter::once(encoder.finish()));

        let slice = self.readback.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        let result = (|| {
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(submission),
                    timeout: Some(READBACK_TIMEOUT),
                })
                .map_err(|error| EmbeddedViewError::Readback(error.to_string()))?;
            receiver
                .recv_timeout(READBACK_TIMEOUT)
                .map_err(|error| EmbeddedViewError::Readback(error.to_string()))?
                .map_err(|error| EmbeddedViewError::Readback(error.to_string()))?;

            let mapped = slice
                .get_mapped_range()
                .map_err(|error| EmbeddedViewError::Readback(error.to_string()))?;
            copy_unpadded_rows(
                &mapped,
                self.padded_row_bytes as usize,
                (self.width * BYTES_PER_PIXEL) as usize,
                pixels,
            );
            Ok(())
        })();
        self.readback.unmap();
        result
    }
}

pub(crate) fn padded_row_bytes(width: u32) -> u32 {
    (width * BYTES_PER_PIXEL).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
}

pub(crate) fn copy_unpadded_rows(
    padded: &[u8],
    padded_row_bytes: usize,
    row_bytes: usize,
    pixels: &mut Vec<u8>,
) {
    pixels.clear();
    pixels.reserve(padded.len().div_ceil(padded_row_bytes) * row_bytes);
    for row in padded.chunks(padded_row_bytes) {
        pixels.extend_from_slice(&row[..row_bytes.min(row.len())]);
    }
}
