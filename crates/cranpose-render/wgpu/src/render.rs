//! GPU rendering implementation using WGPU

use crate::effect_renderer::{CompositeSampleMode, EffectRenderer, RoundedCompositeMask};
#[cfg(test)]
use crate::normalized_scene::estimate_layer_surface_rect;
#[cfg(test)]
use crate::normalized_scene::{
    build_scene_window, filtered_effect_layer_index, scene_bounds, visible_draw_rect,
    SceneWindowSource,
};
use crate::normalized_scene::{
    collect_layer_contents, collect_layer_contents_with_translation_context, effect_layer_in_range,
    estimate_layer_surface_rect_cached, scene_has_layer_events, translate_quad, CollectedLayer,
};
use crate::offscreen::OffscreenTarget;
use crate::scene::{
    BackdropLayer, CompositorScene, DrawShape, EffectLayer, ImageDraw, ShadowDraw, SnapAnchor,
    TextDraw,
};
use crate::shaders;
#[cfg(test)]
use crate::surface_executor::DevicePixelBounds;
use crate::surface_executor::{
    apply_backdrop_layer_to_target as execute_apply_backdrop_layer_to_target,
    axis_aligned_quad_rect, device_pixel_bounds_for_rect, offscreen_byte_size,
    render_effect_layer_to_target as execute_render_effect_layer_to_target,
    render_layer_surface as execute_render_layer_surface,
    render_root_direct as execute_render_root_direct, scaled_quad, snap_motion_stable_dest_quad,
    surface_target_size, CachedLayerSurface, LayerSurface, LayerSurfaceTexture,
    SurfaceExecutionBackend,
};
#[cfg(test)]
use crate::surface_executor::{clamp_effect_surface_scale, visible_layer_rect};
#[cfg(test)]
use crate::surface_plan::{
    composite_sample_mode_for_effect_layer, composite_sample_mode_for_requirements,
    direct_translation, effect_layer_target_scale, layer_contains_descendant_backdrop,
    layer_surface_requirements, layer_surface_target_scale,
};
use crate::surface_plan::{
    layer_surface_requirements_cached, layer_uses_external_backdrop_input,
    root_can_render_directly_cached, LayerSurfaceRequest, LayerSurfaceRequirements,
    TranslationRenderContext,
};
#[cfg(test)]
use crate::surface_requirements::SurfaceRequirement;
use crate::surface_requirements::SurfaceRequirementSet;
use crate::{EnsureTextBufferParams, TextCacheKey, TextSystemState};
use bytemuck::{Pod, Zeroable};
use cranpose_core::NodeId;
use cranpose_render_common::geometry::blur_extent_margin;
use cranpose_render_common::graph::{
    quad_bounds, CachePolicy, LayerNode, ProjectiveTransform, RenderGraph,
};
#[cfg(test)]
use cranpose_render_common::graph::{PrimitiveEntry, PrimitiveNode, PrimitivePhase, RenderNode};
use cranpose_render_common::raster_cache::{LayerRasterCacheKey, ScaleBucket};
#[cfg(test)]
use cranpose_ui_graphics::GraphicsLayer;
use cranpose_ui_graphics::Point;
use cranpose_ui_graphics::{
    BlendMode, Brush, Color, ColorFilter, ImageBitmap, Rect, RenderEffect, RenderHash, TileMode,
};
use glyphon::{
    Cache, Color as GlyphonColor, Resolution, SwashCache, TextArea, TextAtlas, TextBounds,
    TextRenderer, Viewport,
};
use lru::LruCache;
use rustc_hash::FxHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::ops::Range;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::Duration;

use crate::gpu_stats;
use crate::gpu_stats::gpu_stats_enabled;
use crate::pipeline::push_layer_shadow;

// Chunked rendering constants for robustness with large scenes
// Note: Limited to 256 for WebGL compatibility (uniform buffer size limit)
// WebGL guarantees 16KB uniform buffers, ShapeData is 64 bytes = 256 max shapes
const HARD_MAX_BUFFER_MB: usize = 64; // Maximum 64MB per buffer
const MAX_LAYER_SURFACE_CACHE_ITEMS: usize = 256;
pub(crate) const MAX_LAYER_SURFACE_CACHE_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: 18.0 / 255.0,
    g: 18.0 / 255.0,
    b: 24.0 / 255.0,
    a: 1.0,
};
#[cfg(not(target_arch = "wasm32"))]
const INITIAL_UPLOAD_BUFFER_BYTES: u64 = 4 * 1024;
const MAX_TEXTURE_CACHE_ITEMS: usize = 256;
static REPORTED_UNSUPPORTED_WGPU_BLEND_MODES: AtomicBool = AtomicBool::new(false);
static REPORTED_UNSUPPORTED_WGPU_EFFECTS: AtomicBool = AtomicBool::new(false);
static TEXT_RENDER_TELEMETRY_ENABLED: OnceLock<bool> = OnceLock::new();
static TEXT_RENDER_CALLS: AtomicU64 = AtomicU64::new(0);
static TEXT_RENDER_SKIPS: AtomicU64 = AtomicU64::new(0);
static TEXT_RENDER_ENSURE_RESHAPES: AtomicU64 = AtomicU64::new(0);
static TEXT_RENDER_ENSURE_REUSES: AtomicU64 = AtomicU64::new(0);
static TEXT_RENDER_CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static TEXT_RENDER_CACHE_MISSES: AtomicU64 = AtomicU64::new(0);

fn text_render_telemetry_enabled() -> bool {
    *TEXT_RENDER_TELEMETRY_ENABLED
        .get_or_init(|| std::env::var_os("CRANPOSE_TEXT_RENDER_TELEMETRY").is_some())
}

fn is_blend_mode_supported(mode: BlendMode) -> bool {
    matches!(mode, BlendMode::SrcOver | BlendMode::DstOut)
}

fn blend_state_for_mode(mode: BlendMode) -> wgpu::BlendState {
    match mode {
        BlendMode::DstOut => wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        },
        _ => wgpu::BlendState::ALPHA_BLENDING,
    }
}

fn supported_blend_mode(mode: BlendMode) -> BlendMode {
    if is_blend_mode_supported(mode) {
        return mode;
    }

    if !REPORTED_UNSUPPORTED_WGPU_BLEND_MODES.swap(true, Ordering::Relaxed) {
        log::warn!(
            "WGPU renderer currently supports BlendMode::SrcOver and BlendMode::DstOut; falling back to SrcOver for unsupported modes"
        );
    }

    BlendMode::SrcOver
}

fn is_render_effect_supported(effect: &RenderEffect) -> bool {
    match effect {
        RenderEffect::Blur { .. } => true,
        RenderEffect::Offset { .. } => true,
        RenderEffect::Shader { .. } => true,
        RenderEffect::Chain { first, second } => {
            is_render_effect_supported(first) && is_render_effect_supported(second)
        }
    }
}

fn warn_unsupported_effect_once() {
    if !REPORTED_UNSUPPORTED_WGPU_EFFECTS.swap(true, Ordering::Relaxed) {
        log::warn!(
            "WGPU renderer received an unsupported RenderEffect variant; falling back to passthrough compositing"
        );
    }
}

fn resolve_gradient_point(origin: f32, extent: f32, value: f32) -> f32 {
    if value.is_finite() {
        origin + value
    } else if value.is_sign_positive() {
        origin + extent
    } else {
        origin
    }
}

fn gradient_tile_mode_value(tile_mode: TileMode) -> u32 {
    match tile_mode {
        TileMode::Clamp => 0,
        TileMode::Repeated => 1,
        TileMode::Mirror => 2,
        TileMode::Decal => 3,
    }
}

fn create_shape_pipeline(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    uniform_layout: &wgpu::BindGroupLayout,
    shape_layout: &wgpu::BindGroupLayout,
    blend_mode: BlendMode,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Shape Shader"),
        source: wgpu::ShaderSource::Wgsl(shaders::SHADER.into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Render Pipeline Layout"),
        bind_group_layouts: &[uniform_layout, shape_layout],
        immediate_size: 0,
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Render Pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Vertex::desc()],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(blend_state_for_mode(blend_mode)),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn create_image_pipeline(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    uniform_layout: &wgpu::BindGroupLayout,
    image_layout: &wgpu::BindGroupLayout,
    blend_mode: BlendMode,
) -> wgpu::RenderPipeline {
    let image_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Image Shader"),
        source: wgpu::ShaderSource::Wgsl(shaders::IMAGE_SHADER.into()),
    });

    let image_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Image Pipeline Layout"),
        bind_group_layouts: &[uniform_layout, image_layout],
        immediate_size: 0,
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Image Pipeline"),
        layout: Some(&image_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &image_shader,
            entry_point: Some("image_vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Vertex::desc()],
        },
        fragment: Some(wgpu::FragmentState {
            module: &image_shader,
            entry_point: Some("image_fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(blend_state_for_mode(blend_mode)),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
    uv: [f32; 2],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4, 2 => Float32x2];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct Uniforms {
    viewport: [f32; 2],
    viewport_offset: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct ShapeData {
    rect: [f32; 4],            // x, y, width, height
    radii: [f32; 4],           // top_left, top_right, bottom_left, bottom_right
    gradient_params: [f32; 4], // linear: start.xy,end.xy; radial: center.xy,radius,unused
    clip_rect: [f32; 4],       // clip_x, clip_y, clip_width, clip_height (0,0,0,0 = no clip)
    brush_type: u32,           // 0=solid, 1=linear_gradient, 2=radial_gradient
    gradient_start: u32,       // Starting index in gradient buffer
    gradient_count: u32,       // Number of gradient stops
    gradient_tile_mode: u32,   // 0=Clamp, 1=Repeated, 2=Mirror, 3=Decal
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct GradientStop {
    color: [f32; 4],
    position: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImageSampleMode {
    Nearest,
    Linear,
}

struct CachedImageTexture {
    _texture: wgpu::Texture,
    _view: wgpu::TextureView,
    nearest_bind_group: wgpu::BindGroup,
    linear_bind_group: wgpu::BindGroup,
}

struct ImageDrawCmd {
    index_start: u32,
    scissor: (u32, u32, u32, u32),
    image_id: u64,
    sample_mode: ImageSampleMode,
}

#[derive(Clone, Copy)]
enum LayerEventKind {
    Backdrop(usize),
    Effect(usize),
}

#[derive(Clone, Copy)]
struct LayerEvent {
    z_index: usize,
    kind: LayerEventKind,
}

impl LayerEvent {
    fn kind_order(self) -> u8 {
        match self.kind {
            // Backdrop must run before same-z content/effects so it samples only
            // already-rendered background.
            LayerEventKind::Backdrop(_) => 0,
            LayerEventKind::Effect(_) => 1,
        }
    }
}

// Cached text buffer is now defined in lib.rs as SharedTextBuffer and shared
// between measurement and rendering to eliminate duplicate text shaping

/// Persistent GPU buffers for batched shape rendering
struct ShapeBatchBuffers {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    shape_buffer: wgpu::Buffer,
    gradient_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    vertex_capacity: usize,
    index_capacity: usize,
    shape_capacity: usize,
    gradient_capacity: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UploadTarget {
    Uniform,
    ShapeVertex,
    ShapeIndex,
    ShapeData,
    ShapeGradient,
    ImageVertex,
    ImageIndex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingBufferCopy {
    source_offset: u64,
    size: u64,
    target: UploadTarget,
}

#[derive(Default)]
struct StagedBufferUploads {
    bytes: Vec<u8>,
    copies: Vec<PendingBufferCopy>,
}

impl StagedBufferUploads {
    fn clear(&mut self) {
        self.bytes.clear();
        self.copies.clear();
    }

    fn is_empty(&self) -> bool {
        self.copies.is_empty()
    }

    #[cfg(any(test, target_arch = "wasm32"))]
    fn payload_for_copy(&self, copy: PendingBufferCopy) -> &[u8] {
        let start = copy.source_offset as usize;
        let end = start + copy.size as usize;
        &self.bytes[start..end]
    }

    fn stage(&mut self, target: UploadTarget, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        debug_assert_eq!(
            bytes.len() % wgpu::COPY_BUFFER_ALIGNMENT as usize,
            0,
            "buffer uploads must be aligned to copy requirements"
        );

        let aligned_offset = align_usize_to(self.bytes.len(), wgpu::COPY_BUFFER_ALIGNMENT as usize);
        if aligned_offset > self.bytes.len() {
            self.bytes.resize(aligned_offset, 0);
        }

        let source_offset = self.bytes.len() as u64;
        self.bytes.extend_from_slice(bytes);
        self.copies.push(PendingBufferCopy {
            source_offset,
            size: bytes.len() as u64,
            target,
        });
    }
}

impl ShapeBatchBuffers {
    fn new(device: &wgpu::Device, bind_group_layout: &wgpu::BindGroupLayout) -> Self {
        // For WebGL uniform buffers, size MUST match shader declaration (200 shapes)
        // Shader declares: var<uniform> shape_data: array<ShapeData, 200>
        // ShapeData is 80 bytes (with clip_rect), 16KB/80 = 200 shapes
        const WEBGL_UNIFORM_SHAPE_COUNT: usize = 200;
        const WEBGL_UNIFORM_GRADIENT_COUNT: usize = 256;

        let initial_vertex_cap = WEBGL_UNIFORM_SHAPE_COUNT * 4; // 4 vertices per shape
        let initial_index_cap = WEBGL_UNIFORM_SHAPE_COUNT * 6; // 6 indices per shape
        let initial_shape_cap = WEBGL_UNIFORM_SHAPE_COUNT;
        let initial_gradient_cap = WEBGL_UNIFORM_GRADIENT_COUNT;

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shape Vertex Buffer"),
            size: (std::mem::size_of::<Vertex>() * initial_vertex_cap) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shape Index Buffer"),
            size: (std::mem::size_of::<u32>() * initial_index_cap) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Use UNIFORM for WebGL compatibility (storage buffers not supported)
        let shape_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shape Data Buffer"),
            size: (std::mem::size_of::<ShapeData>() * initial_shape_cap) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let gradient_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Gradient Buffer"),
            size: (std::mem::size_of::<GradientStop>() * initial_gradient_cap) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Shape Bind Group"),
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: shape_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: gradient_buffer.as_entire_binding(),
                },
            ],
        });

        Self {
            vertex_buffer,
            index_buffer,
            shape_buffer,
            gradient_buffer,
            bind_group,
            vertex_capacity: initial_vertex_cap,
            index_capacity: initial_index_cap,
            shape_capacity: initial_shape_cap,
            gradient_capacity: initial_gradient_cap,
        }
    }

    /// Ensure buffers have enough capacity, resizing if needed.
    /// Clamps growth to prevent excessive allocations for huge scenes.
    fn ensure_capacity(
        &mut self,
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
        vertices_needed: usize,
        indices_needed: usize,
        shapes_needed: usize,
        gradients_needed: usize,
    ) {
        let mut need_bind_group_update = false;
        let hard_max_bytes = HARD_MAX_BUFFER_MB * 1024 * 1024;

        if vertices_needed > self.vertex_capacity {
            let desired = vertices_needed.next_power_of_two();
            let max_count = hard_max_bytes / std::mem::size_of::<Vertex>();
            let new_cap = desired.min(max_count);
            self.vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Shape Vertex Buffer"),
                size: (std::mem::size_of::<Vertex>() * new_cap) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.vertex_capacity = new_cap;
        }

        if indices_needed > self.index_capacity {
            let desired = indices_needed.next_power_of_two();
            let max_count = hard_max_bytes / std::mem::size_of::<u32>();
            let new_cap = desired.min(max_count);
            self.index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Shape Index Buffer"),
                size: (std::mem::size_of::<u32>() * new_cap) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.index_capacity = new_cap;
        }

        if shapes_needed > self.shape_capacity {
            let desired = shapes_needed.next_power_of_two();
            let max_count = hard_max_bytes / std::mem::size_of::<ShapeData>();
            let new_cap = desired.min(max_count);
            self.shape_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Shape Data Buffer"),
                size: (std::mem::size_of::<ShapeData>() * new_cap) as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.shape_capacity = new_cap;
            need_bind_group_update = true;
        }

        if gradients_needed > self.gradient_capacity {
            let desired = gradients_needed.max(1).next_power_of_two();
            let max_count = hard_max_bytes / std::mem::size_of::<GradientStop>();
            let new_cap = desired.min(max_count);
            self.gradient_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Gradient Buffer"),
                size: (std::mem::size_of::<GradientStop>() * new_cap) as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.gradient_capacity = new_cap;
            need_bind_group_update = true;
        }

        if need_bind_group_update {
            self.bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Shape Bind Group"),
                layout: bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.shape_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.gradient_buffer.as_entire_binding(),
                    },
                ],
            });
        }
    }
}

// TextCacheKey is now defined in lib.rs and shared between measurement and rendering

#[derive(Clone, Copy)]
struct TranslatedTextRasterOrigin {
    position: Point,
    root_scale: f32,
}

pub struct GpuRenderer {
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    surface_format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    pipeline_dst_out: wgpu::RenderPipeline,
    shape_bind_group_layout: wgpu::BindGroupLayout,
    image_pipeline: wgpu::RenderPipeline,
    image_pipeline_dst_out: wgpu::RenderPipeline,
    image_bind_group_layout: wgpu::BindGroupLayout,
    image_sampler_nearest: wgpu::Sampler,
    image_sampler_linear: wgpu::Sampler,
    text_renderer_pool: Vec<TextRendererSlot>,
    /// Which slot in `text_renderer_pool` the next prepare→encode pair should use.
    text_batch_cursor: usize,
    text_atlas: TextAtlas,
    swash_cache: SwashCache,
    // Persistent GPU buffers (reused across frames)
    #[cfg(not(target_arch = "wasm32"))]
    upload_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    shape_buffers: ShapeBatchBuffers,
    image_vertex_buffer: wgpu::Buffer,
    image_index_buffer: wgpu::Buffer,
    image_texture_cache: LruCache<u64, CachedImageTexture>,
    translated_text_raster_origins: HashMap<NodeId, TranslatedTextRasterOrigin>,
    active_translated_text_nodes: HashSet<NodeId>,
    text_viewport: Viewport,
    scratch_shape_data: Vec<ShapeData>,
    scratch_gradients: Vec<GradientStop>,
    scratch_vertices: Vec<Vertex>,
    scratch_indices: Vec<u32>,
    scratch_image_vertices: Vec<Vertex>,
    scratch_image_indices: Vec<u32>,
    scratch_image_cmds: Vec<ImageDrawCmd>,
    scratch_segment_items: Vec<(usize, SegmentDrawItem)>,
    scratch_effect_ranges: Vec<Range<usize>>,
    scratch_layer_events: Vec<LayerEvent>,
    staged_uploads: StagedBufferUploads,
    effect_renderer: EffectRenderer,
    layer_surface_cache: LruCache<LayerRasterCacheKey, CachedLayerSurface>,
    layer_surface_cache_identity: HashMap<usize, LayerRasterCacheKey>,
    layer_surface_rect_cache: HashMap<usize, Rect>,
    layer_surface_requirements_cache: HashMap<usize, LayerSurfaceRequirements>,
    layer_surface_cache_bytes: u64,
    frame_stats: gpu_stats::FrameStats,
    last_frame_stats: Option<gpu_stats::FrameStatsSnapshot>,
    frame_count: u64,
    gpu_stats_enabled: bool,
}

fn snap_delta_for_anchor(anchor: SnapAnchor, root_scale: f32) -> Point {
    if !root_scale.is_finite() || root_scale <= 0.0 {
        return Point::default();
    }
    let device_pixel_step =
        if anchor.device_pixel_step.is_finite() && anchor.device_pixel_step > 0.0 {
            anchor.device_pixel_step
        } else {
            1.0
        };
    Point::new(
        ((anchor.origin.x * root_scale) / device_pixel_step).round() * device_pixel_step
            / root_scale
            - anchor.origin.x,
        ((anchor.origin.y * root_scale) / device_pixel_step).round() * device_pixel_step
            / root_scale
            - anchor.origin.y,
    )
}

fn image_sample_mode(image_draw: &ImageDraw) -> ImageSampleMode {
    if image_draw.motion_context_animated {
        ImageSampleMode::Linear
    } else {
        ImageSampleMode::Nearest
    }
}

#[cfg(test)]
fn layer_raster_cache_candidate(
    layer: &LayerNode,
    root_scale: f32,
    has_backdrop_underlay: bool,
    allow_runtime_cache: bool,
) -> Option<(LayerRasterCacheKey, Rect)> {
    let mut layer_surface_requirements_cache = HashMap::new();
    let surface_requirements =
        layer_surface_requirements_cached(layer, &mut layer_surface_requirements_cache);
    let cache_is_allowed = layer.cache_policy == CachePolicy::Auto
        || (allow_runtime_cache && surface_requirements.has_renderer_forced_surface());
    if !cache_is_allowed {
        return None;
    }
    if layer_uses_external_backdrop_input(layer, has_backdrop_underlay) {
        return None;
    }

    let logical_rect = estimate_layer_surface_rect(layer);
    let pixel_size = surface_target_size(logical_rect, root_scale, u32::MAX);
    Some((
        LayerRasterCacheKey::new(
            layer.node_id,
            layer.target_content_hash(),
            layer.effect_hash(),
            logical_rect,
            pixel_size,
            ScaleBucket::from_scale(root_scale),
        ),
        logical_rect,
    ))
}

