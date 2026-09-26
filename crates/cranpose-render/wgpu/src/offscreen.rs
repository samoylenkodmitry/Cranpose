use std::{
    cell::OnceCell,
    future::Future,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll, Waker},
};

use crate::{gpu_stats::FrameStats, idle_pool::IdlePool};

/// Set once a device turns out unable to draw into the float format, which
/// then no renderer in the process composites in.
static FLOAT_COMPOSITION_UNSUPPORTED: AtomicBool = AtomicBool::new(false);

pub(crate) fn composition_format() -> wgpu::TextureFormat {
    static FORMAT: std::sync::OnceLock<wgpu::TextureFormat> = std::sync::OnceLock::new();
    let preferred = *FORMAT.get_or_init(|| {
        resolve_composition_format(
            crate::debug_toggles::debug_toggle("CRANPOSE_COMPOSITION_8BIT").as_deref(),
            cfg!(target_os = "android"),
        )
    });
    if FLOAT_COMPOSITION_UNSUPPORTED.load(Ordering::Relaxed) {
        wgpu::TextureFormat::Rgba8Unorm
    } else {
        preferred
    }
}

/// The format a renderer on `device` composites in: the float format where
/// the device can draw into it, eight bits where it cannot.
///
/// WebGPU, Vulkan, Metal and DirectX all draw into `Rgba16Float`. OpenGL ES
/// and WebGL2 draw into it only through `EXT_color_buffer_float` or
/// `EXT_color_buffer_half_float`, which some browsers and drivers leave out;
/// every offscreen layer and effect pipeline would then fail validation and
/// leave the window blank.
pub(crate) fn settle_composition_format(
    device: &wgpu::Device,
    backend: wgpu::Backend,
) -> wgpu::TextureFormat {
    let format = composition_format();
    if backend == wgpu::Backend::Gl
        && format != wgpu::TextureFormat::Rgba8Unorm
        && !renders_into(device, format)
    {
        log::warn!("[gpu-init] this device cannot draw into {format:?}; compositing in Rgba8Unorm");
        FLOAT_COMPOSITION_UNSUPPORTED.store(true, Ordering::Relaxed);
    }
    composition_format()
}

/// Whether `device` accepts a texture of `format` to draw into and sample.
pub(crate) fn renders_into(device: &wgpu::Device, format: wgpu::TextureFormat) -> bool {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let _probe = create_2d_texture(
        device,
        format,
        1,
        1,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        Some("Composition format probe"),
    );
    let mut error = std::pin::pin!(scope.pop());
    // A device wgpu validates itself answers at once; only a browser's
    // WebGPU answers later, and WebGPU draws into every format asked here.
    match error.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(error) => error.is_none(),
        Poll::Pending => true,
    }
}

fn resolve_composition_format(requested: Option<&str>, android: bool) -> wgpu::TextureFormat {
    let eight_bit = match requested.map(str::trim) {
        Some("1" | "true" | "yes") => true,
        Some("0" | "false" | "no") => false,
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
    available: IdlePool<OffscreenTarget>,
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
            available: IdlePool::default(),
            format,
            max_texture_dim: device.limits().max_texture_dimension_2d,
        }
    }

    #[cfg(test)]
    fn new_with_limit(format: wgpu::TextureFormat, max_texture_dim: u32) -> Self {
        Self {
            available: IdlePool::default(),
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
        if let Some(target) = self.available.take(|t| t.matches_size(width, height)) {
            if let Some(s) = stats {
                s.record_offscreen_acquire(width, height, self.format, false);
            }
            target
        } else {
            if let Some(s) = stats {
                s.record_offscreen_acquire(width, height, self.format, true);
            }
            OffscreenTarget::new(device, self.format, width, height)
        }
    }

    pub fn release(&mut self, target: OffscreenTarget) {
        let bytes_per_pixel = self.bytes_per_pixel();
        self.available
            .put(target, MAX_POOLED_TARGETS, MAX_POOLED_BYTES, |t| {
                target_bytes(t.width, t.height, bytes_per_pixel)
            });
    }

    /// Ends a frame, dropping the targets no frame has reused for
    /// [`crate::idle_pool::IDLE_FRAMES`].
    pub fn end_frame(&mut self) {
        self.available.end_frame();
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
                    has_dynamic_offset: true,
                    min_binding_size: None,
                },
                count: None,
            }],
        })
    }
}

#[cfg(test)]
#[path = "tests/offscreen_tests.rs"]
mod tests;
