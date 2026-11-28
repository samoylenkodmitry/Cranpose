// Copies out of textures outside any pass: into other textures, with their
// count, and into buffers the CPU reads.

use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use super::note_texture_copy;
use crate::offscreen::OffscreenTarget;

/// A region of one texture copied texel for texel into another of a
/// copy-compatible format, outside any pass.
#[derive(Clone, Copy)]
pub(crate) struct TextureRegionCopy<'a> {
    pub(crate) source: &'a OffscreenTarget,
    pub(crate) source_origin: [u32; 2],
    pub(crate) dest: &'a OffscreenTarget,
    pub(crate) dest_origin: [u32; 2],
    pub(crate) size: [u32; 2],
}

/// Whether a region can be copied between the two textures: their formats
/// are equal up to the sRGB suffix.
pub(crate) fn copy_compatible(a: &OffscreenTarget, b: &OffscreenTarget) -> bool {
    a.format().remove_srgb_suffix() == b.format().remove_srgb_suffix()
}

#[derive(Clone, Copy, Default)]
pub(crate) struct TextureCopyStats {
    pub(super) count: u32,
    pub(super) pixels: u64,
}

impl TextureCopyStats {
    pub(super) fn note(&mut self, size: [u32; 2]) {
        self.count = self.count.saturating_add(1);
        self.pixels = self
            .pixels
            .saturating_add(u64::from(size[0]) * u64::from(size[1]));
    }
}

fn texel_copy_info(target: &OffscreenTarget, origin: [u32; 2]) -> wgpu::TexelCopyTextureInfo<'_> {
    wgpu::TexelCopyTextureInfo {
        texture: target.texture(),
        mip_level: 0,
        origin: wgpu::Origin3d {
            x: origin[0],
            y: origin[1],
            z: 0,
        },
        aspect: wgpu::TextureAspect::All,
    }
}

pub(super) fn encode_texture_region_copy(
    encoder: &mut wgpu::CommandEncoder,
    copy: TextureRegionCopy<'_>,
) {
    encoder.copy_texture_to_texture(
        texel_copy_info(copy.source, copy.source_origin),
        texel_copy_info(copy.dest, copy.dest_origin),
        wgpu::Extent3d {
            width: copy.size[0],
            height: copy.size[1],
            depth_or_array_layers: 1,
        },
    );
    note_texture_copy(copy.size);
}

// A whole texture copied into a buffer the CPU reads.
pub(crate) struct TextureReadBack<'a> {
    pub(crate) source: &'a OffscreenTarget,
    pub(crate) buffer: &'a wgpu::Buffer,
    pub(crate) bytes_per_row: u32,
    pub(crate) state: ReadBackState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReadBackStatus {
    // the copy has not finished on the GPU
    Pending,
    // the buffer is mapped and holds the texture
    Readable,
    // the map failed
    Lost,
}

// Where a read back stands, shared with the callback of its map.
#[derive(Clone, Default)]
pub(crate) struct ReadBackState(Arc<AtomicU8>);

const PENDING: u8 = 0;
const READABLE: u8 = 1;
const LOST: u8 = 2;

impl ReadBackState {
    pub(crate) fn status(&self) -> ReadBackStatus {
        match self.0.load(Ordering::Acquire) {
            PENDING => ReadBackStatus::Pending,
            READABLE => ReadBackStatus::Readable,
            _ => ReadBackStatus::Lost,
        }
    }
}

// Copies whole textures into buffers the CPU reads, in a submit of their
// own; each buffer maps once its copy is done.
pub(crate) fn read_back_textures<'a>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    reads: impl IntoIterator<Item = TextureReadBack<'a>>,
) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Texture Read Back"),
    });
    for read in reads {
        let size = read.source.texture().size();
        encoder.copy_texture_to_buffer(
            texel_copy_info(read.source, [0, 0]),
            wgpu::TexelCopyBufferInfo {
                buffer: read.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(read.bytes_per_row),
                    rows_per_image: Some(size.height),
                },
            },
            size,
        );
        let state = read.state;
        encoder.map_buffer_on_submit(read.buffer, wgpu::MapMode::Read, .., move |result| {
            let status = if result.is_ok() { READABLE } else { LOST };
            state.0.store(status, Ordering::Release);
        });
    }
    queue.submit([encoder.finish()]);
}
