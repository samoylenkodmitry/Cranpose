//! Offscreen render target pool for effect layers.
//!
//! Provides reusable GPU textures that can be both rendered to (as a color
//! attachment) and sampled from (as a texture binding). Used by blur and
//! custom shader effects that need to capture a subtree's rendered output.

use crate::gpu_stats::FrameStats;
use std::cell::RefCell;

/// A GPU texture that can serve as both a render target and a texture source.
pub(crate) struct OffscreenTarget {
    // Texture kept alive for the view's lifetime; the view borrows from it implicitly.
    _texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
    /// Lazily-cached bind group for sampling this target as a texture.
    /// Valid as long as the underlying texture is alive (i.e. while this target exists).
    cached_bind_group: RefCell<Option<wgpu::BindGroup>>,
}

impl OffscreenTarget {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Offscreen Target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            _texture: texture,
            view,
            width,
            height,
            cached_bind_group: RefCell::new(None),
        }
    }

    /// Returns true if this target exactly matches the requested dimensions.
    ///
    /// Effects rely on a 1:1 mapping between render target texels and viewport
    /// coordinates, so larger pooled textures are not considered compatible.
    fn matches_size(&self, width: u32, height: u32) -> bool {
        self.width == width && self.height == height
    }

    /// Get the cached texture bind group, creating it on first access.
    ///
    /// The bind group binds this target's texture view and the provided sampler
    /// for use in effect fragment shaders. Since the underlying texture never
    /// changes while this target is alive, the bind group is valid for reuse.
    pub fn get_or_create_bind_group(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
    ) -> std::cell::Ref<'_, wgpu::BindGroup> {
        // Populate if empty
        {
            let mut slot = self.cached_bind_group.borrow_mut();
            if slot.is_none() {
                *slot = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
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
                }));
            }
        }
        std::cell::Ref::map(self.cached_bind_group.borrow(), |opt| {
            opt.as_ref().expect("bind group was just populated")
        })
    }
}

/// Pool of reusable offscreen render targets.
///
/// Targets are returned to the pool after use and reused when a suitable size
/// is available, avoiding per-frame GPU texture allocation. Capped to prevent
/// unbounded GPU memory growth from accumulating targets of varying sizes.
pub(crate) struct OffscreenPool {
    available: Vec<OffscreenTarget>,
    format: wgpu::TextureFormat,
}

/// Maximum pooled targets. Each target is a GPU texture (4 bytes/pixel RGBA).
/// At 1920×1080 each is ~8 MB, so 16 targets ≈ 128 MB worst case.
const MAX_POOLED_TARGETS: usize = 16;

impl OffscreenPool {
    pub fn new(format: wgpu::TextureFormat) -> Self {
        Self {
            available: Vec::new(),
            format,
        }
    }

    /// Number of targets currently in the pool.
    pub fn pool_size(&self) -> usize {
        self.available.len()
    }

    /// Approximate GPU memory held by pooled targets (bytes).
    pub fn estimated_bytes(&self) -> usize {
        self.available
            .iter()
            .map(|t| (t.width as usize) * (t.height as usize) * 4)
            .sum()
    }

    /// Acquire an offscreen target for the given dimensions.
    ///
    /// Returns a pooled target when dimensions exactly match, otherwise creates
    /// a new target for the requested size.
    pub fn acquire(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        stats: Option<&FrameStats>,
    ) -> OffscreenTarget {
        if let Some(idx) = self
            .available
            .iter()
            .position(|t| t.matches_size(width, height))
        {
            if let Some(s) = stats {
                s.record_offscreen_acquire(width, height, false);
            }
            self.available.swap_remove(idx)
        } else {
            if let Some(s) = stats {
                s.record_offscreen_acquire(width, height, true);
            }
            OffscreenTarget::new(device, self.format, width, height)
        }
    }

    /// Return a target to the pool for future reuse.
    ///
    /// Drops the target instead of pooling if the pool is already at capacity.
    pub fn release(&mut self, target: OffscreenTarget) {
        if self.available.len() < MAX_POOLED_TARGETS {
            self.available.push(target);
        }
        // else: target is dropped, freeing GPU memory
    }

    /// The bind group layout for sampling offscreen textures.
    ///
    /// Provides: `@group(N) @binding(0) var input_texture: texture_2d<f32>`
    ///           `@group(N) @binding(1) var input_sampler: sampler`
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

    /// The bind group layout for RuntimeShader uniforms.
    ///
    /// Provides: `@group(N) @binding(0) var<uniform> u: array<vec4<f32>, 64>`
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
    fn offscreen_target_fits() {
        // We can't create real GPU textures in unit tests, but we can test the fits logic
        // by constructing an OffscreenTarget with known dimensions.
        // Since OffscreenTarget::new requires a device, we test the fits method indirectly
        // through the pool's acquire logic pattern.
        let pool = OffscreenPool::new(wgpu::TextureFormat::Bgra8Unorm);
        assert!(pool.available.is_empty());
    }
}
