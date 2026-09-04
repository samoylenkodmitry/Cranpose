use std::cell::OnceCell;

use crate::gpu_stats::FrameStats;

pub(crate) fn composition_format() -> wgpu::TextureFormat {
    static FORMAT: std::sync::OnceLock<wgpu::TextureFormat> = std::sync::OnceLock::new();
    *FORMAT.get_or_init(|| {
        resolve_composition_format(
            crate::debug_toggles::debug_toggle("CRANPOSE_COMPOSITION_8BIT").as_deref(),
            cfg!(target_os = "android"),
        )
    })
}

fn resolve_composition_format(requested: Option<&str>, android: bool) -> wgpu::TextureFormat {
    let eight_bit = match requested.map(str::trim) {
        Some("1") | Some("true") | Some("yes") => true,
        Some("0") | Some("false") | Some("no") => false,
        _ => android,
    };
    if eight_bit {
        wgpu::TextureFormat::Rgba8Unorm
    } else {
        wgpu::TextureFormat::Rgba16Float
    }
}

pub(crate) fn create_2d_texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    usage: wgpu::TextureUsages,
    label: Option<&str>,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label,
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

pub(crate) struct OffscreenTarget {
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
    bytes_per_pixel: u64,
    cached_bind_group: OnceCell<wgpu::BindGroup>,
}

impl OffscreenTarget {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        Self::new_labeled(device, format, width, height, "Offscreen Target")
    }

    pub(crate) fn new_labeled(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        label: &'static str,
    ) -> Self {
        let texture = create_2d_texture(
            device,
            format,
            width,
            height,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            Some(label),
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            view,
            width,
            height,
            bytes_per_pixel: crate::frame_graph::texture_format_bytes_per_pixel(format),
            cached_bind_group: OnceCell::new(),
        }
    }

    pub(crate) fn texture(&self) -> &wgpu::Texture {
        self.view.texture()
    }

    pub(crate) fn format(&self) -> wgpu::TextureFormat {
        self.texture().format()
    }

    fn matches_size(&self, width: u32, height: u32) -> bool {
        self.width == width && self.height == height
    }

    pub fn get_or_create_bind_group(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
    ) -> &wgpu::BindGroup {
        self.cached_bind_group.get_or_init(|| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Offscreen Texture Bind Group (cached)"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&self.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
        })
    }

    /// Wraps a swapchain image as the frame's root target so the scene
    /// renders into it directly, with no composition copy behind it.
    pub(crate) fn from_surface(texture: wgpu::Texture, view: wgpu::TextureView) -> Self {
        let width = texture.width();
        let height = texture.height();
        let format = texture.format().remove_srgb_suffix();
        Self {
            view,
            width,
            height,
            bytes_per_pixel: crate::frame_graph::texture_format_bytes_per_pixel(format),
            cached_bind_group: OnceCell::new(),
        }
    }
}

/// Bytes one pixel of the renderer's composition format occupies.
pub fn composition_bytes_per_pixel() -> u64 {
    crate::frame_graph::texture_format_bytes_per_pixel(composition_format())
}

pub(crate) struct OffscreenPool {
    available: Vec<OffscreenTarget>,
    format: wgpu::TextureFormat,
    max_texture_dim: u32,
}

const MAX_POOLED_TARGETS: usize = 64;

const MAX_POOLED_BYTES: u64 = 128 * 1024 * 1024;

fn target_bytes(width: u32, height: u32, bytes_per_pixel: u64) -> u64 {
    u64::from(width) * u64::from(height) * bytes_per_pixel
}

