//! GPU rendering implementation using WGPU

use crate::effect_renderer::{EffectRenderer, RoundedCompositeMask};
use crate::offscreen::OffscreenTarget;
use crate::scene::{BackdropLayer, DrawShape, EffectLayer, ImageDraw, ShadowDraw, TextDraw};
use crate::shaders;
use crate::{EnsureTextBufferParams, SharedTextBuffer, SharedTextCache, TextCacheKey};
use bytemuck::{Pod, Zeroable};
use cranpose_ui_graphics::{
    BlendMode, Brush, Color, ColorFilter, ImageBitmap, Rect, RenderEffect, TileMode,
};
use glyphon::{
    Cache, Color as GlyphonColor, FontSystem, Metrics, Resolution, SwashCache, TextArea, TextAtlas,
    TextBounds, TextRenderer, Viewport,
};
use lru::LruCache;
use std::num::NonZeroUsize;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use crate::gpu_stats;
use crate::gpu_stats::gpu_stats_enabled;

// Chunked rendering constants for robustness with large scenes
// Note: Limited to 256 for WebGL compatibility (uniform buffer size limit)
// WebGL guarantees 16KB uniform buffers, ShapeData is 64 bytes = 256 max shapes
const HARD_MAX_BUFFER_MB: usize = 64; // Maximum 64MB per buffer
const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: 18.0 / 255.0,
    g: 18.0 / 255.0,
    b: 24.0 / 255.0,
    a: 1.0,
};
const MAX_TEXTURE_CACHE_ITEMS: usize = 256;
static REPORTED_UNSUPPORTED_WGPU_BLEND_MODES: AtomicBool = AtomicBool::new(false);
static REPORTED_UNSUPPORTED_WGPU_EFFECTS: AtomicBool = AtomicBool::new(false);

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
        push_constant_ranges: &[],
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
        multiview: None,
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
        push_constant_ranges: &[],
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
        multiview: None,
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

struct CachedImageTexture {
    bind_group: wgpu::BindGroup,
}

struct ImageDrawCmd {
    index_start: u32,
    scissor: (u32, u32, u32, u32),
    image_id: u64,
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

pub struct GpuRenderer {
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    #[allow(dead_code)] // Kept for potential future use (e.g., recreating text atlas)
    surface_format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    pipeline_dst_out: wgpu::RenderPipeline,
    shape_bind_group_layout: wgpu::BindGroupLayout,
    image_pipeline: wgpu::RenderPipeline,
    image_pipeline_dst_out: wgpu::RenderPipeline,
    image_bind_group_layout: wgpu::BindGroupLayout,
    image_sampler: wgpu::Sampler,
    font_system: Arc<Mutex<FontSystem>>,
    text_renderer: TextRenderer,
    text_atlas: TextAtlas,
    swash_cache: SwashCache,
    // Persistent GPU buffers (reused across frames)
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    shape_buffers: ShapeBatchBuffers,
    image_vertex_buffer: wgpu::Buffer,
    image_index_buffer: wgpu::Buffer,
    image_texture_cache: LruCache<u64, CachedImageTexture>,
    // Shared text cache used by both measurement and rendering
    text_cache: SharedTextCache,
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
    effect_renderer: EffectRenderer,
    frame_stats: gpu_stats::FrameStats,
    frame_count: u64,
    gpu_stats_enabled: bool,
}

impl GpuRenderer {
    pub fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
        font_system: Arc<Mutex<FontSystem>>,
        text_cache: SharedTextCache,
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
        let glyphon_cache = Cache::new(&device);
        let mut text_atlas = TextAtlas::new(&device, &queue, &glyphon_cache, surface_format);

        log::info!(
            "Text renderer initialized with format: {:?}",
            surface_format
        );

        let text_renderer = TextRenderer::new(
            &mut text_atlas,
            &device,
            wgpu::MultisampleState::default(),
            None,
        );
        let text_viewport = Viewport::new(&device, &glyphon_cache);

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

        let image_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Image Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
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

