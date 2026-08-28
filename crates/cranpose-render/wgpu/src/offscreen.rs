//! Offscreen render target pool for effect layers.
//!
//! Provides reusable GPU textures that can be both rendered to (as a color
//! attachment) and sampled from (as a texture binding). Used by blur and
//! custom shader effects that need to capture a subtree's rendered output.

use std::cell::OnceCell;

use crate::gpu_stats::FrameStats;

/// The format every offscreen surface and the persistent composition target
/// share, latched at first use — render pipelines bake their target format,
/// so it cannot change mid-process.
///
/// `Rgba16Float` (8 B/px) keeps composition exact; on a bandwidth-starved
/// mobile GPU those 8 bytes double the cost of every pass in the frame. On
/// Android the default is `Rgba8Unorm` — the platform composites its own UI
/// through 8-bit `RGBA_8888` surfaces, and on a Kirin 980 scrolling a
/// glass-chrome list the 4 B/px pipeline measured present p50 7.2→5.4 ms
/// with no visible difference on the panel. Desktop, iOS and web keep the
/// float pipeline. `CRANPOSE_COMPOSITION_8BIT=1`/`0`
/// (`debug.cranpose.composition_8bit` over adb) overrides in either
/// direction without a rebuild.
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

/// A GPU texture that can serve as both a render target and a texture source.
pub(crate) struct OffscreenTarget {
    // Texture kept alive for the view's lifetime; the view borrows from it implicitly.
    texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
    bytes_per_pixel: u64,
    /// Lazily-cached bind group for sampling this target as a texture.
    /// Valid as long as the underlying texture is alive (i.e. while this target exists).
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
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            width,
            height,
            bytes_per_pixel: crate::frame_graph::texture_format_bytes_per_pixel(format),
            cached_bind_group: OnceCell::new(),
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

    pub(crate) fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
}

pub(crate) fn composition_bytes_per_pixel() -> u64 {
    crate::frame_graph::texture_format_bytes_per_pixel(composition_format())
}

/// Pool of reusable offscreen render targets.
///
/// Targets are returned to the pool after use and reused when a suitable size
/// is available, avoiding per-frame GPU texture allocation. Capped to prevent
/// unbounded GPU memory growth from accumulating targets of varying sizes.
pub(crate) struct OffscreenPool {
    available: Vec<OffscreenTarget>,
    format: wgpu::TextureFormat,
    max_texture_dim: u32,
}

/// Backstop on pooled targets, so a pathological frame cannot grow the pool
/// without bound even when every target is small.
const MAX_POOLED_TARGETS: usize = 64;

/// Memory the pool may hold. A count-only cap cannot bound memory, because a
/// target is anything from 132x132 to full screen; a byte budget can.
///
/// A screen of frosted controls asks for one surface per control every frame:
/// a scrolling list on a 1080x2244 phone acquired thirteen, and a cap of
/// sixteen targets kept the wrong ones, so twelve of the thirteen were created
/// again on every frame. 128 MB holds a frame's worth of surfaces on that phone
/// with room for the blur scratch.
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

    /// Maximum texture dimension supported by the GPU.
    pub fn max_texture_dim(&self) -> u32 {
        self.max_texture_dim
    }

    /// Number of targets currently in the pool.
    pub fn pool_size(&self) -> usize {
        self.available.len()
    }

    /// Approximate GPU memory held by pooled targets (bytes).
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

    /// Return a target to the pool for future reuse.
    ///
    /// The pool keeps the most recently returned targets, since those are the
    /// sizes the next frame asks for, and drops the oldest ones once it is
    /// over its budget.
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
    fn an_unparseable_override_falls_back_to_the_platform_default() {
        assert_eq!(
            resolve_composition_format(Some("half"), true),
            wgpu::TextureFormat::Rgba8Unorm
        );
        assert_eq!(
            resolve_composition_format(Some(""), false),
            wgpu::TextureFormat::Rgba16Float
        );
    }

    /// A frame of frosted controls returns more surfaces than the old count
    /// cap held, and the sizes it returns are the sizes the next frame asks
    /// for.
    #[test]
    fn a_frame_worth_of_small_surfaces_stays_pooled() {
        let bytes: u64 = (0..20)
            .map(|_| target_bytes(132, 132, composition_bytes_per_pixel()))
            .sum();
        assert!(
            bytes < MAX_POOLED_BYTES,
            "twenty control surfaces must fit the pool budget"
        );
        // `const_assert`-shaped on purpose: both sides are constants, so this
        // is a compile-time claim about the budget, not a runtime check.
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