impl OffscreenPool {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self {
            available: Vec::new(),
            format,
            max_texture_dim: device.limits().max_texture_dimension_2d,
        }
    }

    #[cfg(test)]
    fn new_with_limit(format: wgpu::TextureFormat, max_texture_dim: u32) -> Self {
        Self {
            available: Vec::new(),
            format,
            max_texture_dim,
        }
    }

    pub fn max_texture_dim(&self) -> u32 {
        self.max_texture_dim
    }

    pub fn pool_size(&self) -> usize {
        self.available.len()
    }

    pub fn estimated_bytes(&self) -> usize {
        self.available
            .iter()
            .map(|t| {
                (t.width as u64)
                    .saturating_mul(t.height as u64)
                    .saturating_mul(t.bytes_per_pixel) as usize
            })
            .sum()
    }

    pub fn acquire(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        stats: Option<&FrameStats>,
    ) -> OffscreenTarget {
        let width = width.min(self.max_texture_dim).max(1);
        let height = height.min(self.max_texture_dim).max(1);
        if let Some(idx) = self
            .available
            .iter()
            .position(|t| t.matches_size(width, height))
        {
            if let Some(s) = stats {
                s.record_offscreen_acquire(width, height, self.format, false);
            }
            self.available.swap_remove(idx)
        } else {
            if let Some(s) = stats {
                s.record_offscreen_acquire(width, height, self.format, true);
            }
            OffscreenTarget::new(device, self.format, width, height)
        }
    }

    pub fn release(&mut self, target: OffscreenTarget) {
        self.available.push(target);
        while self.available.len() > MAX_POOLED_TARGETS
            || self.pooled_bytes() > MAX_POOLED_BYTES && self.available.len() > 1
        {
            self.available.remove(0);
        }
    }

    fn pooled_bytes(&self) -> u64 {
        self.available
            .iter()
            .map(|t| target_bytes(t.width, t.height, self.bytes_per_pixel()))
            .sum()
    }

    fn bytes_per_pixel(&self) -> u64 {
        crate::frame_graph::texture_format_bytes_per_pixel(self.format)
    }

    pub fn texture_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Effect Texture Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        })
    }

    pub fn uniform_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Effect Uniform Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_composites_in_eight_bits_and_the_rest_in_float() {
        assert_eq!(
            resolve_composition_format(None, true),
            wgpu::TextureFormat::Rgba8Unorm
        );
        assert_eq!(
            resolve_composition_format(None, false),
            wgpu::TextureFormat::Rgba16Float
        );
    }

    #[test]
    fn the_override_wins_in_both_directions_on_any_platform() {
        assert_eq!(
            resolve_composition_format(Some("0"), true),
            wgpu::TextureFormat::Rgba16Float
        );
        assert_eq!(
            resolve_composition_format(Some(" yes "), false),
            wgpu::TextureFormat::Rgba8Unorm
        );
    }

    #[test]
    fn an_unparsable_override_falls_back_to_the_platform_default() {
        assert_eq!(
            resolve_composition_format(Some("half"), true),
            wgpu::TextureFormat::Rgba8Unorm
        );
        assert_eq!(
            resolve_composition_format(Some(""), false),
            wgpu::TextureFormat::Rgba16Float
        );
    }

    #[test]
    fn a_frame_worth_of_small_surfaces_stays_pooled() {
        let bytes: u64 = (0..20)
            .map(|_| target_bytes(132, 132, composition_bytes_per_pixel()))
            .sum();
        assert!(
            bytes < MAX_POOLED_BYTES,
            "twenty control surfaces must fit the pool budget"
        );
        const _: () = assert!(20 < MAX_POOLED_TARGETS);
    }

    #[test]
    fn the_budget_bounds_full_screen_surfaces() {
        let full_screen = target_bytes(1080, 2244, composition_bytes_per_pixel());
        let held = MAX_POOLED_BYTES / full_screen;
        assert!(
            (2..=8).contains(&held),
            "the budget should hold a few full-screen surfaces, not dozens: {held}"
        );
    }

    #[test]
    fn pool_starts_empty() {
        let pool = OffscreenPool::new_with_limit(wgpu::TextureFormat::Bgra8Unorm, 8192);
        assert!(pool.available.is_empty());
        assert_eq!(pool.pool_size(), 0);
    }

    #[test]
    fn max_texture_dimension_stored() {
        let pool = OffscreenPool::new_with_limit(wgpu::TextureFormat::Bgra8Unorm, 2048);
        assert_eq!(pool.max_texture_dim, 2048);

        let pool = OffscreenPool::new_with_limit(wgpu::TextureFormat::Bgra8Unorm, 4096);
        assert_eq!(pool.max_texture_dim, 4096);
    }
}
