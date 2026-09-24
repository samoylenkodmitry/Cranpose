use std::{sync::mpsc, time::Duration};

use crate::{embed::EmbedError, embed_protocol::PixelRect};

pub(crate) const FRAME_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

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
    pub(crate) fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
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
            format: FRAME_FORMAT,
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
    ) -> Result<(), EmbedError> {
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
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(READBACK_TIMEOUT),
            })
            .map_err(|error| EmbedError::Readback(error.to_string()))?;
        receiver
            .recv_timeout(READBACK_TIMEOUT)
            .map_err(|error| EmbedError::Readback(error.to_string()))?
            .map_err(|error| EmbedError::Readback(error.to_string()))?;

        let mapped = slice
            .get_mapped_range()
            .map_err(|error| EmbedError::Readback(error.to_string()))?;
        copy_unpadded_rows(
            &mapped,
            self.padded_row_bytes as usize,
            (self.width * BYTES_PER_PIXEL) as usize,
            pixels,
        );
        drop(mapped);
        self.readback.unmap();
        Ok(())
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
    pixels.extend(
        padded
            .chunks(padded_row_bytes)
            .flat_map(|row| &row[..row_bytes.min(row.len())])
            .copied(),
    );
}

pub(crate) fn changed_rect(
    previous: &[u8],
    next: &[u8],
    width: u32,
    height: u32,
) -> Option<PixelRect> {
    let row_bytes = (width * BYTES_PER_PIXEL) as usize;
    if row_bytes == 0 || previous.len() != next.len() {
        return Some(PixelRect::full(width, height));
    }
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    let rows = previous
        .chunks_exact(row_bytes)
        .zip(next.chunks_exact(row_bytes));
    for (row, (before, after)) in rows.enumerate() {
        if before == after {
            continue;
        }
        let Some((first, last)) = changed_columns(before, after) else {
            continue;
        };
        let row = row as u32;
        bounds = Some(match bounds {
            None => (first, last, row, row),
            Some((first_column, last_column, first_row, _)) => (
                first_column.min(first),
                last_column.max(last),
                first_row,
                row,
            ),
        });
    }
    bounds.map(
        |(first_column, last_column, first_row, last_row)| PixelRect {
            x: first_column,
            y: first_row,
            width: last_column - first_column + 1,
            height: last_row - first_row + 1,
        },
    )
}

fn changed_columns(before: &[u8], after: &[u8]) -> Option<(u32, u32)> {
    let pixel_size = BYTES_PER_PIXEL as usize;
    let pixels = || {
        before
            .chunks_exact(pixel_size)
            .zip(after.chunks_exact(pixel_size))
    };
    let first = pixels().position(|(a, b)| a != b)?;
    let last = pixels().rposition(|(a, b)| a != b)?;
    Some((first as u32, last as u32))
}

#[cfg(test)]
#[path = "tests/embed_frame_tests.rs"]
mod tests;