        let effect_renderer = EffectRenderer::new(&device, surface_format);

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
            image_sampler,
            font_system,
            text_renderer,
            text_atlas,
            swash_cache,
            uniform_buffer,
            uniform_bind_group,
            shape_buffers,
            image_vertex_buffer,
            image_index_buffer,
            image_texture_cache: LruCache::new(
                NonZeroUsize::new(MAX_TEXTURE_CACHE_ITEMS)
                    .expect("image texture cache size must be non-zero"),
            ),
            text_cache,
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
            effect_renderer,
            frame_stats: gpu_stats::FrameStats::default(),
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

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Image Texture Bind Group"),
            layout: &self.image_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.image_sampler),
                },
            ],
        });

        self.image_texture_cache
            .put(image.id(), CachedImageTexture { bind_group });
        Ok(())
    }

    /// Acquire an offscreen target from the pool with stats tracking.
    /// Uses split borrows to avoid conflicting borrows on self.
    fn acquire_offscreen(&mut self, width: u32, height: u32) -> OffscreenTarget {
        let stats = if self.gpu_stats_enabled {
            Some(&self.frame_stats)
        } else {
            None
        };
        self.effect_renderer
            .offscreen_pool
            .acquire(&self.device, width, height, stats)
    }

    #[allow(clippy::too_many_arguments)] // Render path needs explicit scene slices and target metadata.
    pub fn render(
        &mut self,
        view: &wgpu::TextureView,
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadow_draws: &[ShadowDraw],
        effect_layers: &[EffectLayer],
        backdrop_layers: &[BackdropLayer],
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Result<(), String> {
        log::trace!(
            "🎨 Rendering: {} shapes, {} images, {} texts, {} shadow draws, {} effect layers, {} backdrop layers (size: {}x{})",
            shapes.len(),
            images.len(),
            texts.len(),
            shadow_draws.len(),
            effect_layers.len(),
            backdrop_layers.len(),
            width,
            height
        );

        debug_assert!(
            shapes
                .windows(2)
                .all(|pair| pair[0].z_index <= pair[1].z_index),
            "shapes must be added in z-index order"
        );
        debug_assert!(
            texts
                .windows(2)
                .all(|pair| pair[0].z_index <= pair[1].z_index),
            "texts must be added in z-index order"
        );
        debug_assert!(
            images
                .windows(2)
                .all(|pair| pair[0].z_index <= pair[1].z_index),
            "images must be added in z-index order"
        );
        debug_assert!(
            shadow_draws
                .windows(2)
                .all(|pair| pair[0].z_index <= pair[1].z_index),
            "shadow draws must be added in z-index order"
        );

        let result = self.render_with_layer_events(
            view,
            shapes,
            images,
            texts,
            shadow_draws,
            effect_layers,
            backdrop_layers,
            width,
            height,
            root_scale,
        );
        if self.gpu_stats_enabled {
            self.effect_renderer
                .merge_and_reset_debug_counters(&self.frame_stats);
            self.frame_stats.print_and_reset(&mut self.frame_count);
        }
        result
    }

    #[allow(clippy::too_many_arguments)] // Mirrors render() call site and scene inputs.
    pub fn render_to_rgba_pixels(
        &mut self,
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadow_draws: &[ShadowDraw],
        effect_layers: &[EffectLayer],
        backdrop_layers: &[BackdropLayer],
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

        self.render(
            &output_view,
            shapes,
            images,
            texts,
            shadow_draws,
            effect_layers,
            backdrop_layers,
            width,
            height,
            root_scale,
        )?;

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
        let _ = self.device.poll(wgpu::PollType::wait_for(submission_index));

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

    #[allow(clippy::too_many_arguments)]
    fn render_with_layer_events(
        &mut self,
        surface_view: &wgpu::TextureView,
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        shadow_draws: &[ShadowDraw],
        effect_layers: &[EffectLayer],
        backdrop_layers: &[BackdropLayer],
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Result<(), String> {
        let scene_end = scene_end_z(
            shapes,
            images,
            texts,
            shadow_draws,
            effect_layers,
            backdrop_layers,
        );

        if effect_layers.is_empty() && backdrop_layers.is_empty() {
            // Fast path: no effect/backdrop layers — render directly to the surface
            // without allocating an accumulation buffer or performing the final blit.
            // The clear is folded into the first render pass's LoadOp::Clear.
            self.render_non_effect_segment(
                surface_view,
                shapes,
                images,
                texts,
                shadow_draws,
                0,
                scene_end,
                &[],
                width,
                height,
                root_scale,
                wgpu::LoadOp::Clear(CLEAR_COLOR),
            )?;
        } else {
            // Double-buffer path: accumulate everything into an intermediate texture.
            // The clear is folded into the first render pass via initial_load_op.
            let accum = self.acquire_offscreen(width, height);

            self.render_range_with_layer_events_to_target(
                &accum,
                shapes,
                images,
                texts,
                shadow_draws,
                effect_layers,
                backdrop_layers,
                0,
                scene_end,
                None,
                None,
                width,
                height,
                root_scale,
                wgpu::LoadOp::Clear(CLEAR_COLOR),
            )?;

            self.effect_renderer.composite_to_view(
                &self.device,
                &self.queue,
                &accum,
                surface_view,
                wgpu::LoadOp::Clear(CLEAR_COLOR),
            );
            self.effect_renderer.offscreen_pool.release(accum);
        }

        let mut text_cache = self.text_cache.lock().unwrap();
        crate::trim_text_cache(&mut text_cache);

        // Trim the GPU text atlas to reclaim space for glyphs no longer in use.
        // Without this, the atlas grows monotonically and eventually hits AtlasFull.
        self.text_atlas.trim();

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn render_range_with_layer_events_to_target(
        &mut self,
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
        backdrop_underlay: Option<&OffscreenTarget>,
        width: u32,
        height: u32,
        root_scale: f32,
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
        let mut encoder_buffer_usage = EncoderBufferUsage::default();

        // The first batch uses the caller's load op (which may be Clear to
        // fold a standalone clear into this segment).  Subsequent batches
        // always use Load to preserve prior content.
        let mut first_batch = true;
        let load_op_for_batch = |first: &mut bool| -> wgpu::LoadOp<wgpu::Color> {
            if *first {
                *first = false;
                initial_load_op
            } else {
                wgpu::LoadOp::Load
            }
        };

        let mut cursor = 0usize;
        while cursor < ordered_items.len() {
            match ordered_items[cursor].1 {
                SegmentDrawItem::Shape(index) => {
                    let blend_mode = supported_blend_mode(shapes[index].blend_mode);
                    let start = cursor;
                    cursor += 1;
                    while cursor < ordered_items.len() {
                        match ordered_items[cursor].1 {
                            SegmentDrawItem::Shape(next_index)
                                if supported_blend_mode(shapes[next_index].blend_mode)
                                    == blend_mode =>
                            {
                                cursor += 1;
                            }
                            _ => break,
                        }
                    }

                    // Shape buffers would be overwritten — flush first.
                    if encoder_buffer_usage
                        .requires_flush_for_batch(BatchKind::Shape, encoder_has_work)
                    {
                        self.queue.submit(std::iter::once(encoder.finish()));
                        self.frame_stats.bump_submits();
                        encoder =
                            self.device
                                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                    label: Some("Segment Encoder"),
                                });
                        encoder_buffer_usage.reset();
                    }

                    let shape_batch: Vec<&DrawShape> = ordered_items[start..cursor]
                        .iter()
                        .map(|(_, item)| match item {
                            SegmentDrawItem::Shape(shape_index) => &shapes[*shape_index],
                            _ => unreachable!("shape batch contains only shape items"),
                        })
                        .collect();
                    let load_op = load_op_for_batch(&mut first_batch);
                    self.encode_shapes_pass(
                        &mut encoder,
                        target_view,
                        &shape_batch,
                        blend_mode,
                        width,
                        height,
                        root_scale,
                        load_op,
                        [0.0, 0.0],
                    );
                    encoder_buffer_usage.mark_batch(BatchKind::Shape);
                    encoder_has_work = true;
                }
                SegmentDrawItem::Image(index) => {
                    let blend_mode = supported_blend_mode(images[index].blend_mode);
                    let start = cursor;
                    cursor += 1;
                    while cursor < ordered_items.len() {
                        match ordered_items[cursor].1 {
                            SegmentDrawItem::Image(next_index)
                                if supported_blend_mode(images[next_index].blend_mode)
                                    == blend_mode =>
                            {
                                cursor += 1;
                            }
                            _ => break,
                        }
                    }

                    // Image buffers would be overwritten — flush first.
                    if encoder_buffer_usage
                        .requires_flush_for_batch(BatchKind::Image, encoder_has_work)
                    {
                        self.queue.submit(std::iter::once(encoder.finish()));
                        self.frame_stats.bump_submits();
                        encoder =
                            self.device
                                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                    label: Some("Segment Encoder"),
                                });
                        encoder_buffer_usage.reset();
                    }

                    // Prepare image draw commands (ensure cached, build verts)
                    let image_batch: Vec<&ImageDraw> = ordered_items[start..cursor]
                        .iter()
                        .map(|(_, item)| match item {
                            SegmentDrawItem::Image(image_index) => &images[*image_index],
                            _ => unreachable!("image batch contains only image items"),
                        })
                        .collect();
                    let load_op = load_op_for_batch(&mut first_batch);
                    let image_cmds =
                        self.prepare_image_draw_cmds(&image_batch, width, height, root_scale)?;
                    self.encode_images_pass(
                        &mut encoder,
                        target_view,
                        &image_cmds,
                        blend_mode,
                        load_op,
                    )?;
                    self.scratch_image_cmds = image_cmds;
                    encoder_buffer_usage.mark_batch(BatchKind::Image);
                    encoder_has_work = true;
                }
                SegmentDrawItem::Text(_) => {
                    let start = cursor;
                    cursor += 1;
                    while cursor < ordered_items.len() {
                        match ordered_items[cursor].1 {
                            SegmentDrawItem::Text(_) => {
                                cursor += 1;
                            }
                            _ => break,
                        }
                    }

                    // Glyphon text prepare uploads into shared GPU buffers.
                    // A second text batch in the same encoder would overwrite
                    // already-recorded text pass data before submission.
                    if encoder_buffer_usage
                        .requires_flush_for_batch(BatchKind::Text, encoder_has_work)
                    {
                        self.queue.submit(std::iter::once(encoder.finish()));
                        self.frame_stats.bump_submits();
                        encoder =
                            self.device
                                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                    label: Some("Segment Encoder"),
                                });
                        encoder_buffer_usage.reset();
                    }

                    let text_batch: Vec<&TextDraw> = ordered_items[start..cursor]
                        .iter()
                        .map(|(_, item)| match item {
                            SegmentDrawItem::Text(text_index) => &texts[*text_index],
                            _ => unreachable!("text batch contains only text items"),
                        })
                        .collect();
                    let load_op = load_op_for_batch(&mut first_batch);
                    // Text prepare (shaping, atlas upload) must happen before
                    // we record the pass.
                    self.prepare_text_for_render(&text_batch, width, height, root_scale)?;
                    self.encode_text_pass(&mut encoder, target_view, load_op)?;
                    encoder_has_work = true;
                    encoder_buffer_usage.mark_batch(BatchKind::Text);
                }
                SegmentDrawItem::Shadow(index) => {
                    // Shadows involve effect_renderer calls with their own
                    // submits — flush our encoder first.
                    if first_batch && matches!(initial_load_op, wgpu::LoadOp::Clear(_)) {
                        // Record a clear pass so the shadow composites onto
                        // initialized content.
                        {
                            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("Shadow Pre-Clear"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: target_view,
                                    resolve_target: None,
                                    ops: wgpu::Operations {
                                        load: initial_load_op,
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: None,
                                timestamp_writes: None,
                                occlusion_query_set: None,
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
                        encoder_buffer_usage.reset();
                    }
                    cursor += 1;
                    self.render_shadow_draw(
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
        }

        self.scratch_segment_items = ordered_items;
        Ok(())
    }

    /// Renders a shadow via offscreen target + Gaussian blur + composite.
    fn render_shadow_draw(
        &mut self,
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

        let blur_margin = (shadow.blur_radius * 3.0).max(1.0);
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
                let text_refs: Vec<&TextDraw> = shadow.texts.iter().collect();
                if let Err(e) = self.prepare_text_for_render(&text_refs, width, height, root_scale)
                {
                    eprintln!("Failed to prepare text for zero-blur shadow: {}", e);
                } else {
                    let mut encoder =
                        self.device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("Zero Blur Shadow Text Encoder"),
                            });
                    if let Err(e) =
                        self.encode_text_pass(&mut encoder, target_view, wgpu::LoadOp::Load)
                    {
                        eprintln!("Failed to encode text for zero-blur shadow: {}", e);
                    }
                    self.queue.submit(std::iter::once(encoder.finish()));
                    self.frame_stats.bump_submits();
                }
            }
            return;
        }

        // Compute pixel-space bounds for the offscreen textures, clamped to viewport.
        let bounds_x = (blur_bounds.x * root_scale).max(0.0);
        let bounds_y = (blur_bounds.y * root_scale).max(0.0);
        let bounds_r = ((blur_bounds.x + blur_bounds.width) * root_scale).min(width as f32);
        let bounds_b = ((blur_bounds.y + blur_bounds.height) * root_scale).min(height as f32);
        let bounds_w = (bounds_r - bounds_x).ceil().max(1.0) as u32;
        let bounds_h = (bounds_b - bounds_y).ceil().max(1.0) as u32;

        // 1. Acquire bounds-sized offscreen source.
        let source = self.acquire_offscreen(bounds_w, bounds_h);

        // 2. Render shadow shapes to bounds-sized offscreen.
        //    The viewport_offset shifts the coordinate origin so that shapes
        //    at absolute viewport positions render into the small texture.
        //    The first shape gets LoadOp::Clear to initialize the texture;
        //    subsequent shapes use LoadOp::Load.
        let viewport_offset = [bounds_x.floor(), bounds_y.floor()];
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

            let text_refs: Vec<&TextDraw> = shifted_texts.iter().collect();
            if let Err(e) = self.prepare_text_for_render(&text_refs, bounds_w, bounds_h, root_scale)
            {
                eprintln!("Failed to prepare text for shadow: {}", e);
            } else {
                let mut encoder =
                    self.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("Shadow Text Encoder"),
                        });
                if let Err(e) = self.encode_text_pass(&mut encoder, &source.view, load) {
                    eprintln!("Failed to encode text for shadow: {}", e);
                }
                self.queue.submit(std::iter::once(encoder.finish()));
                self.frame_stats.bump_submits();
            }
        }

        // 3. Apply Gaussian blur on the bounds-sized textures.
        let dest = self.acquire_offscreen(bounds_w, bounds_h);
        let pixel_radius = shadow.blur_radius * root_scale;
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
            );
        self.effect_renderer.offscreen_pool.release(dest);

        // Restore viewport uniform to full size so subsequent image/text rendering
        // (which shares the uniform_buffer but doesn't write to it) works correctly.
        let uniforms = Uniforms {
            viewport: [width as f32, height as f32],
            viewport_offset: [0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
    }

    #[allow(clippy::too_many_arguments)]
    fn render_effect_layer_to_target(
        &mut self,
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
        let layer = effect_layers
            .get(effect_layer_index)
            .cloned()
            .ok_or_else(|| "effect layer index out of bounds".to_string())?;
        let Some(scissor) =
            scissor_rect_for_layer(layer.rect, layer.clip, root_scale, width, height)
        else {
            return Ok(());
        };

        let source = self.acquire_offscreen(width, height);
        // The clear is folded into render_range_with_layer_events_to_target's
        // initial_load_op below, avoiding a standalone clear submit.

        // Nested backdrop layers inside this effect-isolated subtree should still
        // be able to sample the true scene content behind the subtree.
        let has_nested_backdrop =
            has_backdrop_layer_in_range(backdrop_layers, layer.z_start, layer.z_end);
        let layer_underlay = if has_nested_backdrop {
            let underlay = self.acquire_offscreen(width, height);

            if let Some(existing_underlay) = backdrop_underlay {
                self.effect_renderer.composite_to_view(
                    &self.device,
                    &self.queue,
                    existing_underlay,
                    &underlay.view,
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                );
                self.effect_renderer.composite_to_view(
                    &self.device,
                    &self.queue,
                    target,
                    &underlay.view,
                    wgpu::LoadOp::Load,
                );
            } else {
                self.effect_renderer.composite_to_view(
                    &self.device,
                    &self.queue,
                    target,
                    &underlay.view,
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                );
            }
            Some(underlay)
        } else {
            None
        };

        let render_result = self.render_range_with_layer_events_to_target(
            &source,
            shapes,
            images,
            texts,
            shadow_draws,
            effect_layers,
            backdrop_layers,
            layer.z_start,
            layer.z_end,
            Some(effect_layer_index),
            layer_underlay.as_ref(),
            width,
            height,
            root_scale,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        );

        if let Some(underlay) = layer_underlay {
            self.effect_renderer.offscreen_pool.release(underlay);
        }

        render_result?;

        let dest = self.acquire_offscreen(width, height);

        let layer_pixel_rect = [
            layer.rect.x * root_scale,
            layer.rect.y * root_scale,
            layer.rect.width * root_scale,
            layer.rect.height * root_scale,
        ];

        if let Some(effect) = &layer.effect {
            if is_render_effect_supported(effect) {
                self.effect_renderer.apply_effect(
                    &self.device,
                    &self.queue,
                    &source,
                    &dest.view,
                    effect,
                    layer_pixel_rect,
                );
            } else {
                warn_unsupported_effect_once();
                self.effect_renderer.composite_to_view(
                    &self.device,
                    &self.queue,
                    &source,
                    &dest.view,
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                );
            }
        } else {
            self.effect_renderer.composite_to_view(
                &self.device,
                &self.queue,
                &source,
                &dest.view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            );
        }

        let layer_blend_mode = supported_blend_mode(layer.blend_mode);
        self.effect_renderer
            .composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
                &self.device,
                &self.queue,
                &dest,
                &target.view,
                layer.composite_alpha,
                wgpu::LoadOp::Load,
                Some(scissor),
                None,
                layer_blend_mode,
                None,
            );

        self.effect_renderer.offscreen_pool.release(source);
        self.effect_renderer.offscreen_pool.release(dest);

        Ok(())
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
        let Some(scissor) =
            scissor_rect_for_layer(layer.rect, layer.clip, root_scale, width, height)
        else {
            return Ok(());
        };

        let snapshot = self.acquire_offscreen(width, height);
        if let Some(underlay) = backdrop_underlay {
            self.effect_renderer.composite_to_view(
                &self.device,
                &self.queue,
                underlay,
                &snapshot.view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            );
            self.effect_renderer.composite_to_view(
                &self.device,
                &self.queue,
                target,
                &snapshot.view,
                wgpu::LoadOp::Load,
            );
        } else {
            self.effect_renderer.composite_to_view(
                &self.device,
                &self.queue,
                target,
                &snapshot.view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            );
        }

        let dest = self.acquire_offscreen(width, height);
        let layer_pixel_rect = [
            layer.rect.x * root_scale,
            layer.rect.y * root_scale,
            layer.rect.width * root_scale,
            layer.rect.height * root_scale,
        ];
        self.effect_renderer.apply_effect(
            &self.device,
            &self.queue,
            &snapshot,
            &dest.view,
            &layer.effect,
            layer_pixel_rect,
        );

        self.effect_renderer.composite_to_view_scissored(
            &self.device,
            &self.queue,
            &dest,
            &target.view,
            wgpu::LoadOp::Load,
            Some(scissor),
        );

        self.effect_renderer.offscreen_pool.release(snapshot);
        self.effect_renderer.offscreen_pool.release(dest);

        Ok(())
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
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
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
            layer_shapes,
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

    /// Stage shape buffer writes and record a shape render pass onto the
    /// provided encoder.  The caller is responsible for submitting.
    #[allow(clippy::too_many_arguments)]
    fn encode_shapes_pass(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        layer_shapes: &[&DrawShape],
        blend_mode: BlendMode,
        width: u32,
        height: u32,
        root_scale: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        viewport_offset: [f32; 2],
    ) {
        if layer_shapes.is_empty() {
            return;
        }
        self.frame_stats.bump_shapes();

        // Update viewport uniforms (viewport_offset shifts the origin so that
        // a sub-region of scene space maps to the full render target)
        let uniforms = Uniforms {
            viewport: [width as f32, height as f32],
            viewport_offset,
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        // Build shape data for this subset
        self.scratch_shape_data.clear();
        self.scratch_gradients.clear();
        self.scratch_vertices.clear();
        self.scratch_indices.clear();

        for (idx, shape) in layer_shapes.iter().enumerate() {
            let local_rect = shape.local_rect;

            // Clip rect (scaled to physical pixels)
            let clip_rect = if let Some(clip) = shape.clip {
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

            self.scratch_shape_data.push(ShapeData {
                rect: [
                    local_rect.x * root_scale,
                    local_rect.y * root_scale,
                    local_rect.width * root_scale,
                    local_rect.height * root_scale,
                ],
                radii,
                gradient_params,
                clip_rect,
                brush_type,
                gradient_start,
                gradient_count,
                gradient_tile_mode,
            });

            // Build vertices
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

            self.scratch_vertices.extend_from_slice(&[
                Vertex {
                    position: [shape.quad[0][0] * root_scale, shape.quad[0][1] * root_scale],
                    color,
                    uv: [0.0, 0.0],
                },
                Vertex {
                    position: [shape.quad[1][0] * root_scale, shape.quad[1][1] * root_scale],
                    color,
                    uv: [1.0, 0.0],
                },
                Vertex {
                    position: [shape.quad[2][0] * root_scale, shape.quad[2][1] * root_scale],
                    color,
                    uv: [0.0, 1.0],
                },
                Vertex {
                    position: [shape.quad[3][0] * root_scale, shape.quad[3][1] * root_scale],
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
            return;
        }

        let shape_count = self.scratch_shape_data.len();

        // Ensure buffers have capacity
        self.shape_buffers.ensure_capacity(
            &self.device,
            &self.shape_bind_group_layout,
            shape_count * 4,
            shape_count * 6,
            shape_count,
            self.scratch_gradients.len().max(1),
        );

        // Write data to GPU buffers
        self.queue.write_buffer(
            &self.shape_buffers.vertex_buffer,
            0,
            bytemuck::cast_slice(&self.scratch_vertices),
        );
        self.queue.write_buffer(
            &self.shape_buffers.index_buffer,
            0,
            bytemuck::cast_slice(&self.scratch_indices),
        );
        self.queue.write_buffer(
            &self.shape_buffers.shape_buffer,
            0,
            bytemuck::cast_slice(&self.scratch_shape_data),
        );
        if !self.scratch_gradients.is_empty() {
            self.queue.write_buffer(
                &self.shape_buffers.gradient_buffer,
                0,
                bytemuck::cast_slice(&self.scratch_gradients),
            );
        }

        // Record render pass on the provided encoder
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shape Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

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
            render_pass.draw_indexed(0..(shape_count as u32 * 6), 0, 0..1);
        }
    }

    /// Record an image render pass onto the provided encoder.
    fn encode_images_pass(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        image_cmds: &[ImageDrawCmd],
        blend_mode: BlendMode,
        load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<(), String> {
        if image_cmds.is_empty() {
            return Ok(());
        }
        self.frame_stats.bump_images();
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Image Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(match blend_mode {
                BlendMode::DstOut => &self.image_pipeline_dst_out,
                _ => &self.image_pipeline,
            });
            render_pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            render_pass
                .set_index_buffer(self.image_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            render_pass.set_vertex_buffer(0, self.image_vertex_buffer.slice(..));

            for cmd in image_cmds {
                let (sx, sy, sw, sh) = cmd.scissor;
                render_pass.set_scissor_rect(sx, sy, sw, sh);

                let cached = self
                    .image_texture_cache
                    .get(&cmd.image_id)
                    .ok_or_else(|| "image texture missing from cache".to_string())?;
                render_pass.set_bind_group(1, &cached.bind_group, &[]);
                render_pass.draw_indexed(cmd.index_start..(cmd.index_start + 6), 0, 0..1);
            }
        }
        Ok(())
    }

    /// Prepare image vertices, indices, ensure caching, and write to GPU buffers.
    /// Returns the draw commands needed by `encode_images_pass`.
    fn prepare_image_draw_cmds(
        &mut self,
        layer_images: &[&ImageDraw],
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Result<Vec<ImageDrawCmd>, String> {
        let mut image_vertices = std::mem::take(&mut self.scratch_image_vertices);
        let mut image_indices = std::mem::take(&mut self.scratch_image_indices);
        let mut image_cmds = std::mem::take(&mut self.scratch_image_cmds);
        image_vertices.clear();
        image_indices.clear();
        image_cmds.clear();

        for image_draw in layer_images {
            let rect = image_draw.rect;
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

            let scissor = scissor_rect_for_image(image_draw, root_scale, width, height);
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
                        image_draw.quad[0][0] * root_scale,
                        image_draw.quad[0][1] * root_scale,
                    ],
                    color: tint,
                    uv: [u_min, v_min],
                },
                Vertex {
                    position: [
                        image_draw.quad[1][0] * root_scale,
                        image_draw.quad[1][1] * root_scale,
                    ],
                    color: tint,
                    uv: [u_max, v_min],
                },
                Vertex {
                    position: [
                        image_draw.quad[2][0] * root_scale,
                        image_draw.quad[2][1] * root_scale,
                    ],
                    color: tint,
                    uv: [u_min, v_max],
                },
                Vertex {
                    position: [
                        image_draw.quad[3][0] * root_scale,
                        image_draw.quad[3][1] * root_scale,
                    ],
                    color: tint,
                    uv: [u_max, v_max],
                },
            ]);

            image_cmds.push(ImageDrawCmd {
                index_start,
                scissor,
                image_id: prepared_image.id(),
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

        self.queue.write_buffer(
            &self.image_vertex_buffer,
            0,
            bytemuck::cast_slice(&image_vertices),
        );
        self.queue.write_buffer(
            &self.image_index_buffer,
            0,
            bytemuck::cast_slice(&image_indices),
        );

        self.scratch_image_vertices = image_vertices;
        self.scratch_image_indices = image_indices;
        // image_cmds is returned to the caller; it will NOT be returned to
        // scratch_image_cmds here. The caller must not hold it across calls.
        Ok(image_cmds)
    }

    /// Prepare text shaping and atlas uploads for the given text batch.
    /// After calling this, `encode_text_pass` can record the render pass.
    fn prepare_text_for_render(
        &mut self,
        layer_texts: &[&TextDraw],
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Result<(), String> {
        let mut font_system = self.font_system.lock().unwrap();
        let mut text_cache = self.text_cache.lock().unwrap();

        let mut text_keys: Vec<TextCacheKey> = Vec::with_capacity(layer_texts.len());

        for text_draw in layer_texts {
            if text_draw.text.is_empty()
                || text_draw.rect.width <= 0.0
                || text_draw.rect.height <= 0.0
            {
                continue;
            }

            let font_size_px = text_draw.font_size * text_draw.scale * root_scale;
            let style_hash =
                text_draw.text_style.measurement_hash() ^ text_draw.text.span_styles_hash();
            let line_height_px = crate::resolve_effective_line_height(
                &text_draw.text_style,
                &text_draw.text,
                font_size_px,
            );
            let key = TextCacheKey::for_node(text_draw.node_id, font_size_px, style_hash);

            let buffer = text_cache.entry(key.clone()).or_insert_with(|| {
                let buffer = glyphon::Buffer::new(
                    &mut font_system,
                    Metrics::new(font_size_px, line_height_px),
                );
                SharedTextBuffer {
                    buffer,
                    text: String::new(),
                    font_size: 0.0,
                    line_height: 0.0,
                    style_hash: 0,
                    cached_size: None,
                }
            });

            buffer.ensure(
                &mut font_system,
                EnsureTextBufferParams {
                    annotated_text: &text_draw.text,
                    font_size_px,
                    line_height_px,
                    style_hash,
                    style: &text_draw.text_style,
                    scale: text_draw.scale * root_scale,
                },
            );

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

            let cached = text_cache.get(key).expect("Text should be in cache");

            let color = GlyphonColor::rgba(
                (text_draw.color.r() * 255.0) as u8,
                (text_draw.color.g() * 255.0) as u8,
                (text_draw.color.b() * 255.0) as u8,
                (text_draw.color.a() * 255.0) as u8,
            );

            let left_px = text_draw.rect.x * root_scale;
            let top_px = text_draw.rect.y * root_scale;

            let Some(bounds) = text_bounds_for_clip(text_draw.clip, root_scale, width, height)
            else {
                continue;
            };

            text_areas.push(TextArea {
                buffer: &cached.buffer,
                left: left_px,
                top: top_px,
                scale: 1.0,
                bounds,
                default_color: color,
                custom_glyphs: &[],
            });
        }

        if text_areas.is_empty() {
            return Ok(());
        }

        self.text_viewport
            .update(&self.queue, Resolution { width, height });
        self.text_renderer
            .prepare(
                &self.device,
                &self.queue,
                &mut font_system,
                &mut self.text_atlas,
                &self.text_viewport,
                text_areas.iter().cloned(),
                &mut self.swash_cache,
            )
            .map_err(|e| format!("Text prepare error: {:?}", e))?;

        Ok(())
    }

    /// Record a text render pass onto the provided encoder.
    /// Must be called after `text_renderer.prepare()`.
    fn encode_text_pass(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        load_op: wgpu::LoadOp<wgpu::Color>,
    ) -> Result<(), String> {
        self.frame_stats.bump_text();
        {
            let mut text_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Text Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.text_renderer
                .render(&self.text_atlas, &self.text_viewport, &mut text_pass)
                .map_err(|e| format!("Effect text render error: {:?}", e))?;
        }
        Ok(())
    }
}

fn align_to(value: u32, alignment: u32) -> u32 {
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

    fn reset(&mut self) {
        *self = Self::default();
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

fn effect_layer_in_range(layer: &EffectLayer, z_start: usize, z_end: usize) -> bool {
    layer.z_start >= z_start && layer.z_start < z_end && layer.z_end <= z_end
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

fn has_backdrop_layer_in_range(
    backdrop_layers: &[BackdropLayer],
    z_start: usize,
    z_end: usize,
) -> bool {
    backdrop_layers
        .iter()
        .any(|layer| layer.z_index >= z_start && layer.z_index < z_end)
}

fn scene_end_z(
    shapes: &[DrawShape],
    images: &[ImageDraw],
    texts: &[TextDraw],
    shadow_draws: &[ShadowDraw],
    effect_layers: &[EffectLayer],
    backdrop_layers: &[BackdropLayer],
) -> usize {
    let mut end = 0usize;
    if let Some(shape) = shapes.last() {
        end = end.max(shape.z_index.saturating_add(1));
    }
    if let Some(image) = images.last() {
        end = end.max(image.z_index.saturating_add(1));
    }
    if let Some(text) = texts.last() {
        end = end.max(text.z_index.saturating_add(1));
    }
    if let Some(shadow) = shadow_draws.last() {
        end = end.max(shadow.z_index.saturating_add(1));
    }
    if let Some(layer) = effect_layers.iter().max_by_key(|layer| layer.z_end) {
        end = end.max(layer.z_end);
    }
    if let Some(layer) = backdrop_layers.iter().max_by_key(|layer| layer.z_index) {
        end = end.max(layer.z_index.saturating_add(1));
    }
    end
}

fn scissor_rect_for_rect(
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
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    let clipped = match clip {
        Some(clip_rect) => clip_rect.intersect(viewport)?,
        None => viewport,
    };

    let left = (clipped.x * root_scale).floor();
    let top = (clipped.y * root_scale).floor();
    let right = ((clipped.x + clipped.width) * root_scale).ceil();
    let bottom = ((clipped.y + clipped.height) * root_scale).ceil();

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
    use cranpose_ui_graphics::{Rect, RenderEffect, RoundedCornerShape};

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
        }
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
            image: ImageBitmap::from_rgba8(1, 1, vec![255, 255, 255, 255]).expect("image"),
            alpha: 1.0,
            color_filter: None,
            z_index,
            clip: None,
            blend_mode,
            src_rect: None,
        }
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
}