impl GpuRenderer {
    pub fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
        adapter_backend: wgpu::Backend,
    ) -> Self {
        let uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Uniform Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        // Use uniform buffers for WebGL compatibility
        // Storage buffers aren't supported in WebGL fragment shaders
        let shape_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Shape Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let pipeline = create_shape_pipeline(
            &device,
            surface_format,
            &uniform_bind_group_layout,
            &shape_bind_group_layout,
            BlendMode::SrcOver,
        );
        let pipeline_dst_out = create_shape_pipeline(
            &device,
            surface_format,
            &uniform_bind_group_layout,
            &shape_bind_group_layout,
            BlendMode::DstOut,
        );

        let image_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Image Texture Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
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
            });

        let image_pipeline = create_image_pipeline(
            &device,
            surface_format,
            &uniform_bind_group_layout,
            &image_bind_group_layout,
            BlendMode::SrcOver,
        );
        let image_pipeline_dst_out = create_image_pipeline(
            &device,
            surface_format,
            &uniform_bind_group_layout,
            &image_bind_group_layout,
            BlendMode::DstOut,
        );

        let swash_cache = SwashCache::new();
        let glyphon_cache = Arc::new(Cache::new(&device));
        let mut text_atlas = TextAtlas::new(&device, &queue, &glyphon_cache, surface_format);

        log::info!(
            "Text renderer initialized with format: {:?}",
            surface_format
        );

        let first_slot = TextRendererSlot {
            renderer: TextRenderer::new(
                &mut text_atlas,
                &device,
                wgpu::MultisampleState::default(),
                None,
            ),
            last_signature: None,
        };
        let text_viewport = Viewport::new(&device, &glyphon_cache);

        #[cfg(not(target_arch = "wasm32"))]
        let upload_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Frame Upload Buffer"),
            size: INITIAL_UPLOAD_BUFFER_BYTES,
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create persistent uniform buffer
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Uniform Buffer"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Uniform Bind Group"),
            layout: &uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // Create persistent shape buffers
        let shape_buffers = ShapeBatchBuffers::new(&device, &shape_bind_group_layout);

        let image_sampler_nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Nearest Image Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let image_sampler_linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Linear Image Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let image_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Image Vertex Buffer"),
            size: (std::mem::size_of::<Vertex>() * 4) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let image_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Image Index Buffer"),
            size: (std::mem::size_of::<u32>() * 6) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let effect_renderer = EffectRenderer::new(&device, surface_format, adapter_backend);

        Self {
            device,
            queue,
            surface_format,
            pipeline,
            pipeline_dst_out,
            shape_bind_group_layout,
            image_pipeline,
            image_pipeline_dst_out,
            image_bind_group_layout,
            image_sampler_nearest,
            image_sampler_linear,
            text_renderer_pool: vec![first_slot],
            text_batch_cursor: 0,
            text_atlas,
            swash_cache,
            #[cfg(not(target_arch = "wasm32"))]
            upload_buffer,
            uniform_buffer,
            uniform_bind_group,
            shape_buffers,
            image_vertex_buffer,
            image_index_buffer,
            image_texture_cache: LruCache::new(
                NonZeroUsize::new(MAX_TEXTURE_CACHE_ITEMS)
                    .expect("image texture cache size must be non-zero"),
            ),
            translated_text_raster_origins: HashMap::new(),
            active_translated_text_nodes: HashSet::new(),
            text_viewport,
            scratch_shape_data: Vec::new(),
            scratch_gradients: Vec::new(),
            scratch_vertices: Vec::new(),
            scratch_indices: Vec::new(),
            scratch_image_vertices: Vec::new(),
            scratch_image_indices: Vec::new(),
            scratch_image_cmds: Vec::new(),
            scratch_segment_items: Vec::new(),
            scratch_effect_ranges: Vec::new(),
            scratch_layer_events: Vec::new(),
            staged_uploads: StagedBufferUploads::default(),
            effect_renderer,
            layer_surface_cache: LruCache::new(
                NonZeroUsize::new(MAX_LAYER_SURFACE_CACHE_ITEMS)
                    .expect("layer surface cache size must be non-zero"),
            ),
            layer_surface_cache_identity: HashMap::new(),
            layer_surface_rect_cache: HashMap::new(),
            layer_surface_requirements_cache: HashMap::new(),
            layer_surface_cache_bytes: 0,
            frame_stats: gpu_stats::FrameStats::default(),
            last_frame_stats: None,
            frame_count: 0,
            gpu_stats_enabled: gpu_stats_enabled(),
        }
    }

    fn ensure_image_cached(&mut self, image: &ImageBitmap) -> Result<(), String> {
        if self.image_texture_cache.get(&image.id()).is_some() {
            return Ok(());
        }

        let size = wgpu::Extent3d {
            width: image.width(),
            height: image.height(),
            depth_or_array_layers: 1,
        };

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Image Texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            image.pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * image.width()),
                rows_per_image: Some(image.height()),
            },
            size,
        );
        self.frame_stats
            .record_upload_bytes(image.pixels().len() as u64);

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let nearest_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Nearest Image Texture Bind Group"),
            layout: &self.image_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.image_sampler_nearest),
                },
            ],
        });
        let linear_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Linear Image Texture Bind Group"),
            layout: &self.image_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.image_sampler_linear),
                },
            ],
        });

        self.image_texture_cache.put(
            image.id(),
            CachedImageTexture {
                _texture: texture,
                _view: view,
                nearest_bind_group,
                linear_bind_group,
            },
        );
        Ok(())
    }

    fn translated_text_raster_origin(
        &mut self,
        node_id: NodeId,
        current_position: Point,
        root_scale: f32,
    ) -> Point {
        self.active_translated_text_nodes.insert(node_id);

        let origin = self
            .translated_text_raster_origins
            .entry(node_id)
            .or_insert(TranslatedTextRasterOrigin {
                position: current_position,
                root_scale,
            });
        if (origin.root_scale - root_scale).abs() > f32::EPSILON {
            *origin = TranslatedTextRasterOrigin {
                position: current_position,
                root_scale,
            };
        }

        origin.position
    }

    /// Acquire an offscreen target from the pool with stats tracking.
    /// Uses split borrows to avoid conflicting borrows on self.
    fn max_texture_dim(&self) -> u32 {
        self.effect_renderer.offscreen_pool.max_texture_dim()
    }

    fn acquire_offscreen(&mut self, width: u32, height: u32) -> OffscreenTarget {
        self.effect_renderer.offscreen_pool.acquire(
            &self.device,
            width,
            height,
            Some(&self.frame_stats),
        )
    }

    fn release_layer_surface_target(&mut self, target: LayerSurfaceTexture) {
        if let LayerSurfaceTexture::Owned(target) = target {
            self.effect_renderer.offscreen_pool.release(target);
        }
    }

    fn cached_layer_surface(
        &mut self,
        key: &LayerRasterCacheKey,
    ) -> Option<(Rc<OffscreenTarget>, Rect)> {
        let cached = self.layer_surface_cache.get(key)?;
        let (width, height) = key.pixel_size();
        self.frame_stats.record_layer_cache_hit(width, height);
        Some((cached.target.clone(), cached.logical_rect))
    }

    fn remove_cached_layer_surface(&mut self, key: &LayerRasterCacheKey) {
        let Some(entry) = self.layer_surface_cache.pop(key) else {
            return;
        };
        self.layer_surface_cache_bytes = self
            .layer_surface_cache_bytes
            .saturating_sub(entry.byte_size);
        if let Some(stable_id) = key.stable_id() {
            if self.layer_surface_cache_identity.get(&stable_id) == Some(key) {
                self.layer_surface_cache_identity.remove(&stable_id);
            }
        }
    }

    fn insert_cached_layer_surface(
        &mut self,
        key: LayerRasterCacheKey,
        target: OffscreenTarget,
        logical_rect: Rect,
    ) -> Rc<OffscreenTarget> {
        let byte_size = offscreen_byte_size(target.width, target.height);
        if let Some(stable_id) = key.stable_id() {
            if let Some(previous_key) = self.layer_surface_cache_identity.insert(stable_id, key) {
                if previous_key != key {
                    self.remove_cached_layer_surface(&previous_key);
                }
            }
        }

        while self.layer_surface_cache_bytes + byte_size > MAX_LAYER_SURFACE_CACHE_BYTES {
            let Some((evicted_key, evicted_entry)) = self.layer_surface_cache.pop_lru() else {
                break;
            };
            self.layer_surface_cache_bytes = self
                .layer_surface_cache_bytes
                .saturating_sub(evicted_entry.byte_size);
            if let Some(stable_id) = evicted_key.stable_id() {
                if self.layer_surface_cache_identity.get(&stable_id) == Some(&evicted_key) {
                    self.layer_surface_cache_identity.remove(&stable_id);
                }
            }
            self.frame_stats.record_layer_cache_eviction();
        }

        let cached = CachedLayerSurface {
            target: Rc::new(target),
            logical_rect,
            byte_size,
        };
        let cached_handle = cached.target.clone();
        if let Some((replaced_key, replaced_entry)) = self.layer_surface_cache.push(key, cached) {
            self.layer_surface_cache_bytes = self
                .layer_surface_cache_bytes
                .saturating_sub(replaced_entry.byte_size);
            if replaced_key != key {
                self.frame_stats.record_layer_cache_eviction();
            }
            if let Some(stable_id) = replaced_key.stable_id() {
                if self.layer_surface_cache_identity.get(&stable_id) == Some(&replaced_key) {
                    self.layer_surface_cache_identity.remove(&stable_id);
                }
            }
        }
        self.layer_surface_cache_bytes += byte_size;
        cached_handle
    }

    fn layer_raster_cache_candidate(
        &mut self,
        layer: &LayerNode,
        root_scale: f32,
        has_backdrop_underlay: bool,
        allow_runtime_cache: bool,
        logical_rect_override: Option<Rect>,
    ) -> Option<(LayerRasterCacheKey, Rect)> {
        let surface_requirements =
            layer_surface_requirements_cached(layer, &mut self.layer_surface_requirements_cache);
        let cache_is_allowed = layer.cache_policy == CachePolicy::Auto
            || (allow_runtime_cache && surface_requirements.has_renderer_forced_surface());
        if !cache_is_allowed {
            return None;
        }
        if layer_uses_external_backdrop_input(layer, has_backdrop_underlay) {
            return None;
        }

        let logical_rect = logical_rect_override.unwrap_or_else(|| {
            estimate_layer_surface_rect_cached(
                layer,
                &mut self.layer_surface_rect_cache,
                &mut self.layer_surface_requirements_cache,
            )
        });
        let pixel_size = surface_target_size(logical_rect, root_scale, self.max_texture_dim());
        Some((
            LayerRasterCacheKey::new(
                layer.node_id,
                layer.target_content_hash(),
                layer.effect_hash(),
                logical_rect,
                pixel_size,
                ScaleBucket::from_scale(root_scale),
            ),
            logical_rect,
        ))
    }

    fn supports_render_effect(&self, effect: &RenderEffect) -> bool {
        is_render_effect_supported(effect)
    }
}

impl SurfaceExecutionBackend for GpuRenderer {
    fn max_texture_dim(&self) -> u32 {
        GpuRenderer::max_texture_dim(self)
    }

    fn acquire_offscreen(&mut self, width: u32, height: u32) -> OffscreenTarget {
        GpuRenderer::acquire_offscreen(self, width, height)
    }

    fn release_offscreen(&mut self, target: OffscreenTarget) {
        self.effect_renderer.offscreen_pool.release(target);
    }

    fn release_layer_surface_target(&mut self, target: LayerSurfaceTexture) {
        GpuRenderer::release_layer_surface_target(self, target);
    }

    fn cached_layer_surface(
        &mut self,
        key: &LayerRasterCacheKey,
    ) -> Option<(Rc<OffscreenTarget>, Rect)> {
        GpuRenderer::cached_layer_surface(self, key)
    }

    fn insert_cached_layer_surface(
        &mut self,
        key: LayerRasterCacheKey,
        target: OffscreenTarget,
        logical_rect: Rect,
    ) -> Rc<OffscreenTarget> {
        GpuRenderer::insert_cached_layer_surface(self, key, target, logical_rect)
    }

    fn layer_raster_cache_candidate(
        &mut self,
        layer: &LayerNode,
        root_scale: f32,
        has_backdrop_underlay: bool,
        allow_runtime_cache: bool,
        logical_rect_override: Option<Rect>,
    ) -> Option<(LayerRasterCacheKey, Rect)> {
        GpuRenderer::layer_raster_cache_candidate(
            self,
            layer,
            root_scale,
            has_backdrop_underlay,
            allow_runtime_cache,
            logical_rect_override,
        )
    }

    fn layer_surface_requirements(&mut self, layer: &LayerNode) -> LayerSurfaceRequirements {
        layer_surface_requirements_cached(layer, &mut self.layer_surface_requirements_cache)
    }

    fn collect_layer_contents_with_translation_context<'a>(
        &mut self,
        layer: &'a LayerNode,
        inherited_clip: Option<Rect>,
        inherited_translated_snap_anchor: Option<SnapAnchor>,
        translation_context: TranslationRenderContext,
    ) -> CollectedLayer<'a> {
        collect_layer_contents_with_translation_context(
            layer,
            inherited_clip,
            inherited_translated_snap_anchor,
            translation_context,
            &mut self.layer_surface_rect_cache,
            &mut self.layer_surface_requirements_cache,
        )
    }

    fn clear_target_view_with_load_op(
        &self,
        target_view: &wgpu::TextureView,
        load_op: wgpu::LoadOp<wgpu::Color>,
    ) {
        GpuRenderer::clear_target_view_with_load_op(self, target_view, load_op);
    }

    fn render_non_effect_segment(
        &mut self,
        text_state: &mut TextSystemState,
        target_view: &wgpu::TextureView,
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadow_draws: &[ShadowDraw],
        z_start: usize,
        z_end: usize,
        effect_z_ranges: &[Range<usize>],
        width: u32,
        height: u32,
        root_scale: f32,
        initial_load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<(), String> {
        GpuRenderer::render_non_effect_segment(
            self,
            text_state,
            target_view,
            shapes,
            images,
            texts,
            shadow_draws,
            z_start,
            z_end,
            effect_z_ranges,
            width,
            height,
            root_scale,
            initial_load_op,
        )
    }

    fn render_range_with_layer_events_to_target(
        &mut self,
        text_state: &mut TextSystemState,
        target: &OffscreenTarget,
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadow_draws: &[ShadowDraw],
        effect_layers: &[EffectLayer],
        backdrop_layers: &[BackdropLayer],
        z_start: usize,
        z_end: usize,
        excluded_effect_layer: Option<usize>,
        width: u32,
        height: u32,
        root_scale: f32,
        backdrop_underlay: Option<&OffscreenTarget>,
        initial_load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<(), String> {
        GpuRenderer::render_range_with_layer_events_to_target(
            self,
            text_state,
            target,
            shapes,
            images,
            texts,
            shadow_draws,
            effect_layers,
            backdrop_layers,
            z_start,
            z_end,
            excluded_effect_layer,
            width,
            height,
            root_scale,
            backdrop_underlay,
            initial_load_op,
        )
    }

    fn render_shadow_draw(
        &mut self,
        text_state: &mut TextSystemState,
        target_view: &wgpu::TextureView,
        shadow: &ShadowDraw,
        width: u32,
        height: u32,
        root_scale: f32,
    ) {
        GpuRenderer::render_shadow_draw(
            self,
            text_state,
            target_view,
            shadow,
            width,
            height,
            root_scale,
        );
    }

    fn composite_to_view(
        &mut self,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        load_op: wgpu::LoadOp<wgpu::Color>,
        sample_mode: CompositeSampleMode,
    ) {
        self.effect_renderer.composite_to_view(
            &self.device,
            &self.queue,
            source,
            dest_view,
            load_op,
            sample_mode,
        );
    }

    fn composite_to_view_projective(
        &mut self,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        source_size: (f32, f32),
        inverse_matrix: [[f32; 3]; 3],
        dest_bounds: [[f32; 2]; 4],
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        blend_mode: BlendMode,
        sample_mode: CompositeSampleMode,
    ) {
        self.effect_renderer.composite_to_view_projective(
            &self.device,
            &self.queue,
            source,
            dest_view,
            viewport,
            source_size,
            inverse_matrix,
            dest_bounds,
            alpha,
            load_op,
            scissor,
            supported_blend_mode(blend_mode),
            sample_mode,
        );
    }

    fn composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
        &mut self,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        rounded_mask: Option<RoundedCompositeMask>,
        blend_mode: BlendMode,
        dest_viewport: Option<(f32, f32, f32, f32)>,
        sample_mode: CompositeSampleMode,
    ) {
        self.effect_renderer
            .composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
                &self.device,
                &self.queue,
                source,
                dest_view,
                alpha,
                load_op,
                scissor,
                rounded_mask,
                supported_blend_mode(blend_mode),
                dest_viewport,
                sample_mode,
            );
    }

    fn apply_effect(
        &mut self,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        effect: &RenderEffect,
        effect_rect: [f32; 4],
    ) {
        self.effect_renderer.apply_effect(
            &self.device,
            &self.queue,
            source,
            dest_view,
            effect,
            effect_rect,
        );
    }

    fn is_render_effect_supported(&self, effect: &RenderEffect) -> bool {
        self.supports_render_effect(effect)
    }

    fn warn_unsupported_effect_once(&self) {
        warn_unsupported_effect_once();
    }

    fn record_layer_cache_miss(&self, width: u32, height: u32) {
        self.frame_stats.record_layer_cache_miss(width, height);
    }

    fn record_isolated_layer_render(
        &self,
        width: u32,
        height: u32,
        node_id: Option<NodeId>,
        logical_rect: Rect,
        requirements: SurfaceRequirementSet,
    ) {
        self.frame_stats.record_isolated_layer_render(
            width,
            height,
            node_id,
            logical_rect,
            requirements.into(),
        );
    }
}

impl GpuRenderer {
    #[allow(clippy::too_many_arguments)]
    fn composite_offscreen_quad_to_view(
        &mut self,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        dest_quad: [[f32; 2]; 4],
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        blend_mode: BlendMode,
        sample_mode: CompositeSampleMode,
    ) -> Result<(), String> {
        if let Some(dest_rect) = axis_aligned_quad_rect(dest_quad) {
            self.effect_renderer
                .composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
                    &self.device,
                    &self.queue,
                    source,
                    dest_view,
                    alpha,
                    load_op,
                    scissor,
                    None,
                    supported_blend_mode(blend_mode),
                    Some((dest_rect.x, dest_rect.y, dest_rect.width, dest_rect.height)),
                    sample_mode,
                );
            return Ok(());
        }

