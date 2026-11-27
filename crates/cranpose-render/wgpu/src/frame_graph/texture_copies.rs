// Copies between textures outside any pass, and their count.

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
