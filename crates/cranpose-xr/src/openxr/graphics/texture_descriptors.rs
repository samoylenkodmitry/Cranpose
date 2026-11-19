//! The descriptors of a swapchain image that wgpu wraps.

use wgpu::wgt::TextureUses;

// The wgpu-hal and the wgpu descriptor of a 2D image of `size` and `format` that the app uses
// as `usage` says.
pub(crate) fn texture_descriptors(
    size: (u32, u32),
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> (
    wgpu::hal::TextureDescriptor<'static>,
    wgpu::TextureDescriptor<'static>,
) {
    let size = wgpu::Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
    };
    let mut uses = TextureUses::empty();
    // A depth swapchain's image is a render attachment as a depth buffer is
    let attachment = if format.is_depth_stencil_format() {
        TextureUses::DEPTH_STENCIL_WRITE
    } else {
        TextureUses::COLOR_TARGET
    };
    for (usage_bit, use_bit) in [
        (wgpu::TextureUsages::RENDER_ATTACHMENT, attachment),
        (wgpu::TextureUsages::COPY_DST, TextureUses::COPY_DST),
        (wgpu::TextureUsages::COPY_SRC, TextureUses::COPY_SRC),
    ] {
        if usage.contains(usage_bit) {
            uses |= use_bit;
        }
    }
    let hal = wgpu::hal::TextureDescriptor {
        label: Some("cranpose xr swapchain image"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: uses,
        memory_flags: wgpu::hal::MemoryFlags::empty(),
        view_formats: Vec::new(),
    };
    let descriptor = wgpu::TextureDescriptor {
        label: Some("cranpose xr swapchain image"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    };
    (hal, descriptor)
}