        let source_rect = Rect {
            x: 0.0,
            y: 0.0,
            width: source.width as f32,
            height: source.height as f32,
        };
        let inverse = ProjectiveTransform::from_rect_to_quad(source_rect, dest_quad)
            .inverse()
            .ok_or_else(|| "root layer transform is not invertible".to_string())?;
        self.effect_renderer.composite_to_view_projective(
            &self.device,
            &self.queue,
            source,
            dest_view,
            viewport,
            (source_rect.width, source_rect.height),
            inverse.matrix(),
            dest_quad,
            alpha,
            load_op,
            scissor,
            supported_blend_mode(blend_mode),
            sample_mode,
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)] // Render path needs explicit scene slices and target metadata.
    pub fn render(
        &mut self,
        text_state: &mut TextSystemState,
        view: &wgpu::TextureView,
        graph: &RenderGraph,
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Result<(), String> {
        log::trace!("🎨 Rendering graph to {}x{}", width, height);

        self.text_batch_cursor = 0;
        self.active_translated_text_nodes.clear();

        let result = self.render_graph(text_state, view, graph, width, height, root_scale);
        if result.is_ok() {
            self.translated_text_raster_origins
                .retain(|node_id, _| self.active_translated_text_nodes.contains(node_id));
        }
        self.active_translated_text_nodes.clear();

        // Trim text renderer pool to the number of slots actually used this frame,
        // plus a small margin to avoid thrashing on minor batch count fluctuations.
        const TEXT_POOL_MARGIN: usize = 4;
        let target_pool_size = self.text_batch_cursor.saturating_add(TEXT_POOL_MARGIN);
        if self.text_renderer_pool.len() > target_pool_size {
            self.text_renderer_pool.truncate(target_pool_size);
        }

        self.frame_stats
            .offscreen_pool_size
            .set(self.effect_renderer.offscreen_pool.pool_size() as u32);
        self.frame_stats
            .offscreen_pool_bytes
            .set(self.effect_renderer.offscreen_pool.estimated_bytes() as u64);
        self.frame_stats
            .text_pool_size
            .set(self.text_renderer_pool.len() as u32);
        self.frame_stats
            .layer_cache_size
            .set(self.layer_surface_cache.len() as u32);
        self.frame_stats
            .layer_cache_bytes
            .set(self.layer_surface_cache_bytes);
        self.frame_stats
            .image_cache_size
            .set(self.image_texture_cache.len() as u32);
        self.frame_stats
            .text_cache_size
            .set(text_state.text_cache.len() as u32);
        self.effect_renderer
            .merge_and_reset_debug_counters(&self.frame_stats);
        let snapshot = self.frame_stats.snapshot();
        self.last_frame_stats = Some(snapshot);
        self.frame_stats.maybe_print_snapshot(
            snapshot,
            &mut self.frame_count,
            self.gpu_stats_enabled,
        );
        self.frame_stats.reset();
        result
    }

    pub fn last_frame_stats(&self) -> Option<gpu_stats::FrameStatsSnapshot> {
        self.last_frame_stats
    }

    #[allow(clippy::too_many_arguments)] // Mirrors render() call site and scene inputs.
    pub fn render_to_rgba_pixels(
        &mut self,
        text_state: &mut TextSystemState,
        graph: &RenderGraph,
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Result<Vec<u8>, String> {
        if width == 0 || height == 0 {
            return Err("Screenshot size must be non-zero".to_string());
        }

        let output_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Screenshot Output Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let output_view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());

        self.render(text_state, &output_view, graph, width, height, root_scale)?;

        let bytes_per_pixel = 4u32;
        let unpadded_bytes_per_row = width
            .checked_mul(bytes_per_pixel)
            .ok_or_else(|| "Screenshot row byte size overflow".to_string())?;
        let padded_bytes_per_row =
            align_to(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let output_buffer_size = padded_bytes_per_row as u64 * height as u64;

        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Screenshot Readback Buffer"),
            size: output_buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut copy_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Screenshot Copy Encoder"),
                });
        copy_encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &output_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &output_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let submission_index = self.queue.submit(std::iter::once(copy_encoder.finish()));

        let buffer_slice = output_buffer.slice(..);
        let (tx, rx) = mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        let _ = self.device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission_index),
            timeout: None,
        });

        match rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => {}
            Ok(Err(err)) => return Err(format!("Screenshot map_async failed: {err:?}")),
            Err(err) => return Err(format!("Screenshot readback timed out: {err}")),
        }

        let mapped = buffer_slice.get_mapped_range();
        let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];

        let src_row_len = padded_bytes_per_row as usize;
        let dst_row_len = unpadded_bytes_per_row as usize;
        for row in 0..height as usize {
            let src_offset = row * src_row_len;
            let dst_offset = row * dst_row_len;
            pixels[dst_offset..dst_offset + dst_row_len]
                .copy_from_slice(&mapped[src_offset..src_offset + dst_row_len]);
        }
        drop(mapped);
        output_buffer.unmap();

        self.convert_surface_pixels_to_rgba(&mut pixels)?;
        Ok(pixels)
    }

    fn render_graph(
        &mut self,
        text_state: &mut TextSystemState,
        surface_view: &wgpu::TextureView,
        graph: &RenderGraph,
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Result<(), String> {
        self.layer_surface_rect_cache.clear();
        self.layer_surface_requirements_cache.clear();
        if root_can_render_directly_cached(&graph.root, &mut self.layer_surface_requirements_cache)
        {
            let collected = collect_layer_contents(
                &graph.root,
                None,
                None,
                &mut self.layer_surface_rect_cache,
                &mut self.layer_surface_requirements_cache,
            );
            if !scene_has_layer_events(&collected.scene) {
                return self.render_root_direct(
                    text_state,
                    surface_view,
                    collected,
                    width,
                    height,
                    root_scale,
                );
            }
        }
        // The root layer's visible area is always the viewport — content
        // outside the screen is invisible regardless of scroll offsets or
        // inflated scene bounds.  Pass the viewport rect as an explicit
        // surface rect to prevent offscreen inflation on constrained GPUs.
        let viewport_rect = Rect {
            x: 0.0,
            y: 0.0,
            width: width as f32 / root_scale,
            height: height as f32 / root_scale,
        };
        let root_surface = self.render_layer_surface(
            text_state,
            &graph.root,
            LayerSurfaceRequest {
                root_scale,
                backdrop_underlay: None,
                allow_runtime_cache: false,
                logical_rect_override: Some(viewport_rect),
                activates_nested_capture: false,
                translation_context: TranslationRenderContext::default(),
            },
        )?;
        let root_quad = graph
            .root
            .transform_to_parent
            .map_rect(root_surface.logical_rect);
        let root_dest_quad = scaled_quad(root_quad, root_scale);

        let needs_root_composite_target =
            graph.root.backdrop().is_some() || graph.root.graphics_layer.shadow_elevation > 0.0;

        if needs_root_composite_target {
            let composite_target = self.acquire_offscreen(width, height);
            self.clear_target_view_with_load_op(
                &composite_target.view,
                wgpu::LoadOp::Clear(CLEAR_COLOR),
            );

            if let Some(backdrop) = graph.root.backdrop() {
                self.apply_backdrop_layer_to_target(
                    &composite_target,
                    &BackdropLayer {
                        rect: quad_bounds(
                            graph
                                .root
                                .transform_to_parent
                                .map_rect(graph.root.local_bounds),
                        ),
                        clip: graph
                            .root
                            .clip_rect()
                            .map(|clip| quad_bounds(graph.root.transform_to_parent.map_rect(clip))),
                        effect: backdrop.clone(),
                        z_index: 0,
                    },
                    None,
                    width,
                    height,
                    root_scale,
                )?;
            }

            let mut root_shadow_scene = CompositorScene::new();
            let root_shadow_clip = graph
                .root
                .shadow_clip
                .map(|clip| quad_bounds(graph.root.transform_to_parent.map_rect(clip)));
            push_layer_shadow(
                &mut root_shadow_scene,
                &graph.root.graphics_layer,
                graph.root.local_bounds,
                quad_bounds(
                    graph
                        .root
                        .transform_to_parent
                        .map_rect(graph.root.local_bounds),
                ),
                root_shadow_clip,
            );
            for shadow in &root_shadow_scene.shadow_draws {
                self.render_shadow_draw(
                    text_state,
                    &composite_target.view,
                    shadow,
                    width,
                    height,
                    root_scale,
                );
            }

            let composite_dest_quad =
                snap_motion_stable_dest_quad(root_dest_quad, root_surface.sample_mode);
            self.composite_offscreen_quad_to_view(
                root_surface.target.target(),
                &composite_target.view,
                (width, height),
                composite_dest_quad,
                root_surface.composite_alpha,
                wgpu::LoadOp::Load,
                None,
                root_surface.blend_mode,
                root_surface.sample_mode,
            )?;
            self.effect_renderer.composite_to_view(
                &self.device,
                &self.queue,
                &composite_target,
                surface_view,
                wgpu::LoadOp::Clear(CLEAR_COLOR),
                CompositeSampleMode::Linear,
            );
            self.effect_renderer
                .offscreen_pool
                .release(composite_target);
        } else {
            let composite_dest_quad =
                snap_motion_stable_dest_quad(root_dest_quad, root_surface.sample_mode);
            self.composite_offscreen_quad_to_view(
                root_surface.target.target(),
                surface_view,
                (width, height),
                composite_dest_quad,
                root_surface.composite_alpha,
                wgpu::LoadOp::Clear(CLEAR_COLOR),
                None,
                root_surface.blend_mode,
                root_surface.sample_mode,
            )?;
        }
        self.release_layer_surface_target(root_surface.target);
        Ok(())
    }

    fn render_root_direct(
        &mut self,
        text_state: &mut TextSystemState,
        surface_view: &wgpu::TextureView,
        collected: CollectedLayer<'_>,
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Result<(), String> {
        execute_render_root_direct(
            self,
            text_state,
            surface_view,
            collected,
            width,
            height,
            root_scale,
        )
    }

    fn render_layer_surface(
        &mut self,
        text_state: &mut TextSystemState,
        layer: &LayerNode,
        request: LayerSurfaceRequest<'_>,
    ) -> Result<LayerSurface, String> {
        execute_render_layer_surface(self, text_state, layer, request)
    }

    #[allow(clippy::too_many_arguments)]
    fn render_range_with_layer_events_to_target(
        &mut self,
        text_state: &mut TextSystemState,
        target: &OffscreenTarget,
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadow_draws: &[ShadowDraw],
        effect_layers: &[EffectLayer],
        backdrop_layers: &[BackdropLayer],
        z_start: usize,
        z_end: usize,
        excluded_effect_layer: Option<usize>,
        width: u32,
        height: u32,
        root_scale: f32,
        backdrop_underlay: Option<&OffscreenTarget>,
        initial_load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<(), String> {
        if z_start >= z_end {
            // Even if there's nothing to render, the caller may expect the
            // target to be cleared (e.g. freshly-acquired offscreen).
            if matches!(initial_load_op, wgpu::LoadOp::Clear(_)) {
                self.clear_target_view_with_load_op(&target.view, initial_load_op);
            }
            return Ok(());
        }

        let mut effect_z_ranges = std::mem::take(&mut self.scratch_effect_ranges);
        collect_effect_ranges(
            effect_layers,
            z_start,
            z_end,
            excluded_effect_layer,
            &mut effect_z_ranges,
        );
        let mut events = std::mem::take(&mut self.scratch_layer_events);
        collect_layer_events(
            effect_layers,
            backdrop_layers,
            z_start,
            z_end,
            excluded_effect_layer,
            &mut events,
        );

        // The first render_non_effect_segment uses the caller's load_op
        // (which may be Clear to fold a standalone clear).  After the first
        // segment or any layer event, subsequent segments use Load.
        let mut next_load_op = initial_load_op;

        let mut cursor_z = z_start;
        for event in &events {
            if event.z_index > cursor_z {
                self.render_non_effect_segment(
                    text_state,
                    &target.view,
                    shapes,
                    images,
                    texts,
                    shadow_draws,
                    cursor_z,
                    event.z_index,
                    &effect_z_ranges,
                    width,
                    height,
                    root_scale,
                    next_load_op,
                )?;
                next_load_op = wgpu::LoadOp::Load;
                cursor_z = event.z_index;
            } else if event.z_index < cursor_z {
                // Already consumed by a previously composited effect range.
                continue;
            }

            // Backdrop/effect events composite onto the target, so it must
            // be initialized first.
            if matches!(next_load_op, wgpu::LoadOp::Clear(_)) {
                self.clear_target_view_with_load_op(&target.view, next_load_op);
                next_load_op = wgpu::LoadOp::Load;
            }

            match event.kind {
                LayerEventKind::Backdrop(index) => {
                    self.apply_backdrop_layer_to_target(
                        target,
                        &backdrop_layers[index],
                        backdrop_underlay,
                        width,
                        height,
                        root_scale,
                    )?;
                }
                LayerEventKind::Effect(index) => {
                    let layer = &effect_layers[index];
                    if layer.z_start < cursor_z {
                        continue;
                    }
                    self.render_effect_layer_to_target(
                        text_state,
                        target,
                        shapes,
                        images,
                        texts,
                        shadow_draws,
                        effect_layers,
                        backdrop_layers,
                        index,
                        backdrop_underlay,
                        width,
                        height,
                        root_scale,
                    )?;
                    cursor_z = cursor_z.max(layer.z_end);
                }
            }
        }

        if cursor_z < z_end {
            self.render_non_effect_segment(
                text_state,
                &target.view,
                shapes,
                images,
                texts,
                shadow_draws,
                cursor_z,
                z_end,
                &effect_z_ranges,
                width,
                height,
                root_scale,
                next_load_op,
            )?;
        } else if matches!(next_load_op, wgpu::LoadOp::Clear(_)) {
            // All content was consumed by events but the target was never
            // cleared — do it now so the caller sees a clean target.
            self.clear_target_view_with_load_op(&target.view, next_load_op);
        }

        self.scratch_effect_ranges = effect_z_ranges;
        self.scratch_layer_events = events;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn render_non_effect_segment(
        &mut self,
        text_state: &mut TextSystemState,
        target_view: &wgpu::TextureView,
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadow_draws: &[ShadowDraw],
        z_start: usize,
        z_end: usize,
        effect_z_ranges: &[Range<usize>],
        width: u32,
        height: u32,
        root_scale: f32,
        initial_load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<(), String> {
        let mut ordered_items = std::mem::take(&mut self.scratch_segment_items);
        collect_non_effect_segment_items(
            shapes,
            images,
            texts,
            shadow_draws,
            z_start,
            z_end,
            effect_z_ranges,
            &mut ordered_items,
        );
        if ordered_items.is_empty() {
            if matches!(initial_load_op, wgpu::LoadOp::Clear(_)) {
                self.clear_target_view_with_load_op(target_view, initial_load_op);
            }
            return Ok(());
        }

        // Batch render passes into a shared encoder to reduce queue.submit()
        // calls. Buffer conflict tracking ensures we flush before rewriting
        // the same GPU buffers (shapes share shape_buffers, images share
        // image_buffers, and glyphon text prepare() reuses shared buffers).
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Segment Encoder"),
            });
        let mut encoder_has_work = false;
        let mut first_batch = true;
        for command in SegmentCommandIter::new(&ordered_items, shapes, images) {
            match command {
                SegmentRenderCommand::DrawChunk(chunk) => {
                    if encoder_has_work {
                        self.queue.submit(std::iter::once(encoder.finish()));
                        self.frame_stats.bump_submits();
                        encoder =
                            self.device
                                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                    label: Some("Segment Encoder"),
                                });
                        encoder_has_work = false;
                    }
                    let load_op = if first_batch {
                        initial_load_op
                    } else {
                        wgpu::LoadOp::Load
                    };
                    if self.render_segment_draw_chunk(
                        text_state,
                        &mut encoder,
                        target_view,
                        &ordered_items,
                        shapes,
                        images,
                        texts,
                        chunk,
                        width,
                        height,
                        root_scale,
                        load_op,
                    )? {
                        first_batch = false;
                        encoder_has_work = true;
                    }
                }
                SegmentRenderCommand::Shadow(index) => {
                    if first_batch && matches!(initial_load_op, wgpu::LoadOp::Clear(_)) {
                        // Record a clear pass so the shadow composites onto
                        // initialized content.
                        {
                            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("Shadow Pre-Clear"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: target_view,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations {
                                        load: initial_load_op,
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: None,
                                timestamp_writes: None,
                                occlusion_query_set: None,
                                multiview_mask: None,
                            });
                        }
                        first_batch = false;
                        encoder_has_work = true;
                    }
                    if encoder_has_work {
                        self.queue.submit(std::iter::once(encoder.finish()));
                        self.frame_stats.bump_submits();
                        encoder =
                            self.device
                                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                    label: Some("Segment Encoder"),
                                });
                        encoder_has_work = false;
                    }
                    self.render_shadow_draw(
                        text_state,
                        target_view,
                        &shadow_draws[index],
                        width,
                        height,
                        root_scale,
                    );
                }
            }
        }

        // Submit any remaining work.
        if encoder_has_work {
            self.queue.submit(std::iter::once(encoder.finish()));
            self.frame_stats.bump_submits();
        } else if first_batch && matches!(initial_load_op, wgpu::LoadOp::Clear(_)) {
            self.clear_target_view_with_load_op(target_view, initial_load_op);
        }

        self.scratch_segment_items = ordered_items;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn render_segment_draw_chunk(
        &mut self,
        text_state: &mut TextSystemState,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        ordered_items: &[(usize, SegmentDrawItem)],
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        chunk: SegmentDrawChunkPlan,
        width: u32,
        height: u32,
        root_scale: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<bool, String> {
        let mut prepared_batches: [Option<PreparedSegmentBatch>; 3] = [None, None, None];
        let mut prepared_len = 0usize;
        let mut staged_uploads = std::mem::take(&mut self.staged_uploads);
        staged_uploads.clear();
        let result = (|| {
            if chunk.needs_viewport_uniforms() {
                self.stage_viewport_uniforms(&mut staged_uploads, width, height, [0.0, 0.0]);
            }

            for batch in chunk.iter() {
                match batch {
                    SegmentBatchPlan::Shape {
                        start,
                        end,
                        blend_mode,
                    } => {
                        let Some(prepared) = self.prepare_shapes_batch(
                            ordered_items[start..end]
                                .iter()
                                .map(|(_, item)| match item {
                                    SegmentDrawItem::Shape(shape_index) => &shapes[*shape_index],
                                    _ => unreachable!("shape batch contains only shape items"),
                                }),
                            root_scale,
                            &mut staged_uploads,
                        ) else {
                            continue;
                        };
                        prepared_batches[prepared_len] = Some(PreparedSegmentBatch::Shape {
                            blend_mode,
                            batch: prepared,
                        });
                        prepared_len += 1;
                    }
                    SegmentBatchPlan::Image {
                        start,
                        end,
                        blend_mode,
                    } => {
                        let image_cmds = self.prepare_image_draw_cmds(
                            ordered_items[start..end]
                                .iter()
                                .map(|(_, item)| match item {
                                    SegmentDrawItem::Image(image_index) => &images[*image_index],
                                    _ => unreachable!("image batch contains only image items"),
                                }),
                            width,
                            height,
                            root_scale,
                            &mut staged_uploads,
                        )?;
                        if image_cmds.is_empty() {
                            self.scratch_image_cmds = image_cmds;
                            continue;
                        }
                        prepared_batches[prepared_len] = Some(PreparedSegmentBatch::Image {
                            blend_mode,
                            image_cmds,
                        });
                        prepared_len += 1;
                    }
                    SegmentBatchPlan::Text { start, end } => {
                        let slot = self.prepare_text_for_render(
                            text_state,
                            ordered_items[start..end]
                                .iter()
                                .map(|(_, item)| match item {
                                    SegmentDrawItem::Text(text_index) => &texts[*text_index],
                                    _ => unreachable!("text batch contains only text items"),
                                }),
                            width,
                            height,
                            root_scale,
                        )?;
                        prepared_batches[prepared_len] =
                            Some(PreparedSegmentBatch::Text { slot_index: slot });
                        prepared_len += 1;
                    }
                }
            }

            if prepared_len == 0 {
                return Ok(false);
            }

            self.flush_staged_uploads(encoder, &staged_uploads);
            let render_result = {
                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Segment Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: load_op,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });

                let mut result = Ok(());
                for batch in prepared_batches.iter_mut().filter_map(Option::as_mut) {
                    let draw_result = match batch {
                        PreparedSegmentBatch::Shape { blend_mode, batch } => {
                            self.draw_prepared_shapes(&mut render_pass, *blend_mode, *batch);
                            Ok(())
                        }
                        PreparedSegmentBatch::Image {
                            blend_mode,
                            image_cmds,
                        } => self.draw_prepared_images(&mut render_pass, image_cmds, *blend_mode),
                        PreparedSegmentBatch::Text { slot_index } => {
                            self.draw_prepared_text(&mut render_pass, *slot_index)
                        }
                    };
                    if let Err(err) = draw_result {
                        result = Err(err);
                        break;
                    }
                }
                result
            };

            for batch in prepared_batches.into_iter().flatten() {
                if let PreparedSegmentBatch::Image { image_cmds, .. } = batch {
                    self.scratch_image_cmds = image_cmds;
                }
            }

            render_result?;
            Ok(true)
        })();
        self.staged_uploads = staged_uploads;
        result
    }

    fn viewport_uniforms(width: u32, height: u32, viewport_offset: [f32; 2]) -> Uniforms {
        Uniforms {
            viewport: [width as f32, height as f32],
            viewport_offset,
        }
    }

    fn stage_viewport_uniforms(
        &self,
        staged_uploads: &mut StagedBufferUploads,
        width: u32,
        height: u32,
        viewport_offset: [f32; 2],
    ) {
        let uniforms = Self::viewport_uniforms(width, height, viewport_offset);
        staged_uploads.stage(UploadTarget::Uniform, bytemuck::bytes_of(&uniforms));
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_upload_buffer_capacity(&mut self, required_bytes: u64) {
        if required_bytes <= self.upload_buffer.size() {
            return;
        }

        let new_size = required_bytes
            .next_power_of_two()
            .max(INITIAL_UPLOAD_BUFFER_BYTES);
        self.upload_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Frame Upload Buffer"),
            size: new_size,
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
    }

    fn flush_staged_uploads(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        staged_uploads: &StagedBufferUploads,
    ) {
        if staged_uploads.is_empty() {
            return;
        }

        #[cfg(target_arch = "wasm32")]
        {
            let _ = encoder;
            for copy in &staged_uploads.copies {
                let payload = staged_uploads.payload_for_copy(*copy);
                self.frame_stats.record_upload_bytes(payload.len() as u64);
                match copy.target {
                    UploadTarget::Uniform => {
                        self.queue.write_buffer(&self.uniform_buffer, 0, payload);
                    }
                    UploadTarget::ShapeVertex => {
                        self.queue
                            .write_buffer(&self.shape_buffers.vertex_buffer, 0, payload);
                    }
                    UploadTarget::ShapeIndex => {
                        self.queue
                            .write_buffer(&self.shape_buffers.index_buffer, 0, payload);
                    }
                    UploadTarget::ShapeData => {
                        self.queue
                            .write_buffer(&self.shape_buffers.shape_buffer, 0, payload);
                    }
                    UploadTarget::ShapeGradient => {
                        self.queue
                            .write_buffer(&self.shape_buffers.gradient_buffer, 0, payload);
                    }
                    UploadTarget::ImageVertex => {
                        self.queue
                            .write_buffer(&self.image_vertex_buffer, 0, payload);
                    }
                    UploadTarget::ImageIndex => {
                        self.queue
                            .write_buffer(&self.image_index_buffer, 0, payload);
                    }
                }
            }
            return;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            self.ensure_upload_buffer_capacity(staged_uploads.bytes.len() as u64);
            self.frame_stats
                .record_upload_bytes(staged_uploads.bytes.len() as u64);
            self.queue
                .write_buffer(&self.upload_buffer, 0, &staged_uploads.bytes);

            for copy in &staged_uploads.copies {
                let target_buffer = match copy.target {
                    UploadTarget::Uniform => &self.uniform_buffer,
                    UploadTarget::ShapeVertex => &self.shape_buffers.vertex_buffer,
                    UploadTarget::ShapeIndex => &self.shape_buffers.index_buffer,
                    UploadTarget::ShapeData => &self.shape_buffers.shape_buffer,
                    UploadTarget::ShapeGradient => &self.shape_buffers.gradient_buffer,
                    UploadTarget::ImageVertex => &self.image_vertex_buffer,
                    UploadTarget::ImageIndex => &self.image_index_buffer,
                };
                encoder.copy_buffer_to_buffer(
                    &self.upload_buffer,
                    copy.source_offset,
                    target_buffer,
                    0,
                    copy.size,
                );
            }
        }
    }

    fn write_viewport_uniforms(&self, width: u32, height: u32, viewport_offset: [f32; 2]) {
        let uniforms = Self::viewport_uniforms(width, height, viewport_offset);
        self.frame_stats
            .record_upload_bytes(std::mem::size_of_val(&uniforms) as u64);
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
    }

    /// Renders a shadow via offscreen target + Gaussian blur + composite.
    fn render_shadow_draw(
        &mut self,
        text_state: &mut TextSystemState,
        target_view: &wgpu::TextureView,
        shadow: &ShadowDraw,
        width: u32,
        height: u32,
        root_scale: f32,
    ) {
        if shadow.shapes.is_empty() && shadow.texts.is_empty() {
            return;
        }

        let shape_bounds_opt = shadow
            .shapes
            .iter()
            .map(|(shape, _)| shape.rect)
            .reduce(|a, b| Rect {
                x: a.x.min(b.x),
                y: a.y.min(b.y),
                width: (a.x + a.width).max(b.x + b.width) - a.x.min(b.x),
                height: (a.y + a.height).max(b.y + b.height) - a.y.min(b.y),
            });

        let text_bounds_opt = shadow
            .texts
            .iter()
            .map(|text| text.rect)
            .reduce(|a, b| Rect {
                x: a.x.min(b.x),
                y: a.y.min(b.y),
                width: (a.x + a.width).max(b.x + b.width) - a.x.min(b.x),
                height: (a.y + a.height).max(b.y + b.height) - a.y.min(b.y),
            });

        let combined_bounds = match (shape_bounds_opt, text_bounds_opt) {
            (Some(s), Some(t)) => Some(Rect {
                x: s.x.min(t.x),
                y: s.y.min(t.y),
                width: (s.x + s.width).max(t.x + t.width) - s.x.min(t.x),
                height: (s.y + s.height).max(t.y + t.height) - s.y.min(t.y),
            }),
            (Some(s), None) => Some(s),
            (None, Some(t)) => Some(t),
            (None, None) => None,
        };

        let Some(shape_bounds) = combined_bounds else {
            return;
        };

        let blur_margin = blur_extent_margin(shadow.blur_radius);
        let mut blur_bounds = Rect {
            x: shape_bounds.x - blur_margin,
            y: shape_bounds.y - blur_margin,
            width: shape_bounds.width + blur_margin * 2.0,
            height: shape_bounds.height + blur_margin * 2.0,
        };
        if let Some(clip) = shadow.clip {
            let clip_expanded = Rect {
                x: clip.x - blur_margin,
                y: clip.y - blur_margin,
                width: clip.width + blur_margin * 2.0,
                height: clip.height + blur_margin * 2.0,
            };
            let Some(intersection) = blur_bounds.intersect(clip_expanded) else {
                return;
            };
            blur_bounds = intersection;
        }
        let processing_scissor = scissor_rect_for_rect(blur_bounds, root_scale, width, height);
        if processing_scissor.is_none() {
            return;
        }

        // Zero blur: render shapes directly to target (fast path).
        if shadow.blur_radius <= 0.0 {
            for (shape, blend_mode) in &shadow.shapes {
                self.render_shapes_to_offscreen(
                    target_view,
                    &[shape],
                    *blend_mode,
                    width,
                    height,
                    root_scale,
                    wgpu::LoadOp::Load,
                    [0.0, 0.0],
                );
            }
            if !shadow.texts.is_empty() {
                match self.prepare_text_for_render(
                    text_state,
                    shadow.texts.iter(),
                    width,
                    height,
                    root_scale,
                ) {
                    Ok(slot) => {
                        let mut encoder =
                            self.device
                                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                    label: Some("Zero Blur Shadow Text Encoder"),
                                });
                        if let Err(e) = self.encode_text_pass(
                            &mut encoder,
                            target_view,
                            wgpu::LoadOp::Load,
                            slot,
                        ) {
                            eprintln!("Failed to encode text for zero-blur shadow: {}", e);
                        }
                        self.queue.submit(std::iter::once(encoder.finish()));
                        self.frame_stats.bump_submits();
                    }
                    Err(e) => {
                        eprintln!("Failed to prepare text for zero-blur shadow: {}", e);
                    }
                }
            }
            return;
        }

        // Compute pixel-space bounds for the offscreen textures, clamped to viewport.
        let Some(device_bounds) =
            device_pixel_bounds_for_rect(blur_bounds, width, height, root_scale)
        else {
            return;
        };
        let bounds_x = device_bounds.x;
        let bounds_y = device_bounds.y;
        let bounds_w = device_bounds.width;
        let bounds_h = device_bounds.height;
        let pixel_radius = shadow.blur_radius * root_scale;

        // 1. Acquire bounds-sized offscreen source.
        let source = self.acquire_offscreen(bounds_w, bounds_h);

        // 2. Render shadow shapes to bounds-sized offscreen.
        //    The viewport_offset shifts the coordinate origin so that shapes
        //    at absolute viewport positions render into the small texture.
        //    The first shape gets LoadOp::Clear to initialize the texture;
        //    subsequent shapes use LoadOp::Load.
        let viewport_offset = [bounds_x, bounds_y];
        let mut first_shadow_item = true; // Tracks shapes and texts
        for (shape, blend_mode) in &shadow.shapes {
            let load = if first_shadow_item {
                first_shadow_item = false;
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
            } else {
                wgpu::LoadOp::Load
            };
            self.render_shapes_to_offscreen(
                &source.view,
                &[shape],
                *blend_mode,
                bounds_w,
                bounds_h,
                root_scale,
                load,
                viewport_offset,
            );
        }

        if !shadow.texts.is_empty() {
            let mut shifted_texts = shadow.texts.clone();
            for text in &mut shifted_texts {
                text.rect.x -= viewport_offset[0] / root_scale;
                text.rect.y -= viewport_offset[1] / root_scale;
                if let Some(clip) = text.clip.as_mut() {
                    clip.x -= viewport_offset[0] / root_scale;
                    clip.y -= viewport_offset[1] / root_scale;
                }
            }

            let load = if first_shadow_item {
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
            } else {
                wgpu::LoadOp::Load
            };

            match self.prepare_text_for_render(
                text_state,
                shifted_texts.iter(),
                bounds_w,
                bounds_h,
                root_scale,
            ) {
                Ok(slot) => {
                    let mut encoder =
                        self.device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("Shadow Text Encoder"),
                            });
                    if let Err(e) = self.encode_text_pass(&mut encoder, &source.view, load, slot) {
                        eprintln!("Failed to encode text for shadow: {}", e);
                    }
                    self.queue.submit(std::iter::once(encoder.finish()));
                    self.frame_stats.bump_submits();
                }
                Err(e) => {
                    eprintln!("Failed to prepare text for shadow: {}", e);
                }
            }
        }

        // 3. Apply Gaussian blur on the bounds-sized textures.
        let dest = self.acquire_offscreen(bounds_w, bounds_h);
        self.effect_renderer.apply_blur_scissored(
            &self.device,
            &self.queue,
            &source,
            &dest.view,
            pixel_radius,
            pixel_radius,
            TileMode::Decal,
            None, // No scissor needed — the texture is already bounds-sized
        );
        self.effect_renderer.offscreen_pool.release(source);

        // 4. Composite blurred result onto target at the correct position.
        let clip_scissor = shadow
            .clip
            .and_then(|clip| scissor_rect_for_rect(clip, root_scale, width, height));
        let scissor = clip_scissor.or(processing_scissor);
        let rounded_mask = inner_shadow_composite_mask(shadow, root_scale).map(|mut mask| {
            // Adjust mask coordinates from viewport-space to texture-local space,
            // since the blit shader computes world_pos = uv * tex_size.
            mask.rect[0] -= viewport_offset[0];
            mask.rect[1] -= viewport_offset[1];
            mask
        });
        let dest_viewport = Some((
            viewport_offset[0],
            viewport_offset[1],
            bounds_w as f32,
            bounds_h as f32,
        ));
        self.effect_renderer
            .composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
                &self.device,
                &self.queue,
                &dest,
                target_view,
                1.0,
                wgpu::LoadOp::Load,
                scissor,
                rounded_mask,
                BlendMode::SrcOver,
                dest_viewport,
                CompositeSampleMode::Linear,
            );
        self.effect_renderer.offscreen_pool.release(dest);

        // Restore viewport uniform to full size so subsequent image/text rendering
        // (which shares the uniform_buffer but doesn't write to it) works correctly.
        self.write_viewport_uniforms(width, height, [0.0, 0.0]);
    }

    #[allow(clippy::too_many_arguments)]
    fn render_effect_layer_to_target(
        &mut self,
        text_state: &mut TextSystemState,
        target: &OffscreenTarget,
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadow_draws: &[ShadowDraw],
        effect_layers: &[EffectLayer],
        backdrop_layers: &[BackdropLayer],
        effect_layer_index: usize,
        backdrop_underlay: Option<&OffscreenTarget>,
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Result<(), String> {
        execute_render_effect_layer_to_target(
            self,
            text_state,
            target,
            shapes,
            images,
            texts,
            shadow_draws,
            effect_layers,
            backdrop_layers,
            effect_layer_index,
            backdrop_underlay,
            width,
            height,
            root_scale,
        )
    }

    fn apply_backdrop_layer_to_target(
        &mut self,
        target: &OffscreenTarget,
        layer: &BackdropLayer,
        backdrop_underlay: Option<&OffscreenTarget>,
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Result<(), String> {
        execute_apply_backdrop_layer_to_target(
            self,
            target,
            layer,
            backdrop_underlay,
            width,
            height,
            root_scale,
        )
    }

    fn clear_target_view_with_load_op(
        &self,
        target_view: &wgpu::TextureView,
        load_op: wgpu::LoadOp<wgpu::Color>,
    ) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Layer Event Clear Encoder"),
            });
        {
            let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Layer Event Clear Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        self.frame_stats.bump_submits();
    }

    /// Render a subset of shapes to a target view.
    ///
    /// Uses the same shape pipeline and uniforms as the main render path.
    /// The `load_op` controls whether to clear or preserve existing content.
    #[allow(clippy::too_many_arguments)]
    fn render_shapes_to_offscreen(
        &mut self,
        target_view: &wgpu::TextureView,
        layer_shapes: &[&DrawShape],
        blend_mode: BlendMode,
        width: u32,
        height: u32,
        root_scale: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        viewport_offset: [f32; 2],
    ) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Shape Encoder"),
            });
        self.encode_shapes_pass(
            &mut encoder,
            target_view,
            layer_shapes.iter().copied(),
            blend_mode,
            width,
            height,
            root_scale,
            load_op,
            viewport_offset,
        );
        self.queue.submit(std::iter::once(encoder.finish()));
        self.frame_stats.bump_submits();
    }

    fn prepare_shapes_batch<'a, I>(
        &mut self,
        layer_shapes: I,
        root_scale: f32,
        staged_uploads: &mut StagedBufferUploads,
    ) -> Option<PreparedShapeBatch>
    where
        I: Iterator<Item = &'a DrawShape>,
    {
        // Build shape data for this subset
        self.scratch_shape_data.clear();
        self.scratch_gradients.clear();
        self.scratch_vertices.clear();
        self.scratch_indices.clear();

        for (idx, shape) in layer_shapes.enumerate() {
            let snap_delta = shape
                .snap_anchor
                .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
                .unwrap_or_default();
            let local_rect = shape.local_rect.translate(snap_delta.x, snap_delta.y);
            let quad = translate_quad(shape.quad, snap_delta);
            let clip = shape
                .clip
                .map(|clip| clip.translate(snap_delta.x, snap_delta.y));

            // Clip rect (scaled to physical pixels)
            let clip_rect = if let Some(clip) = clip {
                [
                    clip.x * root_scale,
                    clip.y * root_scale,
                    clip.width * root_scale,
                    clip.height * root_scale,
                ]
            } else {
                [0.0, 0.0, 0.0, 0.0]
            };

            // Gradient parameters
            let mut gradient_params = [0.0f32; 4];
            let mut push_gradient_entries = |colors: &[Color], stops: Option<&[f32]>| {
                let start = self.scratch_gradients.len() as u32;
                let count = colors.len();
                let explicit_stops = stops.filter(|values| values.len() == count);
                for (index, color) in colors.iter().enumerate() {
                    let position =
                        explicit_stops
                            .map(|values| values[index])
                            .unwrap_or_else(|| {
                                if count <= 1 {
                                    0.0
                                } else {
                                    index as f32 / (count - 1) as f32
                                }
                            });
                    self.scratch_gradients.push(GradientStop {
                        color: [color.r(), color.g(), color.b(), color.a()],
                        position: [position, 0.0, 0.0, 0.0],
                    });
                }
                (start, count as u32)
            };
            let (brush_type, gradient_start, gradient_count, gradient_tile_mode) = match &shape
                .brush
            {
                Brush::Solid(_) => (0u32, 0u32, 0u32, gradient_tile_mode_value(TileMode::Clamp)),
                Brush::LinearGradient {
                    colors,
                    stops,
                    start,
                    end,
                    tile_mode,
                } => {
                    let (start_idx, count) = push_gradient_entries(colors, stops.as_deref());
                    gradient_params = [
                        resolve_gradient_point(
                            local_rect.x * root_scale,
                            local_rect.width * root_scale,
                            start.x * root_scale,
                        ),
                        resolve_gradient_point(
                            local_rect.y * root_scale,
                            local_rect.height * root_scale,
                            start.y * root_scale,
                        ),
                        resolve_gradient_point(
                            local_rect.x * root_scale,
                            local_rect.width * root_scale,
                            end.x * root_scale,
                        ),
                        resolve_gradient_point(
                            local_rect.y * root_scale,
                            local_rect.height * root_scale,
                            end.y * root_scale,
                        ),
                    ];
                    (1u32, start_idx, count, gradient_tile_mode_value(*tile_mode))
                }
                Brush::RadialGradient {
                    colors,
                    stops,
                    center,
                    radius,
                    tile_mode,
                } => {
                    let (start_idx, count) = push_gradient_entries(colors, stops.as_deref());
                    gradient_params = [
                        local_rect.x * root_scale + center.x * root_scale,
                        local_rect.y * root_scale + center.y * root_scale,
                        (radius * root_scale).max(f32::EPSILON),
                        0.0,
                    ];
                    (2u32, start_idx, count, gradient_tile_mode_value(*tile_mode))
                }
                Brush::SweepGradient {
                    colors,
                    stops,
                    center,
                } => {
                    let (start_idx, count) = push_gradient_entries(colors, stops.as_deref());
                    gradient_params = [
                        local_rect.x * root_scale + center.x * root_scale,
                        local_rect.y * root_scale + center.y * root_scale,
                        0.0,
                        0.0,
                    ];
                    (
                        3u32,
                        start_idx,
                        count,
                        gradient_tile_mode_value(TileMode::Clamp),
                    )
                }
            };

            let radii = if let Some(rounded) = shape.shape {
                let resolved = rounded.resolve(local_rect.width, local_rect.height);
                [
                    resolved.top_left * root_scale,
                    resolved.top_right * root_scale,
                    resolved.bottom_left * root_scale,
                    resolved.bottom_right * root_scale,
                ]
            } else {
                [0.0, 0.0, 0.0, 0.0]
            };

            let device_rect = [
                local_rect.x * root_scale,
                local_rect.y * root_scale,
                local_rect.width * root_scale,
                local_rect.height * root_scale,
            ];

            self.scratch_shape_data.push(ShapeData {
                rect: device_rect,
                radii,
                gradient_params,
                clip_rect,
                brush_type,
                gradient_start,
                gradient_count,
                gradient_tile_mode,
            });

            let base_vertex = (idx * 4) as u32;
            let color = match &shape.brush {
                Brush::Solid(c) => [c.r(), c.g(), c.b(), c.a()],
                Brush::LinearGradient { colors, .. } => {
                    let first = colors.first().unwrap_or(&Color(1.0, 1.0, 1.0, 1.0));
                    [first.r(), first.g(), first.b(), first.a()]
                }
                Brush::RadialGradient { colors, .. } | Brush::SweepGradient { colors, .. } => {
                    let first = colors.first().unwrap_or(&Color(1.0, 1.0, 1.0, 1.0));
                    [first.r(), first.g(), first.b(), first.a()]
                }
            };

            let vertices = [
                [quad[0][0] * root_scale, quad[0][1] * root_scale],
                [quad[1][0] * root_scale, quad[1][1] * root_scale],
                [quad[2][0] * root_scale, quad[2][1] * root_scale],
                [quad[3][0] * root_scale, quad[3][1] * root_scale],
            ];

            self.scratch_vertices.extend_from_slice(&[
                Vertex {
                    position: vertices[0],
                    color,
                    uv: [0.0, 0.0],
                },
                Vertex {
                    position: vertices[1],
                    color,
                    uv: [1.0, 0.0],
                },
                Vertex {
                    position: vertices[2],
                    color,
                    uv: [0.0, 1.0],
                },
                Vertex {
                    position: vertices[3],
                    color,
                    uv: [1.0, 1.0],
                },
            ]);

            self.scratch_indices.extend_from_slice(&[
                base_vertex,
                base_vertex + 1,
                base_vertex + 2,
                base_vertex + 2,
                base_vertex + 1,
                base_vertex + 3,
            ]);
        }

        if self.scratch_vertices.is_empty() {
            return None;
        }

        let shape_count = self.scratch_shape_data.len();
        if shape_count == 0 {
            return None;
        }

        // Ensure buffers have capacity
        self.shape_buffers.ensure_capacity(
            &self.device,
            &self.shape_bind_group_layout,
            shape_count * 4,
            shape_count * 6,
            shape_count,
            self.scratch_gradients.len().max(1),
        );

        staged_uploads.stage(
            UploadTarget::ShapeVertex,
            bytemuck::cast_slice(&self.scratch_vertices),
        );
        staged_uploads.stage(
            UploadTarget::ShapeIndex,
            bytemuck::cast_slice(&self.scratch_indices),
        );
        staged_uploads.stage(
            UploadTarget::ShapeData,
            bytemuck::cast_slice(&self.scratch_shape_data),
        );
        if !self.scratch_gradients.is_empty() {
            staged_uploads.stage(
                UploadTarget::ShapeGradient,
                bytemuck::cast_slice(&self.scratch_gradients),
            );
        }
        Some(PreparedShapeBatch {
            index_count: shape_count as u32 * 6,
        })
    }

    fn draw_prepared_shapes(
        &self,
        render_pass: &mut wgpu::RenderPass<'_>,
        blend_mode: BlendMode,
        batch: PreparedShapeBatch,
    ) {
        self.frame_stats.bump_shapes();
        render_pass.set_pipeline(match blend_mode {
            BlendMode::DstOut => &self.pipeline_dst_out,
            _ => &self.pipeline,
        });
        render_pass.set_bind_group(0, &self.uniform_bind_group, &[]);
        render_pass.set_bind_group(1, &self.shape_buffers.bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.shape_buffers.vertex_buffer.slice(..));
        render_pass.set_index_buffer(
            self.shape_buffers.index_buffer.slice(..),
            wgpu::IndexFormat::Uint32,
        );
        render_pass.draw_indexed(0..batch.index_count, 0, 0..1);
    }

    /// Stage shape buffer writes and record a shape render pass onto the
    /// provided encoder. The caller is responsible for submitting.
    #[allow(clippy::too_many_arguments)]
    fn encode_shapes_pass<'a, I>(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        layer_shapes: I,
        blend_mode: BlendMode,
        width: u32,
        height: u32,
        root_scale: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        viewport_offset: [f32; 2],
    ) where
        I: Iterator<Item = &'a DrawShape>,
    {
        let mut staged_uploads = std::mem::take(&mut self.staged_uploads);
        staged_uploads.clear();
        self.stage_viewport_uniforms(&mut staged_uploads, width, height, viewport_offset);
        let Some(batch) = self.prepare_shapes_batch(layer_shapes, root_scale, &mut staged_uploads)
        else {
            self.staged_uploads = staged_uploads;
            return;
        };
        self.flush_staged_uploads(encoder, &staged_uploads);
        self.staged_uploads = staged_uploads;
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Shape Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: load_op,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.draw_prepared_shapes(&mut render_pass, blend_mode, batch);
    }

    fn draw_prepared_images(
        &mut self,
        render_pass: &mut wgpu::RenderPass<'_>,
        image_cmds: &[ImageDrawCmd],
        blend_mode: BlendMode,
    ) -> Result<(), String> {
        if image_cmds.is_empty() {
            return Ok(());
        }
        self.frame_stats.bump_images();
        render_pass.set_pipeline(match blend_mode {
            BlendMode::DstOut => &self.image_pipeline_dst_out,
            _ => &self.image_pipeline,
        });
        render_pass.set_bind_group(0, &self.uniform_bind_group, &[]);
        render_pass.set_index_buffer(self.image_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        render_pass.set_vertex_buffer(0, self.image_vertex_buffer.slice(..));

        for cmd in image_cmds {
            let (sx, sy, sw, sh) = cmd.scissor;
            render_pass.set_scissor_rect(sx, sy, sw, sh);

            let cached = self
                .image_texture_cache
                .get(&cmd.image_id)
                .ok_or_else(|| "image texture missing from cache".to_string())?;
            let bind_group = match cmd.sample_mode {
                ImageSampleMode::Nearest => &cached.nearest_bind_group,
                ImageSampleMode::Linear => &cached.linear_bind_group,
            };
            render_pass.set_bind_group(1, bind_group, &[]);
            render_pass.draw_indexed(cmd.index_start..(cmd.index_start + 6), 0, 0..1);
        }
        Ok(())
    }

    /// Prepare image vertices, indices, ensure caching, and write to GPU buffers.
    /// Returns the draw commands needed by `encode_images_pass`.
    fn prepare_image_draw_cmds<'a, I>(
        &mut self,
        layer_images: I,
        width: u32,
        height: u32,
        root_scale: f32,
        staged_uploads: &mut StagedBufferUploads,
    ) -> Result<Vec<ImageDrawCmd>, String>
    where
        I: Iterator<Item = &'a ImageDraw>,
    {
        let mut image_vertices = std::mem::take(&mut self.scratch_image_vertices);
        let mut image_indices = std::mem::take(&mut self.scratch_image_indices);
        let mut image_cmds = std::mem::take(&mut self.scratch_image_cmds);
        image_vertices.clear();
        image_indices.clear();
        image_cmds.clear();

        for image_draw in layer_images {
            let snap_delta = image_draw
                .snap_anchor
                .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
                .unwrap_or_default();
            let rect = image_draw.rect.translate(snap_delta.x, snap_delta.y);
            if rect.width <= 0.0 || rect.height <= 0.0 || image_draw.alpha <= 0.0 {
                continue;
            }

            let (tint, cpu_filter) = tint_for_image(image_draw.color_filter, image_draw.alpha);
            if tint[3] <= 0.0 {
                continue;
            }

            let prepared_image = if let Some(filter) = cpu_filter {
                apply_filter_to_bitmap(&image_draw.image, filter)?
            } else {
                image_draw.image.clone()
            };
            self.ensure_image_cached(&prepared_image)?;

            let adjusted_image = ImageDraw {
                rect,
                local_rect: image_draw.local_rect.translate(snap_delta.x, snap_delta.y),
                quad: translate_quad(image_draw.quad, snap_delta),
                snap_anchor: image_draw.snap_anchor,
                image: image_draw.image.clone(),
                alpha: image_draw.alpha,
                color_filter: image_draw.color_filter,
                z_index: image_draw.z_index,
                clip: image_draw
                    .clip
                    .map(|clip| clip.translate(snap_delta.x, snap_delta.y)),
                blend_mode: image_draw.blend_mode,
                src_rect: image_draw.src_rect,
                motion_context_animated: image_draw.motion_context_animated,
            };
            let scissor = scissor_rect_for_image(&adjusted_image, root_scale, width, height);
            let Some(scissor) = scissor else {
                continue;
            };

            let (u_min, v_min, u_max, v_max) = if let Some(sr) = image_draw.src_rect {
                let iw = image_draw.image.width() as f32;
                let ih = image_draw.image.height() as f32;
                (
                    sr.x / iw,
                    sr.y / ih,
                    (sr.x + sr.width) / iw,
                    (sr.y + sr.height) / ih,
                )
            } else {
                (0.0, 0.0, 1.0, 1.0)
            };

            let base_vertex = image_vertices.len() as u32;
            let index_start = image_indices.len() as u32;
            image_indices.extend_from_slice(&[
                base_vertex,
                base_vertex + 1,
                base_vertex + 2,
                base_vertex + 2,
                base_vertex + 1,
                base_vertex + 3,
            ]);
            image_vertices.extend_from_slice(&[
                Vertex {
                    position: [
                        adjusted_image.quad[0][0] * root_scale,
                        adjusted_image.quad[0][1] * root_scale,
                    ],
                    color: tint,
                    uv: [u_min, v_min],
                },
                Vertex {
                    position: [
                        adjusted_image.quad[1][0] * root_scale,
                        adjusted_image.quad[1][1] * root_scale,
                    ],
                    color: tint,
                    uv: [u_max, v_min],
                },
                Vertex {
                    position: [
                        adjusted_image.quad[2][0] * root_scale,
                        adjusted_image.quad[2][1] * root_scale,
                    ],
                    color: tint,
                    uv: [u_min, v_max],
                },
                Vertex {
                    position: [
                        adjusted_image.quad[3][0] * root_scale,
                        adjusted_image.quad[3][1] * root_scale,
                    ],
                    color: tint,
                    uv: [u_max, v_max],
                },
            ]);

            image_cmds.push(ImageDrawCmd {
                index_start,
                scissor,
                image_id: prepared_image.id(),
                sample_mode: image_sample_mode(image_draw),
            });
        }

        if image_cmds.is_empty() {
            return Ok(image_cmds);
        }

        // Resize buffers if needed
        let needed_bytes = (image_vertices.len() * std::mem::size_of::<Vertex>()) as u64;
        if needed_bytes > self.image_vertex_buffer.size() {
            self.image_vertex_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Image Vertex Buffer"),
                size: needed_bytes,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        let needed_index_bytes = (image_indices.len() * std::mem::size_of::<u32>()) as u64;
        if needed_index_bytes > self.image_index_buffer.size() {
            self.image_index_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Image Index Buffer"),
                size: needed_index_bytes,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }

        staged_uploads.stage(
            UploadTarget::ImageVertex,
            bytemuck::cast_slice(&image_vertices),
        );
        staged_uploads.stage(
            UploadTarget::ImageIndex,
            bytemuck::cast_slice(&image_indices),
        );

        self.scratch_image_vertices = image_vertices;
        self.scratch_image_indices = image_indices;
        // image_cmds is returned to the caller; it will NOT be returned to
        // scratch_image_cmds here. The caller must not hold it across calls.
        Ok(image_cmds)
    }

    /// Prepare text shaping and atlas uploads for the given text batch.
    ///
    /// Each call claims the next slot from `text_renderer_pool` (growing the
    /// pool on demand).  The slot independently caches its batch signature so
    /// that repeated identical batches at the same position across frames can
    /// skip the expensive `glyphon::TextRenderer::prepare()` call.
    ///
    /// Returns the pool slot index; pass it to `encode_text_pass` to render.
    fn prepare_text_for_render<'a, I>(
        &mut self,
        text_state: &mut TextSystemState,
        layer_texts: I,
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Result<usize, String>
    where
        I: Clone + Iterator<Item = &'a TextDraw>,
    {
        let slot_index = self.text_batch_cursor;
        self.text_batch_cursor += 1;

        // Grow pool on demand.
        while self.text_renderer_pool.len() <= slot_index {
            self.text_renderer_pool.push(TextRendererSlot {
                renderer: TextRenderer::new(
                    &mut self.text_atlas,
                    &self.device,
                    wgpu::MultisampleState::default(),
                    None,
                ),
                last_signature: None,
            });
        }

        let telemetry_enabled = text_render_telemetry_enabled();
        let call_sequence = if telemetry_enabled {
            TEXT_RENDER_CALLS.fetch_add(1, Ordering::Relaxed) + 1
        } else {
            0
        };

        // Skip prepare() when the batch signature matches — the slot's cached vertex
        // buffers are still valid. We defer text_atlas.trim() whenever any slot skips
        // so that unreferenced-but-still-needed glyphs aren't evicted.
        let batch_signature =
            prepared_text_batch_signature(layer_texts.clone(), width, height, root_scale);
        if batch_signature.is_some()
            && self.text_renderer_pool[slot_index].last_signature == batch_signature
        {
            // Viewport must be updated even on skip: encode_text_pass uses it for
            // projection. Shadow draws pass offscreen dimensions (bounds_w × bounds_h)
            // which differ from full-screen resolution used by normal text batches.
            // Without this update, a skipped shadow text renders with the stale
            // full-screen viewport, mapping its vertices to wrong coordinates.
            self.text_viewport
                .update(&self.queue, Resolution { width, height });
            if telemetry_enabled {
                let skips = TEXT_RENDER_SKIPS.fetch_add(1, Ordering::Relaxed) + 1;
                if call_sequence.is_multiple_of(20) {
                    log::warn!(
                        "[text-render-telemetry] calls={} skips={} skip_rate={:.1}% (prepare reused, slot={})",
                        call_sequence,
                        skips,
                        (skips as f64 / call_sequence as f64) * 100.0,
                        slot_index
                    );
                }
            }
            return Ok(slot_index);
        }

        let prepare_started = telemetry_enabled.then(std::time::Instant::now);
        let (font_system, font_family_resolver, text_cache) = text_state.parts_mut();

        let mut text_keys: Vec<TextCacheKey> =
            Vec::with_capacity(layer_texts.clone().size_hint().0);
        let mut valid_items = 0usize;
        let mut total_chars = 0usize;

        for text_draw in layer_texts.clone() {
            if text_draw.text.is_empty()
                || text_draw.rect.width <= 0.0
                || text_draw.rect.height <= 0.0
            {
                continue;
            }
            valid_items += 1;
            total_chars += text_draw.text.text.len();

            let font_size_px = text_draw.font_size * text_draw.scale * root_scale;
            let style_hash = crate::text_buffer_style_hash(&text_draw.text_style, &text_draw.text);
            let line_height_px = crate::resolve_effective_line_height(
                &text_draw.text_style,
                &text_draw.text,
                font_size_px,
            );
            let key = TextCacheKey::for_node(text_draw.node_id, font_size_px, style_hash);

            let (cache_hit, _, _, buffer) = crate::shared_text_buffer_mut(
                text_cache,
                key.clone(),
                font_system,
                font_size_px,
                line_height_px,
            );

            let reshaped = buffer.ensure(
                font_system,
                font_family_resolver,
                EnsureTextBufferParams {
                    annotated_text: &text_draw.text,
                    font_size_px,
                    line_height_px,
                    style_hash,
                    style: &text_draw.text_style,
                    scale: text_draw.scale * root_scale,
                },
            );

            if telemetry_enabled {
                if cache_hit {
                    TEXT_RENDER_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
                } else {
                    TEXT_RENDER_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
                }
                if reshaped {
                    TEXT_RENDER_ENSURE_RESHAPES.fetch_add(1, Ordering::Relaxed);
                } else {
                    TEXT_RENDER_ENSURE_REUSES.fetch_add(1, Ordering::Relaxed);
                }
            }

            text_keys.push(key);
        }

        // Build text areas
        let mut text_areas = Vec::with_capacity(text_keys.len());
        let mut key_idx = 0;

        for text_draw in layer_texts {
            if text_draw.text.is_empty()
                || text_draw.rect.width <= 0.0
                || text_draw.rect.height <= 0.0
            {
                continue;
            }

            let key = &text_keys[key_idx];
            key_idx += 1;

            let cached = text_cache.peek(key).expect("Text should be in cache");

            let color = GlyphonColor::rgba(
                (text_draw.color.r() * 255.0) as u8,
                (text_draw.color.g() * 255.0) as u8,
                (text_draw.color.b() * 255.0) as u8,
                (text_draw.color.a() * 255.0) as u8,
            );

            let snap_delta = text_draw
                .snap_anchor
                .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
                .unwrap_or_default();
            let rect = text_draw.rect.translate(snap_delta.x, snap_delta.y);
            let left_px = rect.x * root_scale;
            let top_px = rect.y * root_scale;
            let current_position = Point::new(left_px, top_px);
            let (cache_origin, subpixel_y) = if text_draw.translated_content_context {
                (
                    self.translated_text_raster_origin(
                        text_draw.node_id,
                        current_position,
                        root_scale,
                    ),
                    true,
                )
            } else {
                self.translated_text_raster_origins
                    .remove(&text_draw.node_id);
                (current_position, false)
            };

            let adjusted_clip = text_draw
                .clip
                .map(|clip| clip.translate(snap_delta.x, snap_delta.y));
            let Some(bounds) = text_bounds_for_clip(adjusted_clip, root_scale, width, height)
            else {
                continue;
            };

            text_areas.push(TextArea {
                buffer: &cached.buffer,
                left: left_px,
                top: top_px,
                cache_left: cache_origin.x,
                cache_top: cache_origin.y,
                subpixel_y,
                scale: 1.0,
                bounds,
                default_color: color,
                custom_glyphs: &[],
            });
        }

        self.text_viewport
            .update(&self.queue, Resolution { width, height });
        let prepare_result = self.text_renderer_pool[slot_index].renderer.prepare(
            &self.device,
            &self.queue,
            font_system,
            &mut self.text_atlas,
            &self.text_viewport,
            text_areas.iter().cloned(),
            &mut self.swash_cache,
        );

        if let Err(ref e) = prepare_result {
            // Safety net for AtlasFull: trim to reclaim space, invalidate all
            // cached signatures (their vertex data references freed atlas positions),
            // and retry once.
            let err_msg = format!("{e:?}");
            if err_msg.contains("%.%.") || err_msg.contains("Atlas") {
                log::warn!("[text-render] atlas full, trimming and retrying");
                for slot in &mut self.text_renderer_pool {
                    slot.last_signature = None;
                }
                self.text_atlas.trim();
                self.text_renderer_pool[slot_index]
                    .renderer
                    .prepare(
                        &self.device,
                        &self.queue,
                        font_system,
                        &mut self.text_atlas,
                        &self.text_viewport,
                        text_areas.iter().cloned(),
                        &mut self.swash_cache,
                    )
                    .map_err(|e| format!("Text prepare error after atlas trim: {e:?}"))?;
            } else {
                return Err(format!("Text prepare error: {err_msg}"));
            }
        }

        self.text_renderer_pool[slot_index].last_signature = batch_signature;
        if telemetry_enabled && call_sequence.is_multiple_of(20) {
            let skips = TEXT_RENDER_SKIPS.load(Ordering::Relaxed);
            let elapsed_ms = prepare_started
                .map(|start| start.elapsed().as_secs_f64() * 1000.0)
                .unwrap_or(0.0);
            let render_reshapes = TEXT_RENDER_ENSURE_RESHAPES.load(Ordering::Relaxed);
            let render_reuses = TEXT_RENDER_ENSURE_REUSES.load(Ordering::Relaxed);
            let render_cache_hits = TEXT_RENDER_CACHE_HITS.load(Ordering::Relaxed);
            let render_cache_misses = TEXT_RENDER_CACHE_MISSES.load(Ordering::Relaxed);
            let render_cache_total = render_cache_hits + render_cache_misses;
            log::warn!(
                "[text-render-telemetry] calls={} skips={} skip_rate={:.1}% batch_items={} chars={} prepare_ms={:.2} slot={} \
                 cache_hit_rate={:.1}% ensure_reshape_rate={:.1}% reshapes={} reuses={}",
                call_sequence,
                skips,
                (skips as f64 / call_sequence as f64) * 100.0,
                valid_items,
                total_chars,
                elapsed_ms,
                slot_index,
                if render_cache_total > 0 { (render_cache_hits as f64 / render_cache_total as f64) * 100.0 } else { 0.0 },
                if render_reshapes + render_reuses > 0 { (render_reshapes as f64 / (render_reshapes + render_reuses) as f64) * 100.0 } else { 0.0 },
                render_reshapes,
                render_reuses,
            );
        }
        Ok(slot_index)
    }

    fn draw_prepared_text(
        &mut self,
        render_pass: &mut wgpu::RenderPass<'_>,
        slot_index: usize,
    ) -> Result<(), String> {
        self.frame_stats.bump_text();
        self.text_renderer_pool[slot_index]
            .renderer
            .render(&self.text_atlas, &self.text_viewport, render_pass)
            .map_err(|e| format!("Text render error: {:?}", e))
    }

    /// Record a text render pass onto the provided encoder.
    /// `slot_index` must be the value returned by `prepare_text_for_render`.
    fn encode_text_pass(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        load_op: wgpu::LoadOp<wgpu::Color>,
        slot_index: usize,
    ) -> Result<(), String> {
        let mut text_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Text Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: load_op,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.draw_prepared_text(&mut text_pass, slot_index)
    }
}

fn align_to(value: u32, alignment: u32) -> u32 {
    debug_assert!(alignment > 0);
    value.div_ceil(alignment) * alignment
}

fn align_usize_to(value: usize, alignment: usize) -> usize {
    debug_assert!(alignment > 0);
    value.div_ceil(alignment) * alignment
}

impl GpuRenderer {
    fn convert_surface_pixels_to_rgba(&self, pixels: &mut [u8]) -> Result<(), String> {
        match self.surface_format {
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => Ok(()),
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
                for pixel in pixels.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                }
                Ok(())
            }
            format => Err(format!(
                "Screenshot readback unsupported for texture format: {format:?}"
            )),
        }
    }
}

fn is_in_effect_range(z_index: usize, effect_z_ranges: &[Range<usize>]) -> bool {
    effect_z_ranges.iter().any(|range| range.contains(&z_index))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SegmentDrawItem {
    Shape(usize),
    Image(usize),
    Text(usize),
    Shadow(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BatchKind {
    Shape,
    Image,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SegmentBatchPlan {
    Shape {
        start: usize,
        end: usize,
        blend_mode: BlendMode,
    },
    Image {
        start: usize,
        end: usize,
        blend_mode: BlendMode,
    },
    Text {
        start: usize,
        end: usize,
    },
}

impl SegmentBatchPlan {
    fn kind(self) -> BatchKind {
        match self {
            Self::Shape { .. } => BatchKind::Shape,
            Self::Image { .. } => BatchKind::Image,
            Self::Text { .. } => BatchKind::Text,
        }
    }

    fn needs_viewport_uniforms(self) -> bool {
        !matches!(self, Self::Text { .. })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SegmentDrawChunkPlan {
    batches: [Option<SegmentBatchPlan>; 3],
    len: usize,
}

impl SegmentDrawChunkPlan {
    fn is_empty(self) -> bool {
        self.len == 0
    }

    fn push(&mut self, batch: SegmentBatchPlan) {
        debug_assert!(self.len < self.batches.len());
        self.batches[self.len] = Some(batch);
        self.len += 1;
    }

    fn iter(self) -> impl Iterator<Item = SegmentBatchPlan> {
        self.batches.into_iter().flatten().take(self.len)
    }

    fn needs_viewport_uniforms(self) -> bool {
        self.iter().any(SegmentBatchPlan::needs_viewport_uniforms)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SegmentRenderCommand {
    DrawChunk(SegmentDrawChunkPlan),
    Shadow(usize),
}

struct SegmentCommandIter<'a> {
    ordered_items: &'a [(usize, SegmentDrawItem)],
    shapes: &'a [DrawShape],
    images: &'a [ImageDraw],
    cursor: usize,
}

impl<'a> SegmentCommandIter<'a> {
    fn new(
        ordered_items: &'a [(usize, SegmentDrawItem)],
        shapes: &'a [DrawShape],
        images: &'a [ImageDraw],
    ) -> Self {
        Self {
            ordered_items,
            shapes,
            images,
            cursor: 0,
        }
    }
}

impl Iterator for SegmentCommandIter<'_> {
    type Item = SegmentRenderCommand;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor >= self.ordered_items.len() {
            return None;
        }

        if let SegmentDrawItem::Shadow(index) = self.ordered_items[self.cursor].1 {
            self.cursor += 1;
            return Some(SegmentRenderCommand::Shadow(index));
        }

        let mut chunk = SegmentDrawChunkPlan::default();
        let mut buffer_usage = EncoderBufferUsage::default();

        while self.cursor < self.ordered_items.len() {
            if let SegmentDrawItem::Shadow(index) = self.ordered_items[self.cursor].1 {
                if chunk.is_empty() {
                    self.cursor += 1;
                    return Some(SegmentRenderCommand::Shadow(index));
                }
                break;
            }

            let (batch, next_cursor) = segment_batch_plan_at_cursor(
                self.ordered_items,
                self.shapes,
                self.images,
                self.cursor,
            );
            if buffer_usage.requires_flush_for_batch(batch.kind(), !chunk.is_empty()) {
                break;
            }

            chunk.push(batch);
            buffer_usage.mark_batch(batch.kind());
            self.cursor = next_cursor;
        }

        Some(SegmentRenderCommand::DrawChunk(chunk))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PreparedShapeBatch {
    index_count: u32,
}

enum PreparedSegmentBatch {
    Shape {
        blend_mode: BlendMode,
        batch: PreparedShapeBatch,
    },
    Image {
        blend_mode: BlendMode,
        image_cmds: Vec<ImageDrawCmd>,
    },
    Text {
        slot_index: usize,
    },
}

/// A pooled text renderer slot that independently caches its last batch signature.
///
/// The GPU renderer uses multiple `TextRendererSlot`s (one per text-batch position
/// within a frame) so that interleaved shape–text–shape–text rendering can skip
/// per-batch `glyphon::TextRenderer::prepare()` calls when the text draw list at
/// that position hasn't changed since the previous frame.
struct TextRendererSlot {
    renderer: TextRenderer,
    last_signature: Option<PreparedTextBatchSignature>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PreparedTextBatchSignature {
    hash: u64,
    width: u32,
    height: u32,
    root_scale_bits: u32,
}

fn hash_optional_rect<H: Hasher>(rect: Option<Rect>, state: &mut H) {
    match rect {
        Some(rect) => {
            1u8.hash(state);
            rect.render_hash().hash(state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_optional_snap_anchor<H: Hasher>(snap_anchor: Option<SnapAnchor>, state: &mut H) {
    match snap_anchor {
        Some(snap_anchor) => {
            1u8.hash(state);
            snap_anchor.origin.render_hash().hash(state);
            snap_anchor.device_pixel_step.to_bits().hash(state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_f32_bits<H: Hasher>(value: f32, state: &mut H) {
    value.to_bits().hash(state);
}

fn prepared_text_batch_signature<'a, I>(
    layer_texts: I,
    width: u32,
    height: u32,
    root_scale: f32,
) -> Option<PreparedTextBatchSignature>
where
    I: Clone + Iterator<Item = &'a TextDraw>,
{
    let mut hasher = FxHasher::default();
    let mut item_count = 0usize;

    for text_draw in layer_texts {
        if text_draw.text.is_empty() || text_draw.rect.width <= 0.0 || text_draw.rect.height <= 0.0
        {
            continue;
        }

        item_count += 1;
        text_draw.text.text.hash(&mut hasher);
        text_draw.text.text.len().hash(&mut hasher);

        let style_hash = crate::text_buffer_style_hash(&text_draw.text_style, &text_draw.text);
        style_hash.hash(&mut hasher);

        hash_f32_bits(text_draw.rect.x, &mut hasher);
        hash_f32_bits(text_draw.rect.y, &mut hasher);
        hash_f32_bits(text_draw.rect.width, &mut hasher);
        hash_f32_bits(text_draw.rect.height, &mut hasher);
        hash_optional_snap_anchor(text_draw.snap_anchor, &mut hasher);
        hash_optional_rect(text_draw.clip, &mut hasher);

        hash_f32_bits(text_draw.color.r(), &mut hasher);
        hash_f32_bits(text_draw.color.g(), &mut hasher);
        hash_f32_bits(text_draw.color.b(), &mut hasher);
        hash_f32_bits(text_draw.color.a(), &mut hasher);
        hash_f32_bits(text_draw.font_size, &mut hasher);
        hash_f32_bits(text_draw.scale, &mut hasher);
        text_draw.layout_options.hash(&mut hasher);
    }

    if item_count == 0 {
        return None;
    }

    width.hash(&mut hasher);
    height.hash(&mut hasher);
    hash_f32_bits(root_scale, &mut hasher);

    Some(PreparedTextBatchSignature {
        hash: hasher.finish(),
        width,
        height,
        root_scale_bits: root_scale.to_bits(),
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct EncoderBufferUsage {
    shape: bool,
    image: bool,
    text: bool,
}

impl EncoderBufferUsage {
    fn requires_flush_for_batch(self, kind: BatchKind, encoder_has_work: bool) -> bool {
        if !encoder_has_work {
            return false;
        }
        match kind {
            BatchKind::Shape => self.shape,
            BatchKind::Image => self.image,
            // Glyphon reuses shared GPU buffers across prepare() calls.
            // A second text batch in the same encoder would overwrite
            // the first batch's vertex data before submission.
            BatchKind::Text => self.text,
        }
    }

    fn mark_batch(&mut self, kind: BatchKind) {
        match kind {
            BatchKind::Shape => self.shape = true,
            BatchKind::Image => self.image = true,
            BatchKind::Text => self.text = true,
        }
    }

    #[cfg(test)]
    fn reset(&mut self) {
        *self = Self::default();
    }
}

fn segment_batch_plan_at_cursor(
    ordered_items: &[(usize, SegmentDrawItem)],
    shapes: &[DrawShape],
    images: &[ImageDraw],
    start: usize,
) -> (SegmentBatchPlan, usize) {
    match ordered_items[start].1 {
        SegmentDrawItem::Shape(index) => {
            let blend_mode = supported_blend_mode(shapes[index].blend_mode);
            let mut end = start + 1;
            while end < ordered_items.len() {
                match ordered_items[end].1 {
                    SegmentDrawItem::Shape(next_index)
                        if supported_blend_mode(shapes[next_index].blend_mode) == blend_mode =>
                    {
                        end += 1;
                    }
                    _ => break,
                }
            }
            (
                SegmentBatchPlan::Shape {
                    start,
                    end,
                    blend_mode,
                },
                end,
            )
        }
        SegmentDrawItem::Image(index) => {
            let blend_mode = supported_blend_mode(images[index].blend_mode);
            let mut end = start + 1;
            while end < ordered_items.len() {
                match ordered_items[end].1 {
                    SegmentDrawItem::Image(next_index)
                        if supported_blend_mode(images[next_index].blend_mode) == blend_mode =>
                    {
                        end += 1;
                    }
                    _ => break,
                }
            }
            (
                SegmentBatchPlan::Image {
                    start,
                    end,
                    blend_mode,
                },
                end,
            )
        }
        SegmentDrawItem::Text(_) => {
            let mut end = start + 1;
            while end < ordered_items.len() {
                if matches!(ordered_items[end].1, SegmentDrawItem::Text(_)) {
                    end += 1;
                } else {
                    break;
                }
            }
            (SegmentBatchPlan::Text { start, end }, end)
        }
        SegmentDrawItem::Shadow(_) => unreachable!("shadows are handled before batch planning"),
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_non_effect_segment_items(
    shapes: &[DrawShape],
    images: &[ImageDraw],
    texts: &[TextDraw],
    shadow_draws: &[ShadowDraw],
    z_start: usize,
    z_end: usize,
    effect_z_ranges: &[Range<usize>],
    scratch: &mut Vec<(usize, SegmentDrawItem)>,
) {
    scratch.clear();

    for (index, shape) in shapes.iter().enumerate() {
        if shape.z_index >= z_start
            && shape.z_index < z_end
            && !is_in_effect_range(shape.z_index, effect_z_ranges)
        {
            scratch.push((shape.z_index, SegmentDrawItem::Shape(index)));
        }
    }

    for (index, image) in images.iter().enumerate() {
        if image.z_index >= z_start
            && image.z_index < z_end
            && !is_in_effect_range(image.z_index, effect_z_ranges)
        {
            scratch.push((image.z_index, SegmentDrawItem::Image(index)));
        }
    }

    for (index, text) in texts.iter().enumerate() {
        if text.z_index >= z_start
            && text.z_index < z_end
            && !is_in_effect_range(text.z_index, effect_z_ranges)
        {
            scratch.push((text.z_index, SegmentDrawItem::Text(index)));
        }
    }

    for (index, shadow) in shadow_draws.iter().enumerate() {
        if shadow.z_index >= z_start
            && shadow.z_index < z_end
            && !is_in_effect_range(shadow.z_index, effect_z_ranges)
        {
            scratch.push((shadow.z_index, SegmentDrawItem::Shadow(index)));
        }
    }

    scratch.sort_by_key(|(z_index, _)| *z_index);
}

fn collect_effect_ranges(
    effect_layers: &[EffectLayer],
    z_start: usize,
    z_end: usize,
    excluded_effect_layer: Option<usize>,
    out: &mut Vec<Range<usize>>,
) {
    out.clear();
    for (index, layer) in effect_layers.iter().enumerate() {
        if Some(index) == excluded_effect_layer {
            continue;
        }
        if effect_layer_in_range(layer, z_start, z_end) {
            out.push(layer.z_start..layer.z_end);
        }
    }
    out.sort_by_key(|range| range.start);
}

fn collect_layer_events(
    effect_layers: &[EffectLayer],
    backdrop_layers: &[BackdropLayer],
    z_start: usize,
    z_end: usize,
    excluded_effect_layer: Option<usize>,
    out: &mut Vec<LayerEvent>,
) {
    out.clear();
    for (index, layer) in backdrop_layers.iter().enumerate() {
        if layer.z_index >= z_start && layer.z_index < z_end {
            out.push(LayerEvent {
                z_index: layer.z_index,
                kind: LayerEventKind::Backdrop(index),
            });
        }
    }
    for (index, layer) in effect_layers.iter().enumerate() {
        if Some(index) == excluded_effect_layer {
            continue;
        }
        if effect_layer_in_range(layer, z_start, z_end) {
            out.push(LayerEvent {
                z_index: layer.z_start,
                kind: LayerEventKind::Effect(index),
            });
        }
    }
    out.sort_by(|a, b| {
        // Primary key: z-index ascending.
        let z_cmp = a.z_index.cmp(&b.z_index);
        if z_cmp != std::cmp::Ordering::Equal {
            return z_cmp;
        }

        // Secondary key: backdrop before effect at same z-index.
        let kind_cmp = a.kind_order().cmp(&b.kind_order());
        if kind_cmp != std::cmp::Ordering::Equal {
            return kind_cmp;
        }

        // Tertiary key for same-z effects: outer-most (largest z_end) first.
        // If ranges are identical, prefer later insertion index (parents are
        // emitted after children during scene collection).
        match (a.kind, b.kind) {
            (LayerEventKind::Effect(ai), LayerEventKind::Effect(bi)) => effect_layers[bi]
                .z_end
                .cmp(&effect_layers[ai].z_end)
                .then_with(|| bi.cmp(&ai)),
            _ => std::cmp::Ordering::Equal,
        }
    });
}

pub(crate) fn has_backdrop_layer_in_range(
    backdrop_layers: &[BackdropLayer],
    z_start: usize,
    z_end: usize,
) -> bool {
    backdrop_layers
        .iter()
        .any(|layer| layer.z_index >= z_start && layer.z_index < z_end)
}

pub(crate) fn scissor_rect_for_rect(
    rect: Rect,
    root_scale: f32,
    width: u32,
    height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let mut left = rect.x * root_scale;
    let mut top = rect.y * root_scale;
    let mut right = (rect.x + rect.width) * root_scale;
    let mut bottom = (rect.y + rect.height) * root_scale;

    left = left.max(0.0).min(width as f32).floor();
    top = top.max(0.0).min(height as f32).floor();
    right = right.max(0.0).min(width as f32).ceil();
    bottom = bottom.max(0.0).min(height as f32).ceil();

    if right <= left || bottom <= top {
        return None;
    }

    Some((
        left as u32,
        top as u32,
        (right - left) as u32,
        (bottom - top) as u32,
    ))
}

fn scissor_rect_for_layer(
    rect: Rect,
    clip: Option<Rect>,
    root_scale: f32,
    width: u32,
    height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let clipped_rect = match clip {
        Some(clip_rect) => rect.intersect(clip_rect)?,
        None => rect,
    };

    scissor_rect_for_rect(clipped_rect, root_scale, width, height)
}

fn text_bounds_for_clip(
    clip: Option<Rect>,
    root_scale: f32,
    width: u32,
    height: u32,
) -> Option<TextBounds> {
    let (left, top, right, bottom) = match clip {
        Some(clip_rect) => (
            (clip_rect.x * root_scale).floor().max(0.0),
            (clip_rect.y * root_scale).floor().max(0.0),
            ((clip_rect.x + clip_rect.width) * root_scale)
                .ceil()
                .min(width as f32),
            ((clip_rect.y + clip_rect.height) * root_scale)
                .ceil()
                .min(height as f32),
        ),
        None => (0.0, 0.0, width as f32, height as f32),
    };

    if !(left.is_finite() && top.is_finite() && right.is_finite() && bottom.is_finite()) {
        return None;
    }
    if right <= left || bottom <= top {
        return None;
    }

    Some(TextBounds {
        left: left as i32,
        top: top as i32,
        right: right as i32,
        bottom: bottom as i32,
    })
}

fn tint_for_image(
    color_filter: Option<ColorFilter>,
    alpha: f32,
) -> ([f32; 4], Option<ColorFilter>) {
    let alpha = alpha.clamp(0.0, 1.0);
    match color_filter {
        Some(filter) if filter.supports_gpu_vertex_modulation() => {
            let Some(tint) = filter.gpu_vertex_tint() else {
                return ([1.0, 1.0, 1.0, alpha], Some(filter));
            };
            (
                [
                    tint[0].clamp(0.0, 1.0),
                    tint[1].clamp(0.0, 1.0),
                    tint[2].clamp(0.0, 1.0),
                    (tint[3] * alpha).clamp(0.0, 1.0),
                ],
                None,
            )
        }
        Some(filter) => ([1.0, 1.0, 1.0, alpha], Some(filter)),
        None => ([1.0, 1.0, 1.0, alpha], None),
    }
}

fn apply_filter_to_bitmap(image: &ImageBitmap, filter: ColorFilter) -> Result<ImageBitmap, String> {
    let mut filtered = Vec::with_capacity(image.pixels().len());
    for pixel in image.pixels().chunks_exact(4) {
        let rgba = [
            pixel[0] as f32 / 255.0,
            pixel[1] as f32 / 255.0,
            pixel[2] as f32 / 255.0,
            pixel[3] as f32 / 255.0,
        ];
        let out = filter.apply_rgba(rgba);
        filtered.push((out[0].clamp(0.0, 1.0) * 255.0).round() as u8);
        filtered.push((out[1].clamp(0.0, 1.0) * 255.0).round() as u8);
        filtered.push((out[2].clamp(0.0, 1.0) * 255.0).round() as u8);
        filtered.push((out[3].clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    ImageBitmap::from_rgba8(image.width(), image.height(), filtered)
        .map_err(|error| format!("failed to build filtered bitmap: {error}"))
}

fn scissor_rect_for_image(
    image: &ImageDraw,
    root_scale: f32,
    width: u32,
    height: u32,
) -> Option<(u32, u32, u32, u32)> {
    scissor_rect_for_layer(image.rect, image.clip, root_scale, width, height)
}

fn inner_shadow_composite_mask(
    shadow: &ShadowDraw,
    root_scale: f32,
) -> Option<RoundedCompositeMask> {
    if !shadow
        .shapes
        .iter()
        .any(|(_, mode)| *mode == BlendMode::DstOut)
    {
        return None;
    }
    let (fill, _) = shadow.shapes.first()?;
    let rect = fill.local_rect;
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }

    let radii = fill.shape.map_or([0.0; 4], |rounded| {
        let resolved = rounded.resolve(rect.width, rect.height);
        [
            resolved.top_left * root_scale,
            resolved.top_right * root_scale,
            resolved.bottom_left * root_scale,
            resolved.bottom_right * root_scale,
        ]
    });

    Some(RoundedCompositeMask {
        rect: [
            rect.x * root_scale,
            rect.y * root_scale,
            rect.width * root_scale,
            rect.height * root_scale,
        ],
        radii,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_render_common::graph::{DrawPrimitiveNode, IsolationReasons, TextPrimitiveNode};
    use cranpose_render_common::raster_cache::LayerRasterCacheHashes;
    use cranpose_ui::text::{
        AnnotatedString, BaselineShift, RangeStyle, Shadow, SpanStyle, TextDecoration,
        TextDrawStyle, TextGeometricTransform, TextUnit,
    };
    use cranpose_ui::{TextLayoutOptions, TextStyle};
    use cranpose_ui_graphics::{
        Brush, Color, CornerRadii, DrawPrimitive, Rect, RenderEffect, RoundedCornerShape,
    };

    fn chunk(batches: &[SegmentBatchPlan]) -> SegmentDrawChunkPlan {
        let mut chunk = SegmentDrawChunkPlan::default();
        for batch in batches {
            chunk.push(*batch);
        }
        chunk
    }

    fn effect_layer(z_start: usize, z_end: usize) -> EffectLayer {
        EffectLayer {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            clip: None,
            effect: Some(RenderEffect::blur(4.0)),
            blend_mode: BlendMode::SrcOver,
            composite_alpha: 1.0,
            z_start,
            z_end,
            requirements: SurfaceRequirementSet::default().with(SurfaceRequirement::RenderEffect),
        }
    }

    #[test]
    fn device_pixel_bounds_for_rect_snaps_origin_and_extents() {
        let bounds = device_pixel_bounds_for_rect(
            Rect {
                x: 10.25,
                y: 14.6,
                width: 20.1,
                height: 9.2,
            },
            200,
            120,
            2.0,
        )
        .expect("rect should intersect the viewport");

        assert_eq!(
            bounds,
            DevicePixelBounds {
                x: 20.0,
                y: 29.0,
                width: 41,
                height: 19,
            }
        );
    }

    #[test]
    fn visible_layer_rect_intersects_clip_and_viewport() {
        let visible = visible_layer_rect(
            Rect {
                x: -10.0,
                y: 5.0,
                width: 80.0,
                height: 40.0,
            },
            Some(Rect {
                x: 4.0,
                y: 8.0,
                width: 20.0,
                height: 50.0,
            }),
            2.0,
            60,
            40,
        )
        .expect("visible rect");

        assert_eq!(
            visible,
            Rect {
                x: 4.0,
                y: 8.0,
                width: 20.0,
                height: 12.0,
            }
        );
    }

    #[test]
    fn clamp_effect_surface_scale_caps_large_surfaces_but_keeps_base_scale() {
        let clamped = clamp_effect_surface_scale(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 1200.0,
                height: 900.0,
            },
            1.0,
            8.0,
            16_384,
        );

        assert!(
            clamped < 8.0,
            "large translated effect layers must be capped to avoid OOM, got {clamped}"
        );
        assert!(
            clamped >= 1.0,
            "effect surfaces must not fall below destination resolution, got {clamped}"
        );
    }

    fn backdrop_layer(z_index: usize) -> BackdropLayer {
        BackdropLayer {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            clip: None,
            effect: RenderEffect::blur(2.0),
            z_index,
        }
    }

    fn test_shape(z_index: usize, blend_mode: BlendMode) -> DrawShape {
        DrawShape {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            },
            local_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            },
            quad: [[0.0, 0.0], [8.0, 0.0], [0.0, 8.0], [8.0, 8.0]],
            snap_anchor: None,
            brush: Brush::solid(Color::BLACK),
            shape: None,
            z_index,
            clip: None,
            blend_mode,
        }
    }

    fn test_shadow_draw(shapes: Vec<(DrawShape, BlendMode)>) -> ShadowDraw {
        ShadowDraw {
            shapes,
            texts: vec![],
            blur_radius: 8.0,
            clip: None,
            z_index: 0,
        }
    }

    fn test_image(z_index: usize, blend_mode: BlendMode) -> ImageDraw {
        ImageDraw {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            },
            local_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            },
            quad: [[0.0, 0.0], [8.0, 0.0], [0.0, 8.0], [8.0, 8.0]],
            snap_anchor: None,
            image: ImageBitmap::from_rgba8(1, 1, vec![255, 255, 255, 255]).expect("image"),
            alpha: 1.0,
            color_filter: None,
            z_index,
            clip: None,
            blend_mode,
            src_rect: None,
            motion_context_animated: false,
        }
    }

    #[test]
    fn image_sample_mode_keeps_static_images_nearest() {
        let image = test_image(0, BlendMode::SrcOver);
        assert_eq!(image_sample_mode(&image), ImageSampleMode::Nearest);
    }

    #[test]
    fn image_sample_mode_switches_scrolling_images_to_linear() {
        let mut image = test_image(0, BlendMode::SrcOver);
        image.motion_context_animated = true;
        assert_eq!(image_sample_mode(&image), ImageSampleMode::Linear);
    }

    fn test_text(z_index: usize) -> TextDraw {
        TextDraw {
            node_id: 0,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            },
            snap_anchor: None,
            translated_content_context: false,
            text: std::rc::Rc::new(cranpose_ui::text::AnnotatedString::from("t")),
            color: Color::WHITE,
            text_style: cranpose_ui::TextStyle::default(),
            font_size: 12.0,
            scale: 1.0,
            layout_options: cranpose_ui::TextLayoutOptions::default(),
            z_index,
            clip: None,
        }
    }

    fn test_layer(local_bounds: Rect, children: Vec<RenderNode>) -> LayerNode {
        crate::test_support::layer_node(
            local_bounds,
            ProjectiveTransform::identity(),
            GraphicsLayer::default(),
            children,
        )
    }

    fn cacheable_layer(
        node_id: cranpose_core::NodeId,
        local_bounds: Rect,
        children: Vec<RenderNode>,
    ) -> LayerNode {
        let mut layer = test_layer(local_bounds, children);
        layer.node_id = Some(node_id);
        layer.cache_policy = cranpose_render_common::graph::CachePolicy::Auto;
        layer.recompute_raster_cache_hashes();
        layer
    }

    fn text_layer_with_style(text: AnnotatedString, text_style: TextStyle) -> LayerNode {
        test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 32.0,
            },
            vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                    node_id: 1,
                    rect: Rect {
                        x: 2.0,
                        y: 3.0,
                        width: 48.0,
                        height: 18.0,
                    },
                    text,
                    text_style,
                    font_size: 14.0,
                    layout_options: TextLayoutOptions::default(),
                    clip: None,
                })),
            })],
        )
    }

    fn snapped_text_leaf(animated: bool, translated_content_context: bool) -> LayerNode {
        LayerNode {
            node_id: Some(77),
            local_bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 48.0,
                height: 24.0,
            },
            transform_to_parent: ProjectiveTransform::translation(14.25, 16.5),
            motion_context_animated: animated,
            translated_content_context,
            graphics_layer: GraphicsLayer::default(),
            clip_to_bounds: false,
            shadow_clip: None,
            hit_test: None,
            has_hit_targets: false,
            isolation: IsolationReasons::default(),
            cache_policy: CachePolicy::None,
            cache_hashes: LayerRasterCacheHashes::default(),
            cache_hashes_valid: false,
            children: vec![
                RenderNode::Primitive(PrimitiveEntry {
                    phase: PrimitivePhase::BeforeChildren,
                    node: PrimitiveNode::Draw(DrawPrimitiveNode {
                        primitive: DrawPrimitive::RoundRect {
                            rect: Rect {
                                x: 0.0,
                                y: 0.0,
                                width: 48.0,
                                height: 24.0,
                            },
                            brush: Brush::solid(Color(0.28, 0.30, 0.46, 0.88)),
                            radii: CornerRadii::uniform(6.0),
                        },
                        clip: None,
                    }),
                }),
                RenderNode::Primitive(PrimitiveEntry {
                    phase: PrimitivePhase::BeforeChildren,
                    node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                        node_id: 77,
                        rect: Rect {
                            x: 6.0,
                            y: 4.0,
                            width: 36.0,
                            height: 16.0,
                        },
                        text: AnnotatedString::from("48 px"),
                        text_style: TextStyle::default(),
                        font_size: 14.0,
                        layout_options: TextLayoutOptions::default(),
                        clip: None,
                    })),
                }),
            ],
        }
    }

    fn snapped_text_leaf_root(animated: bool, translated_content_context: bool) -> LayerNode {
        let text_leaf = snapped_text_leaf(animated, translated_content_context);
        test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 96.0,
                height: 64.0,
            },
            vec![RenderNode::Layer(Box::new(text_leaf))],
        )
    }

    fn translated_content_local_surface_root() -> LayerNode {
        let mut effectful_text = text_layer_with_style(
            AnnotatedString::from("shadow"),
            TextStyle::from_span_style(SpanStyle {
                shadow: Some(Shadow {
                    color: Color::BLACK,
                    offset: Point::new(1.0, 2.0),
                    blur_radius: 3.0,
                }),
                ..SpanStyle::default()
            }),
        );
        effectful_text.translated_content_context = true;

        let translated_content = LayerNode {
            node_id: Some(78),
            local_bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 96.0,
                height: 64.0,
            },
            transform_to_parent: ProjectiveTransform::translation(14.25, 16.5),
            motion_context_animated: false,
            translated_content_context: true,
            graphics_layer: GraphicsLayer::default(),
            clip_to_bounds: false,
            shadow_clip: None,
            hit_test: None,
            has_hit_targets: false,
            isolation: IsolationReasons::default(),
            cache_policy: CachePolicy::None,
            cache_hashes: LayerRasterCacheHashes::default(),
            cache_hashes_valid: false,
            children: vec![RenderNode::Layer(Box::new(effectful_text))],
        };

        test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 160.0,
                height: 120.0,
            },
            vec![RenderNode::Layer(Box::new(translated_content))],
        )
    }

    #[test]
    fn prepared_text_batch_signature_is_stable_for_equivalent_inputs() {
        let text = test_text(0);
        let cloned = text.clone();
        let sig_a = prepared_text_batch_signature([&text].into_iter(), 1920, 1080, 1.0);
        let sig_b = prepared_text_batch_signature([&cloned].into_iter(), 1920, 1080, 1.0);
        assert_eq!(sig_a, sig_b);
    }

    #[test]
    fn prepared_text_batch_signature_changes_when_geometry_changes() {
        let text = test_text(0);
        let mut moved = text.clone();
        moved.rect.y = 42.0;

        let original = prepared_text_batch_signature([&text].into_iter(), 1920, 1080, 1.0);
        let changed = prepared_text_batch_signature([&moved].into_iter(), 1920, 1080, 1.0);
        assert_ne!(original, changed);
    }

    #[test]
    fn scissor_rect_for_layer_intersects_with_clip() {
        let rect = Rect {
            x: 10.0,
            y: 10.0,
            width: 30.0,
            height: 20.0,
        };
        let clip = Rect {
            x: 20.0,
            y: 15.0,
            width: 100.0,
            height: 100.0,
        };

        let scissor = scissor_rect_for_layer(rect, Some(clip), 1.0, 200, 200);
        assert_eq!(scissor, Some((20, 15, 20, 15)));
    }

    #[test]
    fn text_bounds_for_clip_rounds_outward_and_clamps() {
        let clip = Rect {
            x: 10.2,
            y: 5.4,
            width: 20.1,
            height: 9.3,
        };
        let bounds = text_bounds_for_clip(Some(clip), 1.0, 200, 120).expect("bounds");
        assert_eq!(bounds.left, 10);
        assert_eq!(bounds.top, 5);
        assert_eq!(bounds.right, 31);
        assert_eq!(bounds.bottom, 15);
    }

    #[test]
    fn text_bounds_for_clip_returns_none_when_intersection_is_empty() {
        let clip = Rect {
            x: 220.0,
            y: 10.0,
            width: 40.0,
            height: 20.0,
        };
        assert!(text_bounds_for_clip(Some(clip), 1.0, 200, 120).is_none());
    }

    #[test]
    fn text_bounds_for_clip_scales_to_physical_pixels() {
        let clip = Rect {
            x: 1.25,
            y: 2.5,
            width: 6.0,
            height: 4.0,
        };
        let bounds = text_bounds_for_clip(Some(clip), 2.0, 200, 120).expect("bounds");
        assert_eq!(bounds.left, 2);
        assert_eq!(bounds.top, 5);
        assert_eq!(bounds.right, 15);
        assert_eq!(bounds.bottom, 13);
    }

    #[test]
    fn text_bounds_for_clip_clamps_to_render_target() {
        // Clip extends beyond the render target — bounds must not exceed target dimensions.
        let clip = Rect {
            x: 0.0,
            y: 0.0,
            width: 900.0,
            height: 600.0,
        };
        let bounds = text_bounds_for_clip(Some(clip), 2.0, 1229, 815).expect("bounds");
        assert_eq!(bounds.left, 0);
        assert_eq!(bounds.top, 0);
        // Without clamping, right would be ceil(900*2)=1800 which exceeds width 1229.
        assert!(bounds.right <= 1229);
        assert!(bounds.bottom <= 815);
    }

    #[test]
    fn text_bounds_for_clip_none_uses_render_target_size() {
        let bounds = text_bounds_for_clip(None, 2.0, 1229, 815).expect("bounds");
        assert_eq!(bounds.left, 0);
        assert_eq!(bounds.top, 0);
        // No-clip case: bounds must equal exactly the render target dimensions.
        assert_eq!(bounds.right, 1229);
        assert_eq!(bounds.bottom, 815);
    }

    #[test]
    fn visible_draw_rect_no_clip_returns_original() {
        let rect = Rect {
            x: 100.0,
            y: 200.0,
            width: 300.0,
            height: 400.0,
        };
        assert_eq!(visible_draw_rect(rect, None), Some(rect));
    }

    #[test]
    fn visible_draw_rect_with_clip_intersects() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 2000.0,
            height: 5000.0,
        };
        let clip = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let visible = visible_draw_rect(rect, Some(clip)).expect("should have visible area");
        assert_eq!(visible.width, 800.0);
        assert_eq!(visible.height, 600.0);
    }

    #[test]
    fn visible_draw_rect_fully_clipped_returns_none() {
        let rect = Rect {
            x: 1000.0,
            y: 1000.0,
            width: 200.0,
            height: 200.0,
        };
        let clip = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        assert!(visible_draw_rect(rect, Some(clip)).is_none());
    }

    #[test]
    fn scene_bounds_respects_clip_on_shapes() {
        let mut scene = CompositorScene::new();
        // Shape inside viewport — visible
        scene.shapes.push(DrawShape {
            rect: Rect {
                x: 10.0,
                y: 10.0,
                width: 100.0,
                height: 50.0,
            },
            clip: Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            }),
            ..test_shape(0, BlendMode::SrcOver)
        });
        // Shape far outside viewport — clipped away entirely
        scene.shapes.push(DrawShape {
            rect: Rect {
                x: 0.0,
                y: 3000.0,
                width: 100.0,
                height: 50.0,
            },
            clip: Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            }),
            ..test_shape(1, BlendMode::SrcOver)
        });
        let bounds = scene_bounds(&scene).expect("should have bounds");
        // Bounds should only cover the first shape's visible area,
        // NOT extend to y=3050 from the clipped second shape.
        assert!(bounds.y + bounds.height <= 600.0);
    }

    #[test]
    fn scene_bounds_scroll_content_clipped_to_viewport() {
        // Simulates a scroll container: many items with large y offsets,
        // all clipped to a viewport-sized clip rect.
        let mut scene = CompositorScene::new();
        let viewport_clip = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        for i in 0..20 {
            scene.shapes.push(DrawShape {
                rect: Rect {
                    x: 0.0,
                    y: i as f32 * 300.0,
                    width: 800.0,
                    height: 200.0,
                },
                clip: Some(viewport_clip),
                ..test_shape(i, BlendMode::SrcOver)
            });
        }
        let bounds = scene_bounds(&scene).expect("should have bounds");
        // All shapes are clipped to viewport — bounds should be viewport-sized,
        // NOT 20*300 = 6000 dp tall.
        assert_eq!(bounds.x, 0.0);
        assert_eq!(bounds.y, 0.0);
        assert!(bounds.width <= 800.0);
        assert!(bounds.height <= 600.0);
    }

    #[test]
    fn scene_bounds_stable_across_scroll_offsets() {
        // Simulates horizontal scroll at different offsets —
        // bounds should be identical regardless of scroll position.
        let viewport_clip = Rect {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 50.0,
        };
        let compute_bounds_at_offset = |scroll_x: f32| {
            let mut scene = CompositorScene::new();
            for i in 0..10 {
                scene.shapes.push(DrawShape {
                    rect: Rect {
                        x: i as f32 * 100.0 - scroll_x,
                        y: 0.0,
                        width: 80.0,
                        height: 40.0,
                    },
                    clip: Some(viewport_clip),
                    ..test_shape(i, BlendMode::SrcOver)
                });
            }
            scene_bounds(&scene).expect("bounds")
        };
        let bounds_at_0 = compute_bounds_at_offset(0.0);
        let bounds_at_300 = compute_bounds_at_offset(300.0);
        let bounds_at_600 = compute_bounds_at_offset(600.0);
        // Width should be stable (clipped to viewport) regardless of scroll offset
        assert!(
            (bounds_at_0.width - bounds_at_300.width).abs() < 1.0,
            "bounds width changed with scroll: {} vs {}",
            bounds_at_0.width,
            bounds_at_300.width
        );
        assert!(
            (bounds_at_0.width - bounds_at_600.width).abs() < 1.0,
            "bounds width changed with scroll: {} vs {}",
            bounds_at_0.width,
            bounds_at_600.width
        );
    }

    #[test]
    fn collect_effect_ranges_respects_excluded_effect() {
        let layers = vec![effect_layer(10, 40), effect_layer(20, 30)];
        let mut ranges = Vec::new();
        collect_effect_ranges(&layers, 10, 40, Some(0), &mut ranges);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], 20..30);
    }

    #[test]
    fn collect_layer_events_includes_nested_when_parent_excluded() {
        let effects = vec![effect_layer(10, 40), effect_layer(20, 30)];
        let backdrops = vec![backdrop_layer(25)];
        let mut events = Vec::new();
        collect_layer_events(&effects, &backdrops, 10, 40, Some(0), &mut events);
        assert_eq!(events.len(), 2);

        match events[0].kind {
            LayerEventKind::Effect(index) => assert_eq!(index, 1),
            LayerEventKind::Backdrop(_) => panic!("expected nested effect as first event"),
        }
        match events[1].kind {
            LayerEventKind::Backdrop(index) => assert_eq!(index, 0),
            LayerEventKind::Effect(_) => panic!("expected backdrop as second event"),
        }
    }

    fn pure_text_leaf(animated: bool, translated_content_context: bool) -> LayerNode {
        LayerNode {
            node_id: Some(177),
            local_bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 96.0,
                height: 32.0,
            },
            transform_to_parent: ProjectiveTransform::translation(11.4, 23.6),
            motion_context_animated: animated,
            translated_content_context,
            graphics_layer: GraphicsLayer::default(),
            clip_to_bounds: false,
            shadow_clip: None,
            hit_test: None,
            has_hit_targets: false,
            isolation: IsolationReasons::default(),
            cache_policy: CachePolicy::None,
            cache_hashes: LayerRasterCacheHashes::default(),
            cache_hashes_valid: false,
            children: vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                    node_id: 177,
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 96.0,
                        height: 24.0,
                    },
                    clip: None,
                    text: AnnotatedString::from("Pure text"),
                    text_style: TextStyle::default(),
                    font_size: 14.0,
                    layout_options: TextLayoutOptions::default(),
                })),
            })],
        }
    }

    fn pure_text_leaf_root(animated: bool, translated_content_context: bool) -> LayerNode {
        let text_leaf = pure_text_leaf(animated, translated_content_context);
        test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 160.0,
                height: 96.0,
            },
            vec![RenderNode::Layer(Box::new(text_leaf))],
        )
    }

    #[test]
    fn collect_layer_events_sorts_backdrop_before_effect_at_same_z() {
        let effects = vec![effect_layer(10, 20)];
        let backdrops = vec![backdrop_layer(10)];
        let mut events = Vec::new();
        collect_layer_events(&effects, &backdrops, 0, 30, None, &mut events);
        assert_eq!(events.len(), 2);

        match events[0].kind {
            LayerEventKind::Backdrop(_) => {}
            LayerEventKind::Effect(_) => panic!("expected backdrop to run before effect"),
        }
        match events[1].kind {
            LayerEventKind::Effect(_) => {}
            LayerEventKind::Backdrop(_) => panic!("expected effect as second event"),
        }
    }

    #[test]
    fn collect_layer_events_prefers_outer_effect_when_same_start_z() {
        // Child emitted before parent (matching scene collection order where a
        // parent effect is recorded after recursively processing children).
        let effects = vec![effect_layer(10, 20), effect_layer(10, 40)];
        let mut events = Vec::new();
        collect_layer_events(&effects, &[], 0, 50, None, &mut events);

        assert_eq!(events.len(), 2);
        match events[0].kind {
            LayerEventKind::Effect(index) => assert_eq!(index, 1),
            LayerEventKind::Backdrop(_) => panic!("expected outer effect first"),
        }
        match events[1].kind {
            LayerEventKind::Effect(index) => assert_eq!(index, 0),
            LayerEventKind::Backdrop(_) => panic!("expected child effect second"),
        }
    }

    #[test]
    fn collect_layer_events_prefers_later_effect_when_ranges_match() {
        let effects = vec![effect_layer(10, 20), effect_layer(10, 20)];
        let mut events = Vec::new();
        collect_layer_events(&effects, &[], 0, 30, None, &mut events);

        assert_eq!(events.len(), 2);
        match events[0].kind {
            LayerEventKind::Effect(index) => assert_eq!(index, 1),
            LayerEventKind::Backdrop(_) => panic!("expected later effect first"),
        }
        match events[1].kind {
            LayerEventKind::Effect(index) => assert_eq!(index, 0),
            LayerEventKind::Backdrop(_) => panic!("expected earlier effect second"),
        }
    }

    #[test]
    fn has_backdrop_layer_in_range_detects_nested_layers() {
        let backdrops = vec![backdrop_layer(5), backdrop_layer(15), backdrop_layer(25)];
        assert!(has_backdrop_layer_in_range(&backdrops, 10, 20));
        assert!(has_backdrop_layer_in_range(&backdrops, 0, 6));
        assert!(!has_backdrop_layer_in_range(&backdrops, 20, 25));
    }

    #[test]
    fn layer_contains_descendant_backdrop_ignores_self_backdrop() {
        let mut self_backdrop = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            vec![],
        );
        self_backdrop.graphics_layer.backdrop_effect = Some(RenderEffect::blur(2.0));
        assert!(!layer_contains_descendant_backdrop(&self_backdrop));

        let mut child = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            },
            vec![],
        );
        child.graphics_layer.backdrop_effect = Some(RenderEffect::blur(2.0));

        let parent = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            vec![RenderNode::Layer(Box::new(child))],
        );
        assert!(layer_contains_descendant_backdrop(&parent));
    }

    #[test]
    fn estimate_layer_surface_rect_includes_transformed_child_bounds() {
        let mut child = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 6.0,
            },
            vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                        rect: Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 10.0,
                            height: 6.0,
                        },
                        brush: Brush::solid(Color::WHITE),
                    },
                    clip: None,
                }),
            })],
        );
        child.transform_to_parent = ProjectiveTransform::translation(18.0, 7.0);

        let parent = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 4.0,
                height: 4.0,
            },
            vec![RenderNode::Layer(Box::new(child))],
        );

        assert_eq!(
            estimate_layer_surface_rect(&parent),
            Rect {
                x: 18.0,
                y: 7.0,
                width: 10.0,
                height: 6.0,
            }
        );
    }

    #[test]
    fn estimate_layer_surface_rect_expands_for_child_layer_shadow() {
        let mut child = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 12.0,
                height: 8.0,
            },
            vec![],
        );
        child.transform_to_parent = ProjectiveTransform::translation(20.0, 9.0);
        child.graphics_layer.shadow_elevation = 6.0;

        let parent = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 4.0,
                height: 4.0,
            },
            vec![RenderNode::Layer(Box::new(child))],
        );

        let rect = estimate_layer_surface_rect(&parent);
        assert!(rect.x < 20.0);
        assert!(rect.y < 9.0);
        assert!(rect.width > 12.0);
        assert!(rect.height > 8.0);
    }

    #[test]
    fn estimate_layer_surface_rect_respects_local_bounds_for_effect_layers() {
        let mut layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 28.0,
                height: 28.0,
            },
            vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                        rect: Rect {
                            x: 10.0,
                            y: 10.0,
                            width: 10.0,
                            height: 10.0,
                        },
                        brush: Brush::solid(Color::WHITE),
                    },
                    clip: None,
                }),
            })],
        );
        layer.graphics_layer.render_effect = Some(RenderEffect::blur(12.0));

        assert_eq!(
            estimate_layer_surface_rect(&layer),
            Rect {
                x: 0.0,
                y: 0.0,
                width: 28.0,
                height: 28.0,
            }
        );
    }

    #[test]
    fn layer_raster_cache_candidate_ignores_parent_transform() {
        let primitive = PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: cranpose_ui_graphics::DrawPrimitive::Rect {
                    rect: Rect {
                        x: 2.0,
                        y: 3.0,
                        width: 6.0,
                        height: 4.0,
                    },
                    brush: Brush::solid(Color::BLACK),
                },
                clip: None,
            }),
        };
        let base = cacheable_layer(
            41,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            vec![RenderNode::Primitive(primitive.clone())],
        );
        let mut moved = base.clone();
        moved.transform_to_parent = ProjectiveTransform::translation(32.0, 18.0);

        assert_eq!(
            layer_raster_cache_candidate(&base, 1.25, false, false),
            layer_raster_cache_candidate(&moved, 1.25, false, false)
        );
    }

    #[test]
    fn layer_raster_cache_candidate_changes_for_child_transform() {
        let mut child = cacheable_layer(
            8,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 12.0,
                height: 10.0,
            },
            vec![],
        );
        child.transform_to_parent = ProjectiveTransform::translation(4.0, 6.0);
        let base = cacheable_layer(
            7,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            vec![RenderNode::Layer(Box::new(child.clone()))],
        );
        let mut moved_child = child;
        moved_child.transform_to_parent = ProjectiveTransform::translation(9.0, 6.0);
        let moved = cacheable_layer(
            7,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            vec![RenderNode::Layer(Box::new(moved_child))],
        );

        assert_ne!(
            layer_raster_cache_candidate(&base, 1.0, false, false),
            layer_raster_cache_candidate(&moved, 1.0, false, false)
        );
    }

    #[test]
    fn layer_raster_cache_candidate_rejects_external_backdrop_dependency() {
        let mut child = cacheable_layer(
            12,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            },
            vec![],
        );
        child.graphics_layer.backdrop_effect = Some(RenderEffect::blur(2.0));
        let parent = cacheable_layer(
            11,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 16.0,
                height: 16.0,
            },
            vec![RenderNode::Layer(Box::new(child))],
        );

        assert!(layer_raster_cache_candidate(&parent, 1.0, false, false).is_some());
        assert!(layer_raster_cache_candidate(&parent, 1.0, true, false).is_none());
    }

    #[test]
    fn layer_raster_cache_candidate_does_not_force_translation_only_text_surfaces() {
        let text = RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                node_id: 77,
                rect: Rect {
                    x: 2.0,
                    y: 3.0,
                    width: 48.0,
                    height: 18.0,
                },
                text: AnnotatedString::from("runtime cache"),
                text_style: TextStyle::default(),
                font_size: 14.0,
                layout_options: TextLayoutOptions::default(),
                clip: None,
            })),
        });
        let mut layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 32.0,
            },
            vec![text],
        );
        layer.node_id = Some(77);
        layer.recompute_raster_cache_hashes();

        assert!(
            layer_raster_cache_candidate(&layer, 1.0, false, false).is_none(),
            "root path should not isolate plain translation-only text layers"
        );
        assert!(
            layer_raster_cache_candidate(&layer, 1.0, false, true).is_none(),
            "child path should also render plain translation-only text layers directly"
        );
    }

    #[test]
    fn layer_surface_requirements_keep_plain_text_on_direct_path() {
        let layer = text_layer_with_style(AnnotatedString::from("plain"), TextStyle::default());

        let requirements = layer_surface_requirements(&layer);

        assert_eq!(requirements.direct_translation, Some(Point::default()));
        assert!(!requirements.surface_requirements.has_any());
    }

    #[test]
    fn layer_surface_requirements_keep_translated_plain_text_leaf_on_direct_path() {
        let layer = pure_text_leaf(false, true);

        let requirements = layer_surface_requirements(&layer);

        assert_eq!(
            requirements.direct_translation,
            Some(Point::new(11.4, 23.6))
        );
        assert!(
            !requirements.surface_requirements.has_any(),
            "translated plain text should stay on the direct path and isolate only the glyph draw"
        );
    }

    #[test]
    fn layer_surface_requirements_keep_translated_text_leaf_with_background_on_direct_path() {
        let layer = snapped_text_leaf(false, true);

        let requirements = layer_surface_requirements(&layer);

        assert_eq!(
            requirements.direct_translation,
            Some(Point::new(14.25, 16.5))
        );
        assert!(
            !requirements.surface_requirements.has_any(),
            "translated text with direct sibling decoration/background should keep the layer direct"
        );
    }

    #[test]
    fn translated_text_effect_layer_uses_box4_composite_resolve() {
        let root = pure_text_leaf_root(false, true);
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();
        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.child_layers.len(), 0);
        assert_eq!(collected.scene.texts.len(), 1);
        assert_eq!(collected.scene.effect_layers.len(), 1);
        assert!(collected.scene.effect_layers[0]
            .requirements
            .contains(SurfaceRequirement::MotionStableCapture));
        assert_eq!(
            collected.scene.effect_layers[0].effect, None,
            "translated plain text should isolate only the glyph draw, not apply a post-effect"
        );
    }

    #[test]
    fn non_translated_text_local_surface_keeps_linear_composite_resolve() {
        let layer = text_layer_with_style(
            AnnotatedString::from("gradient"),
            TextStyle::from_span_style(SpanStyle {
                brush: Some(Brush::linear_gradient(vec![Color::WHITE, Color::BLACK])),
                ..SpanStyle::default()
            }),
        );
        let requirements = layer_surface_requirements(&layer);

        assert!(requirements
            .surface_requirements
            .contains(SurfaceRequirement::TextMaterialMask));
        assert_eq!(
            composite_sample_mode_for_requirements(false, false, requirements),
            CompositeSampleMode::Linear
        );
    }

    #[test]
    fn inherited_translated_text_local_surface_uses_box4_layer_surface() {
        let layer = text_layer_with_style(
            AnnotatedString::from("shadow"),
            TextStyle::from_span_style(SpanStyle {
                shadow: Some(Shadow {
                    color: Color::BLACK,
                    offset: Point::new(1.0, 2.0),
                    blur_radius: 3.0,
                }),
                ..SpanStyle::default()
            }),
        );
        let requirements = layer_surface_requirements(&layer);

        assert!(requirements
            .surface_requirements
            .contains(SurfaceRequirement::TextMaterialMask));
        assert_eq!(
            composite_sample_mode_for_requirements(true, false, requirements),
            CompositeSampleMode::Box4
        );
        assert_eq!(
            layer_surface_target_scale(true, false, requirements, 1.25),
            SurfaceRequirementSet::default()
                .with(SurfaceRequirement::TextMaterialMask)
                .with(SurfaceRequirement::MotionStableCapture)
                .target_scale(1.25)
        );
    }

    #[test]
    fn translated_text_local_surface_inside_capture_keeps_parent_scale() {
        let layer = text_layer_with_style(
            AnnotatedString::from("shadow"),
            TextStyle::from_span_style(SpanStyle {
                shadow: Some(Shadow {
                    color: Color::BLACK,
                    offset: Point::new(1.0, 2.0),
                    blur_radius: 3.0,
                }),
                ..SpanStyle::default()
            }),
        );
        let requirements = layer_surface_requirements(&layer);

        assert_eq!(
            composite_sample_mode_for_requirements(true, true, requirements),
            CompositeSampleMode::Linear
        );
        assert_eq!(
            layer_surface_target_scale(true, true, requirements, 10.0),
            SurfaceRequirementSet::default()
                .with(SurfaceRequirement::TextMaterialMask)
                .target_scale(10.0)
        );
    }

    #[test]
    fn layer_surface_requirements_use_local_surface_for_gradient_and_stroke_text() {
        let cases = [
            (
                "draw_style",
                AnnotatedString::from("draw_style"),
                TextStyle::from_span_style(SpanStyle {
                    draw_style: Some(TextDrawStyle::Stroke { width: 2.0 }),
                    ..SpanStyle::default()
                }),
            ),
            (
                "gradient_brush",
                AnnotatedString::from("gradient"),
                TextStyle::from_span_style(SpanStyle {
                    brush: Some(Brush::linear_gradient(vec![Color::WHITE, Color::BLACK])),
                    ..SpanStyle::default()
                }),
            ),
        ];

        for (label, text, text_style) in cases {
            let layer = text_layer_with_style(text, text_style);
            let requirements = layer_surface_requirements(&layer);
            assert!(
                requirements
                    .surface_requirements
                    .contains(SurfaceRequirement::TextMaterialMask),
                "{label} text should use a bounded local surface: {requirements:?}"
            );
        }
    }

    #[test]
    fn layer_surface_requirements_use_local_surface_for_complex_text_effects() {
        let cases = [
            (
                "shadow",
                AnnotatedString::from("shadow"),
                TextStyle::from_span_style(SpanStyle {
                    shadow: Some(Shadow {
                        color: Color::BLACK,
                        offset: Point::new(1.0, 2.0),
                        blur_radius: 3.0,
                    }),
                    ..SpanStyle::default()
                }),
            ),
            (
                "background",
                AnnotatedString::from("background"),
                TextStyle::from_span_style(SpanStyle {
                    background: Some(Color::BLACK),
                    ..SpanStyle::default()
                }),
            ),
            (
                "baseline_shift",
                AnnotatedString::from("baseline_shift"),
                TextStyle::from_span_style(SpanStyle {
                    baseline_shift: Some(BaselineShift::SUPERSCRIPT),
                    ..SpanStyle::default()
                }),
            ),
            (
                "geometric_transform",
                AnnotatedString::from("geometric_transform"),
                TextStyle::from_span_style(SpanStyle {
                    text_geometric_transform: Some(TextGeometricTransform {
                        scale_x: 1.2,
                        skew_x: 0.15,
                    }),
                    ..SpanStyle::default()
                }),
            ),
            (
                "letter_spacing",
                AnnotatedString::from("letter_spacing"),
                TextStyle::from_span_style(SpanStyle {
                    letter_spacing: TextUnit::Em(0.2),
                    ..SpanStyle::default()
                }),
            ),
        ];

        for (label, text, text_style) in cases {
            let layer = text_layer_with_style(text, text_style);
            let requirements = layer_surface_requirements(&layer);
            assert!(
                requirements
                    .surface_requirements
                    .contains(SurfaceRequirement::TextMaterialMask),
                "{label} text should use a bounded local surface: {requirements:?}"
            );
            assert_eq!(
                requirements.direct_translation,
                Some(Point::default()),
                "{label} text should still classify as a direct translation"
            );
        }
    }

    #[test]
    fn layer_surface_requirements_color_only_span_styles_use_direct_path() {
        let layer = text_layer_with_style(
            AnnotatedString {
                text: "styled".to_string(),
                span_styles: vec![RangeStyle {
                    item: SpanStyle {
                        color: Some(Color::BLACK),
                        ..SpanStyle::default()
                    },
                    range: 0..3,
                }],
                ..AnnotatedString::default()
            },
            TextStyle::default(),
        );
        let requirements = layer_surface_requirements(&layer);
        assert!(
            !requirements
                .surface_requirements
                .contains(SurfaceRequirement::TextMaterialMask),
            "color-only span styles should render directly via cosmic-text per-glyph colors"
        );
    }

    #[test]
    fn layer_surface_requirements_keep_decoration_only_text_on_direct_path() {
        let layer = text_layer_with_style(
            AnnotatedString::from("decoration"),
            TextStyle::from_span_style(SpanStyle {
                text_decoration: Some(TextDecoration::UNDERLINE),
                ..SpanStyle::default()
            }),
        );

        let requirements = layer_surface_requirements(&layer);

        assert_eq!(requirements.direct_translation, Some(Point::default()));
        assert!(
            !requirements.surface_requirements.has_any(),
            "decoration-only text should not force any layer surface reasons: {requirements:?}"
        );
    }

    #[test]
    fn direct_text_leaf_snaps_modifier_background_and_text_with_one_anchor() {
        let root = snapped_text_leaf_root(false, false);
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.scene.shapes.len(), 1);
        assert_eq!(collected.scene.texts.len(), 1);
        let expected_anchor = Some(SnapAnchor::rigid(Point::new(14.25, 16.5)));
        assert_eq!(collected.scene.shapes[0].snap_anchor, expected_anchor);
        assert_eq!(collected.scene.texts[0].snap_anchor, expected_anchor);
    }

    #[test]
    fn translated_animated_text_leaf_disables_snap_for_smooth_scroll() {
        let root = snapped_text_leaf_root(true, true);
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.child_layers.len(), 0);
        assert_eq!(collected.scene.shapes.len(), 1);
        assert_eq!(collected.scene.texts.len(), 1);
        assert_eq!(collected.scene.effect_layers.len(), 1);
        assert_eq!(
            collected.scene.shapes[0].snap_anchor, None,
            "scrollable content must not snap — discrete jumps cause visible rendering artifacts"
        );
        assert_eq!(
            collected.scene.texts[0].snap_anchor, None,
            "scrollable text must not snap — discrete jumps cause visible rendering artifacts"
        );
    }

    #[test]
    fn translated_content_context_text_leaf_disables_snap_for_smooth_scroll() {
        let root = snapped_text_leaf_root(false, true);
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.child_layers.len(), 0);
        assert_eq!(collected.scene.shapes.len(), 1);
        assert_eq!(collected.scene.texts.len(), 1);
        assert_eq!(collected.scene.effect_layers.len(), 1);
        assert_eq!(
            collected.scene.shapes[0].snap_anchor, None,
            "scrollable content must not snap — discrete jumps cause visible rendering artifacts"
        );
        assert_eq!(
            collected.scene.texts[0].snap_anchor, None,
            "scrollable text must not snap — discrete jumps cause visible rendering artifacts"
        );
    }

    #[test]
    fn complex_text_uses_local_surface() {
        let root = translated_content_local_surface_root();
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert!(
            !collected.child_layers.is_empty(),
            "translated-content effectful text should render through a bounded local surface"
        );
        assert!(collected.scene.texts.is_empty());
        assert!(collected.scene.shadow_draws.is_empty());
    }

    #[test]
    fn translated_layer_surface_capture_does_not_restart_local_picture_for_shadow_text() {
        let mut layer = text_layer_with_style(
            AnnotatedString::from("shadow"),
            TextStyle::from_span_style(SpanStyle {
                shadow: Some(Shadow {
                    color: Color::BLACK,
                    offset: Point::new(1.0, 2.0),
                    blur_radius: 3.0,
                }),
                ..SpanStyle::default()
            }),
        );
        layer.translated_content_context = true;
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected = collect_layer_contents_with_translation_context(
            &layer,
            None,
            None,
            TranslationRenderContext {
                inherited_content_translation: false,
                surface_capture_active: true,
            },
            &mut rect_cache,
            &mut requirements_cache,
        );

        assert!(
            collected.scene.effect_layers.is_empty(),
            "a translated layer surface already provides the stable local capture"
        );
        assert_eq!(collected.scene.shadow_draws.len(), 1);
        assert_eq!(collected.scene.texts.len(), 1);
        assert!(
            !collected.scene.texts[0].translated_content_context,
            "text inside an active motion-stable capture must raster in capture-local coordinates"
        );
    }

    #[test]
    fn translated_layer_surface_capture_keeps_only_material_effect_layers() {
        let mut layer = text_layer_with_style(
            AnnotatedString::from("gradient"),
            TextStyle::from_span_style(SpanStyle {
                brush: Some(Brush::linear_gradient(vec![Color::WHITE, Color::BLACK])),
                ..SpanStyle::default()
            }),
        );
        layer.translated_content_context = true;
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected = collect_layer_contents_with_translation_context(
            &layer,
            None,
            None,
            TranslationRenderContext {
                inherited_content_translation: false,
                surface_capture_active: true,
            },
            &mut rect_cache,
            &mut requirements_cache,
        );

        assert_eq!(collected.scene.effect_layers.len(), 1);
        assert!(
            collected.scene.effect_layers[0]
                .requirements
                .contains(SurfaceRequirement::MotionStableCapture),
            "translated text materials still need motion-stable resolve semantics inside a stable capture"
        );
        assert_eq!(
            composite_sample_mode_for_effect_layer(&collected.scene.effect_layers[0]),
            CompositeSampleMode::Box4
        );
        assert_eq!(
            effect_layer_target_scale(&collected.scene.effect_layers[0], 10.0),
            10.0
        );
        assert!(collected.scene.effect_layers[0].effect.is_some());
    }

    #[test]
    fn static_pure_text_leaf_snaps_without_sibling_draw_primitives() {
        let root = pure_text_leaf_root(false, false);
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.scene.texts.len(), 1);
        assert!(
            collected.scene.texts[0].snap_anchor.is_some(),
            "idle pure text leaves should participate in rigid snap anchoring"
        );
    }

    #[test]
    fn animated_pure_text_leaf_stays_unsnapped() {
        let root = pure_text_leaf_root(true, false);
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.scene.texts.len(), 1);
        assert_eq!(collected.scene.texts[0].snap_anchor, None);
    }

    #[test]
    fn translated_pure_text_leaf_disables_snap_for_smooth_scroll() {
        let root = pure_text_leaf_root(false, true);
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.child_layers.len(), 0);
        assert_eq!(collected.scene.texts.len(), 1);
        assert_eq!(collected.scene.effect_layers.len(), 1);
        assert_eq!(
            collected.scene.texts[0].snap_anchor, None,
            "scrollable text must not snap — discrete jumps cause visible rendering artifacts"
        );
    }

    #[test]
    fn static_gpu_effect_text_leaf_stays_unsnapped() {
        let root = text_layer_with_style(
            AnnotatedString::from("Gradient"),
            TextStyle::from_span_style(SpanStyle {
                brush: Some(Brush::linear_gradient(vec![
                    Color(0.2, 0.8, 1.0, 1.0),
                    Color(1.0, 0.7, 0.4, 1.0),
                ])),
                draw_style: Some(TextDrawStyle::Stroke { width: 2.5 }),
                ..SpanStyle::default()
            }),
        );
        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();

        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(collected.scene.texts.len(), 1);
        assert_eq!(
            collected.scene.texts[0].snap_anchor, None,
            "gpu text-effect leaves must not take the rigid text snap path"
        );
        assert_eq!(
            collected.scene.effect_layers.len(),
            1,
            "gradient stroke text should still emit a runtime shader effect layer"
        );
    }

    #[test]
    fn layer_surface_requirements_keep_shape_plus_direct_child_on_direct_path() {
        let mut child = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 20.0,
            },
            vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: DrawPrimitive::Rect {
                        rect: Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 40.0,
                            height: 20.0,
                        },
                        brush: Brush::solid(Color::WHITE),
                    },
                    clip: None,
                }),
            })],
        );
        child.transform_to_parent = ProjectiveTransform::translation(8.0, 6.0);

        let layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 32.0,
            },
            vec![
                RenderNode::Primitive(PrimitiveEntry {
                    phase: PrimitivePhase::BeforeChildren,
                    node: PrimitiveNode::Draw(DrawPrimitiveNode {
                        primitive: DrawPrimitive::Rect {
                            rect: Rect {
                                x: 0.0,
                                y: 0.0,
                                width: 64.0,
                                height: 32.0,
                            },
                            brush: Brush::solid(Color::BLACK),
                        },
                        clip: None,
                    }),
                }),
                RenderNode::Layer(Box::new(child)),
            ],
        );

        let requirements = layer_surface_requirements(&layer);

        assert_eq!(requirements.direct_translation, Some(Point::default()));
        assert!(!requirements
            .surface_requirements
            .contains(SurfaceRequirement::MixedDirectContent));
        assert!(!requirements.surface_requirements.has_any());
    }

    #[test]
    fn collect_layer_contents_translates_direct_text_rects_into_parent_space() {
        let mut child = text_layer_with_style(
            AnnotatedString::from("direct"),
            TextStyle::from_span_style(SpanStyle {
                text_decoration: Some(TextDecoration::UNDERLINE),
                ..SpanStyle::default()
            }),
        );
        child.transform_to_parent = ProjectiveTransform::translation(9.0, 7.0);

        let parent = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 32.0,
            },
            vec![RenderNode::Layer(Box::new(child))],
        );

        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();
        let collected = collect_layer_contents(
            &parent,
            None,
            None,
            &mut rect_cache,
            &mut requirements_cache,
        );

        assert!(
            collected.child_layers.is_empty(),
            "decoration-only text child should collapse directly into the parent scene"
        );
        assert_eq!(collected.scene.texts.len(), 1, "expected one text draw");
        let text = &collected.scene.texts[0];
        assert!(
            text.rect.x >= 9.0 && text.rect.y >= 7.0,
            "collapsed text rect should be translated into parent space, got {:?}",
            text.rect
        );
        assert!(
            collected
                .scene
                .shapes
                .iter()
                .any(|shape| shape.rect.y >= 7.0),
            "collapsed underline geometry should also be translated into parent space"
        );
    }

    #[test]
    fn direct_translation_accepts_nearly_identity_axis_scale_noise() {
        let local_bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 393.3,
            height: 16.8,
        };
        let quad = [
            [10.0, 78.399_994],
            [403.3, 78.399_994],
            [10.0, 95.2],
            [403.3, 95.2],
        ];
        let transform = ProjectiveTransform::from_rect_to_quad(local_bounds, quad);

        assert_eq!(
            direct_translation(transform),
            Some(Point::new(10.0, 78.399_994)),
        );
    }

    #[test]
    fn layer_surface_requirements_keep_shape_plus_isolating_child_as_mixed_content() {
        let mut child = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 24.0,
                height: 18.0,
            },
            vec![RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(DrawPrimitiveNode {
                    primitive: DrawPrimitive::Rect {
                        rect: Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 24.0,
                            height: 18.0,
                        },
                        brush: Brush::solid(Color::WHITE),
                    },
                    clip: None,
                }),
            })],
        );
        child.transform_to_parent = ProjectiveTransform::translation(8.0, 6.0);
        child.graphics_layer.render_effect = Some(RenderEffect::blur(2.0));

        let layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 32.0,
            },
            vec![
                RenderNode::Primitive(PrimitiveEntry {
                    phase: PrimitivePhase::BeforeChildren,
                    node: PrimitiveNode::Draw(DrawPrimitiveNode {
                        primitive: DrawPrimitive::Rect {
                            rect: Rect {
                                x: 0.0,
                                y: 0.0,
                                width: 64.0,
                                height: 32.0,
                            },
                            brush: Brush::solid(Color::BLACK),
                        },
                        clip: None,
                    }),
                }),
                RenderNode::Layer(Box::new(child)),
            ],
        );

        let requirements = layer_surface_requirements(&layer);

        assert!(requirements
            .surface_requirements
            .contains(SurfaceRequirement::MixedDirectContent));
        assert!(!requirements
            .surface_requirements
            .has_isolating_requirement());
    }

    #[test]
    fn build_scene_window_filters_and_translates_items() {
        let mut shape = test_shape(6, BlendMode::SrcOver);
        shape.rect.x = 12.0;
        shape.rect.y = 25.0;
        shape.local_rect.x = 12.0;
        shape.local_rect.y = 25.0;
        shape.quad = [[12.0, 25.0], [20.0, 25.0], [12.0, 33.0], [20.0, 33.0]];
        shape.clip = Some(Rect {
            x: 11.0,
            y: 24.0,
            width: 10.0,
            height: 10.0,
        });

        let mut image = test_image(8, BlendMode::SrcOver);
        image.rect.x = 18.0;
        image.rect.y = 27.0;
        image.local_rect.x = 18.0;
        image.local_rect.y = 27.0;
        image.quad = [[18.0, 27.0], [26.0, 27.0], [18.0, 35.0], [26.0, 35.0]];

        let mut text = test_text(9);
        text.rect.x = 16.0;
        text.rect.y = 29.0;
        text.clip = Some(Rect {
            x: 15.0,
            y: 28.0,
            width: 9.0,
            height: 6.0,
        });

        let mut shadow_shape = test_shape(7, BlendMode::SrcOver);
        shadow_shape.rect.x = 14.0;
        shadow_shape.rect.y = 26.0;
        shadow_shape.local_rect.x = 14.0;
        shadow_shape.local_rect.y = 26.0;
        shadow_shape.quad = [[14.0, 26.0], [22.0, 26.0], [14.0, 34.0], [22.0, 34.0]];
        let mut shadow = test_shadow_draw(vec![(shadow_shape, BlendMode::SrcOver)]);
        shadow.z_index = 7;

        let mut nested_effect = effect_layer(6, 10);
        nested_effect.rect.x = 13.0;
        nested_effect.rect.y = 24.0;
        nested_effect.clip = Some(Rect {
            x: 15.0,
            y: 25.0,
            width: 4.0,
            height: 5.0,
        });

        let mut nested_backdrop = backdrop_layer(8);
        nested_backdrop.rect.x = 17.0;
        nested_backdrop.rect.y = 26.0;
        nested_backdrop.clip = Some(Rect {
            x: 18.0,
            y: 27.0,
            width: 3.0,
            height: 4.0,
        });

        let window = build_scene_window(
            SceneWindowSource {
                shapes: &[test_shape(4, BlendMode::SrcOver), shape],
                images: &[image],
                texts: &[text],
                shadow_draws: &[shadow],
                effect_layers: &[effect_layer(2, 4), nested_effect.clone()],
                backdrop_layers: &[backdrop_layer(4), nested_backdrop.clone()],
            },
            5,
            10,
            Rect {
                x: 10.0,
                y: 20.0,
                width: 20.0,
                height: 20.0,
            },
        );

        assert_eq!(window.shapes.len(), 1);
        assert_eq!(
            window.shapes[0].rect,
            Rect {
                x: 2.0,
                y: 5.0,
                width: 8.0,
                height: 8.0,
            }
        );
        assert_eq!(
            window.shapes[0].clip,
            Some(Rect {
                x: 1.0,
                y: 4.0,
                width: 10.0,
                height: 10.0,
            })
        );
        assert_eq!(window.images.len(), 1);
        assert_eq!(window.images[0].rect.x, 8.0);
        assert_eq!(window.images[0].rect.y, 7.0);
        assert_eq!(window.texts.len(), 1);
        assert_eq!(window.texts[0].rect.x, 6.0);
        assert_eq!(window.texts[0].rect.y, 9.0);
        assert_eq!(
            window.texts[0].clip,
            Some(Rect {
                x: 5.0,
                y: 8.0,
                width: 9.0,
                height: 6.0,
            })
        );
        assert_eq!(window.shadow_draws.len(), 1);
        assert_eq!(window.shadow_draws[0].shapes[0].0.rect.x, 4.0);
        assert_eq!(window.shadow_draws[0].shapes[0].0.rect.y, 6.0);
        assert_eq!(window.effect_layers.len(), 1);
        assert_eq!(
            window.effect_layers[0].rect,
            Rect {
                x: 3.0,
                y: 4.0,
                width: 10.0,
                height: 10.0,
            }
        );
        assert_eq!(
            window.effect_layers[0].clip,
            Some(Rect {
                x: 5.0,
                y: 5.0,
                width: 4.0,
                height: 5.0,
            })
        );
        assert_eq!(window.backdrop_layers.len(), 1);
        assert_eq!(
            window.backdrop_layers[0].rect,
            Rect {
                x: 7.0,
                y: 6.0,
                width: 10.0,
                height: 10.0,
            }
        );
        assert_eq!(
            window.backdrop_layers[0].clip,
            Some(Rect {
                x: 8.0,
                y: 7.0,
                width: 3.0,
                height: 4.0,
            })
        );
    }

    #[test]
    fn filtered_effect_layer_index_counts_only_window_members() {
        let effects = vec![
            effect_layer(0, 2),
            effect_layer(5, 12),
            effect_layer(6, 10),
            effect_layer(14, 20),
        ];

        assert_eq!(filtered_effect_layer_index(&effects, 1, 5, 12), Some(0));
        assert_eq!(filtered_effect_layer_index(&effects, 2, 5, 12), Some(1));
        assert_eq!(filtered_effect_layer_index(&effects, 3, 5, 12), None);
    }

    #[test]
    fn blend_mode_support_matrix_is_explicit() {
        assert!(is_blend_mode_supported(BlendMode::SrcOver));
        assert!(is_blend_mode_supported(BlendMode::DstOut));
        assert!(!is_blend_mode_supported(BlendMode::Clear));
        assert!(!is_blend_mode_supported(BlendMode::Multiply));
    }

    #[test]
    fn collect_non_effect_segment_items_preserves_global_z_order() {
        let shapes = vec![
            test_shape(3, BlendMode::SrcOver),
            test_shape(1, BlendMode::DstOut),
        ];
        let images = vec![test_image(2, BlendMode::SrcOver)];
        let texts = vec![test_text(0)];
        let shadows: Vec<ShadowDraw> = Vec::new();

        let mut scratch = Vec::new();
        collect_non_effect_segment_items(
            &shapes,
            &images,
            &texts,
            &shadows,
            0,
            4,
            &[],
            &mut scratch,
        );
        let items: Vec<_> = scratch.iter().map(|(_, item)| *item).collect();
        assert_eq!(
            items,
            vec![
                SegmentDrawItem::Text(0),
                SegmentDrawItem::Shape(1),
                SegmentDrawItem::Image(0),
                SegmentDrawItem::Shape(0),
            ]
        );
    }

    #[test]
    fn collect_non_effect_segment_items_filters_effect_ranges() {
        let shapes = vec![
            test_shape(1, BlendMode::SrcOver),
            test_shape(3, BlendMode::DstOut),
        ];
        let images = vec![test_image(2, BlendMode::SrcOver)];
        let texts = vec![test_text(4)];
        let shadows: Vec<ShadowDraw> = Vec::new();
        let effect_ranges = [std::ops::Range { start: 2, end: 4 }];

        let mut scratch = Vec::new();
        collect_non_effect_segment_items(
            &shapes,
            &images,
            &texts,
            &shadows,
            0,
            5,
            &effect_ranges,
            &mut scratch,
        );
        let items: Vec<_> = scratch.iter().map(|(_, item)| *item).collect();
        assert_eq!(
            items,
            vec![SegmentDrawItem::Shape(0), SegmentDrawItem::Text(0)]
        );
    }

    #[test]
    fn segment_command_iter_merges_non_conflicting_batches_into_one_chunk() {
        let ordered_items = vec![
            (0, SegmentDrawItem::Shape(0)),
            (1, SegmentDrawItem::Image(0)),
            (2, SegmentDrawItem::Text(0)),
        ];
        let shapes = vec![test_shape(0, BlendMode::SrcOver)];
        let images = vec![test_image(1, BlendMode::DstOut)];

        let commands: Vec<_> = SegmentCommandIter::new(&ordered_items, &shapes, &images).collect();

        assert_eq!(
            commands,
            vec![SegmentRenderCommand::DrawChunk(chunk(&[
                SegmentBatchPlan::Shape {
                    start: 0,
                    end: 1,
                    blend_mode: BlendMode::SrcOver,
                },
                SegmentBatchPlan::Image {
                    start: 1,
                    end: 2,
                    blend_mode: BlendMode::DstOut,
                },
                SegmentBatchPlan::Text { start: 2, end: 3 },
            ]))]
        );
    }

    #[test]
    fn segment_command_iter_splits_when_batch_kind_repeats() {
        let ordered_items = vec![
            (0, SegmentDrawItem::Shape(0)),
            (1, SegmentDrawItem::Image(0)),
            (2, SegmentDrawItem::Shape(1)),
        ];
        let shapes = vec![
            test_shape(0, BlendMode::SrcOver),
            test_shape(2, BlendMode::DstOut),
        ];
        let images = vec![test_image(1, BlendMode::SrcOver)];

        let commands: Vec<_> = SegmentCommandIter::new(&ordered_items, &shapes, &images).collect();

        assert_eq!(
            commands,
            vec![
                SegmentRenderCommand::DrawChunk(chunk(&[
                    SegmentBatchPlan::Shape {
                        start: 0,
                        end: 1,
                        blend_mode: BlendMode::SrcOver,
                    },
                    SegmentBatchPlan::Image {
                        start: 1,
                        end: 2,
                        blend_mode: BlendMode::SrcOver,
                    },
                ])),
                SegmentRenderCommand::DrawChunk(chunk(&[SegmentBatchPlan::Shape {
                    start: 2,
                    end: 3,
                    blend_mode: BlendMode::DstOut,
                }])),
            ]
        );
    }

    #[test]
    fn segment_command_iter_keeps_shadows_as_explicit_boundaries() {
        let ordered_items = vec![
            (0, SegmentDrawItem::Shape(0)),
            (1, SegmentDrawItem::Shadow(0)),
            (2, SegmentDrawItem::Image(0)),
            (3, SegmentDrawItem::Text(0)),
        ];
        let shapes = vec![test_shape(0, BlendMode::SrcOver)];
        let images = vec![test_image(2, BlendMode::SrcOver)];

        let commands: Vec<_> = SegmentCommandIter::new(&ordered_items, &shapes, &images).collect();

        assert_eq!(
            commands,
            vec![
                SegmentRenderCommand::DrawChunk(chunk(&[SegmentBatchPlan::Shape {
                    start: 0,
                    end: 1,
                    blend_mode: BlendMode::SrcOver,
                }])),
                SegmentRenderCommand::Shadow(0),
                SegmentRenderCommand::DrawChunk(chunk(&[
                    SegmentBatchPlan::Image {
                        start: 2,
                        end: 3,
                        blend_mode: BlendMode::SrcOver,
                    },
                    SegmentBatchPlan::Text { start: 3, end: 4 },
                ])),
            ]
        );
    }

    #[test]
    fn encoder_buffer_usage_does_not_flush_when_encoder_is_empty() {
        let usage = EncoderBufferUsage::default();
        assert!(!usage.requires_flush_for_batch(BatchKind::Shape, false));
        assert!(!usage.requires_flush_for_batch(BatchKind::Image, false));
        assert!(!usage.requires_flush_for_batch(BatchKind::Text, false));
    }

    #[test]
    fn encoder_buffer_usage_tracks_conflicts_per_batch_kind() {
        let mut usage = EncoderBufferUsage::default();
        usage.mark_batch(BatchKind::Text);

        // Text prepare rewrites shared glyph buffers, so repeated text work
        // in one encoder must flush first.
        assert!(usage.requires_flush_for_batch(BatchKind::Text, true));
        assert!(!usage.requires_flush_for_batch(BatchKind::Shape, true));
        assert!(!usage.requires_flush_for_batch(BatchKind::Image, true));

        usage.mark_batch(BatchKind::Shape);
        assert!(usage.requires_flush_for_batch(BatchKind::Shape, true));
        assert!(!usage.requires_flush_for_batch(BatchKind::Image, true));
        assert!(usage.requires_flush_for_batch(BatchKind::Text, true));
    }

    #[test]
    fn encoder_buffer_usage_reset_clears_all_conflicts() {
        let mut usage = EncoderBufferUsage::default();
        usage.mark_batch(BatchKind::Shape);
        usage.mark_batch(BatchKind::Image);
        usage.mark_batch(BatchKind::Text);
        usage.reset();

        assert!(!usage.requires_flush_for_batch(BatchKind::Shape, true));
        assert!(!usage.requires_flush_for_batch(BatchKind::Image, true));
        assert!(!usage.requires_flush_for_batch(BatchKind::Text, true));
    }

    #[test]
    fn staged_buffer_uploads_align_new_copies_to_copy_buffer_alignment() {
        let mut uploads = StagedBufferUploads::default();
        uploads.bytes.extend_from_slice(&[1, 2]);

        uploads.stage(UploadTarget::ImageIndex, &[3, 4, 5, 6]);

        assert_eq!(uploads.bytes, vec![1, 2, 0, 0, 3, 4, 5, 6]);
        assert_eq!(
            uploads.copies,
            vec![PendingBufferCopy {
                source_offset: 4,
                size: 4,
                target: UploadTarget::ImageIndex,
            }]
        );
    }

    #[test]
    fn staged_buffer_uploads_ignore_empty_payloads() {
        let mut uploads = StagedBufferUploads::default();

        uploads.stage(UploadTarget::Uniform, &[]);

        assert!(uploads.is_empty());
        assert!(uploads.bytes.is_empty());
    }

    #[test]
    fn staged_buffer_uploads_return_exact_payload_slice_for_copy() {
        let mut uploads = StagedBufferUploads::default();
        uploads.stage(UploadTarget::Uniform, &[1, 2, 3, 4]);
        uploads.stage(UploadTarget::ImageIndex, &[5, 6, 7, 8]);

        assert_eq!(uploads.payload_for_copy(uploads.copies[0]), &[1, 2, 3, 4]);
        assert_eq!(uploads.payload_for_copy(uploads.copies[1]), &[5, 6, 7, 8]);
    }

    #[test]
    fn inner_shadow_composite_mask_uses_fill_shape_and_scale() {
        let mut fill = test_shape(0, BlendMode::SrcOver);
        fill.local_rect = Rect {
            x: 10.0,
            y: 12.0,
            width: 40.0,
            height: 20.0,
        };
        fill.shape = Some(RoundedCornerShape::uniform(6.0));

        let cutout = test_shape(1, BlendMode::DstOut);
        let shadow = test_shadow_draw(vec![
            (fill, BlendMode::SrcOver),
            (cutout, BlendMode::DstOut),
        ]);

        let mask = inner_shadow_composite_mask(&shadow, 1.5).expect("inner mask expected");
        assert_eq!(mask.rect, [15.0, 18.0, 60.0, 30.0]);
        assert_eq!(mask.radii, [9.0, 9.0, 9.0, 9.0]);
    }

    #[test]
    fn inner_shadow_composite_mask_is_none_without_dst_out() {
        let fill = test_shape(0, BlendMode::SrcOver);
        let shadow = test_shadow_draw(vec![(fill, BlendMode::SrcOver)]);
        assert!(inner_shadow_composite_mask(&shadow, 1.0).is_none());
    }

    #[test]
    fn render_effect_support_matrix_covers_all_variants() {
        let blur = RenderEffect::blur(4.0);
        let offset = RenderEffect::offset(2.0, 3.0);
        let shader = RenderEffect::runtime_shader(cranpose_ui_graphics::RuntimeShader::new(
            r#"
            @group(0) @binding(0) var input_texture: texture_2d<f32>;
            @group(0) @binding(1) var input_sampler: sampler;
            @group(1) @binding(0) var<uniform> u: array<vec4<f32>, 64>;
            struct VertexOutput {
                @builtin(position) position: vec4<f32>,
                @location(0) uv: vec2<f32>,
            }
            @vertex
            fn fullscreen_vs(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
                var output: VertexOutput;
                let x = f32(i32(vertex_index & 1u) * 2 - 1);
                let y = f32(i32(vertex_index >> 1u) * 2 - 1);
                output.uv = vec2<f32>(x * 0.5 + 0.5, 1.0 - (y * 0.5 + 0.5));
                output.position = vec4<f32>(x, y, 0.0, 1.0);
                return output;
            }
            @fragment
            fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
                return textureSample(input_texture, input_sampler, input.uv);
            }
            "#,
        ));
        let chain = blur.clone().then(offset.clone());

        assert!(is_render_effect_supported(&blur));
        assert!(is_render_effect_supported(&offset));
        assert!(is_render_effect_supported(&shader));
        assert!(is_render_effect_supported(&chain));
    }

    #[test]
    fn clip_to_bounds_propagates_visual_clip_to_all_descendant_shapes() {
        // Simulates: root → clip_to_bounds container → child with shapes above/below clip
        // All shapes inside the clip_to_bounds container must have a clip set.
        let container_local_bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 500.0,
        };
        // Container is placed at y=50 in parent space via transform_to_parent
        let container_clip_in_parent = Rect {
            x: 0.0,
            y: 50.0,
            width: 800.0,
            height: 500.0,
        };

        // Shape that extends above the clip boundary (scroll content scrolled up)
        let shape_above = RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: DrawPrimitive::Rect {
                    rect: Rect {
                        x: 10.0,
                        y: -30.0,
                        width: 100.0,
                        height: 40.0,
                    },
                    brush: Brush::solid(Color::WHITE),
                },
                clip: None,
            }),
        });

        // Shape within the clip boundary
        let shape_inside = RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: DrawPrimitive::Rect {
                    rect: Rect {
                        x: 10.0,
                        y: 100.0,
                        width: 100.0,
                        height: 40.0,
                    },
                    brush: Brush::solid(Color::WHITE),
                },
                clip: None,
            }),
        });

        // Shape below the clip boundary (scroll content below viewport)
        let shape_below = RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: DrawPrimitive::Rect {
                    rect: Rect {
                        x: 10.0,
                        y: 600.0,
                        width: 100.0,
                        height: 40.0,
                    },
                    brush: Brush::solid(Color::WHITE),
                },
                clip: None,
            }),
        });

        // Content child layer (represents scroll content, translated up by scroll offset)
        let mut content_layer = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 1000.0,
            },
            vec![shape_above, shape_inside, shape_below],
        );
        content_layer.transform_to_parent = ProjectiveTransform::translation(0.0, -30.0);
        content_layer.translated_content_context = true;

        // Clip container (e.g. TabContent with clip_to_bounds)
        let mut clip_container = test_layer(
            container_local_bounds,
            vec![RenderNode::Layer(Box::new(content_layer))],
        );
        clip_container.clip_to_bounds = true;
        clip_container.transform_to_parent = ProjectiveTransform::translation(0.0, 50.0);

        // Root
        let root = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            },
            vec![RenderNode::Layer(Box::new(clip_container))],
        );

        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();
        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(
            collected.scene.shapes.len(),
            3,
            "all three shapes should be flattened into the scene"
        );

        for (i, shape) in collected.scene.shapes.iter().enumerate() {
            assert!(
                shape.clip.is_some(),
                "shape {} at rect {:?} must have a clip from clip_to_bounds container, but clip is None",
                i,
                shape.rect
            );
            let clip = shape.clip.unwrap();
            assert_eq!(
                clip, container_clip_in_parent,
                "shape {} clip should match the clip_to_bounds container bounds in parent space",
                i
            );
        }
    }

    #[test]
    fn clip_to_bounds_culls_child_layers_outside_boundary() {
        // Reproduces the out-of-clip rendering bug: a child layer with
        // graphics_layer.clip=true (e.g. from rounded_surface()) positioned
        // entirely below the parent's clip_to_bounds boundary must be culled.
        // Before the fix, resolve_clip returned None for non-overlapping rects,
        // which downstream code interpreted as "no clipping" instead of "fully clipped",
        // causing invisible content to render everywhere.

        let clip_container_bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 500.0,
        };

        let shape_in_card = RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: DrawPrimitive::Rect {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 300.0,
                        height: 80.0,
                    },
                    brush: Brush::solid(Color::WHITE),
                },
                clip: None,
            }),
        });

        // Card layer with graphics_layer.clip=true, positioned BELOW the clip boundary
        let mut card_outside = crate::test_support::layer_node(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 300.0,
                height: 80.0,
            },
            ProjectiveTransform::identity(),
            GraphicsLayer {
                clip: true,
                ..GraphicsLayer::default()
            },
            vec![shape_in_card.clone()],
        );
        card_outside.transform_to_parent = ProjectiveTransform::translation(10.0, 600.0);

        // Card layer with graphics_layer.clip=true, positioned INSIDE the clip boundary
        let mut card_inside = crate::test_support::layer_node(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 300.0,
                height: 80.0,
            },
            ProjectiveTransform::identity(),
            GraphicsLayer {
                clip: true,
                ..GraphicsLayer::default()
            },
            vec![shape_in_card],
        );
        card_inside.transform_to_parent = ProjectiveTransform::translation(10.0, 100.0);

        // Content layer holding both cards
        let content = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 1000.0,
            },
            vec![
                RenderNode::Layer(Box::new(card_inside)),
                RenderNode::Layer(Box::new(card_outside)),
            ],
        );

        // Clip container
        let mut clip_container = test_layer(
            clip_container_bounds,
            vec![RenderNode::Layer(Box::new(content))],
        );
        clip_container.clip_to_bounds = true;

        // Root
        let root = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            },
            vec![RenderNode::Layer(Box::new(clip_container))],
        );

        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();
        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert_eq!(
            collected.scene.shapes.len(),
            1,
            "only the card inside the clip boundary should produce shapes; \
             the card outside must be culled entirely"
        );

        let shape = &collected.scene.shapes[0];
        assert!(
            shape.clip.is_some(),
            "the visible card's shape must have a clip from clip_to_bounds"
        );
    }

    #[test]
    fn flattened_layer_shadow_z_index_is_below_content() {
        // Shadow must render behind content. When a child layer with shadow_elevation
        // is flattened (no isolation), its shadow z-index must be lower than any
        // content z-index so shadow draws render first.
        let shape = RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: DrawPrimitive::Rect {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 100.0,
                    },
                    brush: Brush::solid(Color::WHITE),
                },
                clip: None,
            }),
        });

        let child_bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };

        let child = crate::test_support::layer_node(
            child_bounds,
            ProjectiveTransform::translation(50.0, 50.0),
            GraphicsLayer {
                shadow_elevation: 20.0,
                ..GraphicsLayer::default()
            },
            vec![shape],
        );

        let root = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            },
            vec![RenderNode::Layer(Box::new(child))],
        );

        let mut rect_cache = HashMap::new();
        let mut requirements_cache = HashMap::new();
        let collected =
            collect_layer_contents(&root, None, None, &mut rect_cache, &mut requirements_cache);

        assert!(
            !collected.scene.shadow_draws.is_empty(),
            "shadow_elevation > 0 must produce shadow draws"
        );
        let max_shadow_z = collected
            .scene
            .shadow_draws
            .iter()
            .map(|s| s.z_index)
            .max()
            .unwrap();
        let min_content_z = collected
            .scene
            .shapes
            .iter()
            .map(|s| s.z_index)
            .min()
            .unwrap();
        assert!(
            max_shadow_z < min_content_z,
            "shadow z-index ({}) must be less than content z-index ({}); \
             shadows must render behind their content",
            max_shadow_z,
            min_content_z
        );
    }
}
