//! GPU rendering implementation using WGPU

use crate::effect_renderer::{EffectRenderer, RoundedCompositeMask};
use crate::offscreen::OffscreenTarget;
use crate::scene::{BackdropLayer, DrawShape, EffectLayer, ImageDraw, ShadowDraw, TextDraw};
use crate::shaders;
use crate::{SharedTextBuffer, SharedTextCache, TextCacheKey};
use bytemuck::{Pod, Zeroable};
use cranpose_ui_graphics::{
    BlendMode, Brush, Color, ColorFilter, ImageBitmap, Rect, RenderEffect, TileMode,
};
use glyphon::{
    Attrs, Cache, Color as GlyphonColor, FontSystem, Metrics, Resolution, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport,
};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

// Chunked rendering constants for robustness with large scenes
// Note: Limited to 256 for WebGL compatibility (uniform buffer size limit)
// WebGL guarantees 16KB uniform buffers, ShapeData is 64 bytes = 256 max shapes
const MAX_SHAPES_PER_DRAW: usize = 200; // ShapeData is 80 bytes, 16KB uniform limit = ~200 shapes
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
    _padding: [f32; 2],
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
    image_texture_cache: HashMap<u64, CachedImageTexture>,
    // Shared text cache used by both measurement and rendering
    text_cache: SharedTextCache,
    text_viewport: Viewport,
    scratch_shape_data: Vec<ShapeData>,
    scratch_gradients: Vec<GradientStop>,
    scratch_filtered_indices: Vec<usize>,
    scratch_vertices: Vec<Vertex>,
    scratch_indices: Vec<u32>,
    scratch_text_entries: Vec<(usize, TextCacheKey)>,
    last_shape_chunk_count: usize,
    effect_renderer: EffectRenderer,
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
            image_texture_cache: HashMap::new(),
            text_cache,
            text_viewport,
            scratch_shape_data: Vec::new(),
            scratch_gradients: Vec::new(),
            scratch_filtered_indices: Vec::new(),
            scratch_vertices: Vec::new(),
            scratch_indices: Vec::new(),
            scratch_text_entries: Vec::new(),
            last_shape_chunk_count: 0,
            effect_renderer,
        }
    }

    fn ensure_image_cached(&mut self, image: &ImageBitmap) -> Result<(), String> {
        if self.image_texture_cache.contains_key(&image.id()) {
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

        // Keep cache bounded to avoid unbounded GPU memory growth in long sessions.
        if self.image_texture_cache.len() >= MAX_TEXTURE_CACHE_ITEMS {
            let remove_count = self.image_texture_cache.len() - (MAX_TEXTURE_CACHE_ITEMS / 2) + 1;
            let keys: Vec<u64> = self
                .image_texture_cache
                .keys()
                .take(remove_count)
                .copied()
                .collect();
            for key in keys {
                self.image_texture_cache.remove(&key);
            }
        }

        self.image_texture_cache
            .insert(image.id(), CachedImageTexture { bind_group });
        Ok(())
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

        let has_non_src_over_blend = shapes
            .iter()
            .any(|shape| supported_blend_mode(shape.blend_mode) != BlendMode::SrcOver)
            || images
                .iter()
                .any(|image| supported_blend_mode(image.blend_mode) != BlendMode::SrcOver);

        if has_non_src_over_blend
            || !shadow_draws.is_empty()
            || !effect_layers.is_empty()
            || !backdrop_layers.is_empty()
        {
            return self.render_with_layer_events(
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
        }

        // Build z-index exclusion ranges from effect layers.
        // Items in these ranges are rendered offscreen with effects applied.
        // Items with z >= max_effect_z_end are rendered AFTER effect compositing
        // so they appear on top of the composited effect result.
        let effect_z_ranges: Vec<Range<usize>> = effect_layers
            .iter()
            .map(|layer| layer.z_start..layer.z_end)
            .collect();
        let max_effect_z_end = effect_layers.iter().map(|l| l.z_end).max().unwrap_or(0);

        // Update uniform buffer with viewport dimensions
        let uniforms = Uniforms {
            viewport: [width as f32, height as f32],
            _padding: [0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        // Chunked rendering for robustness with large scenes
        let total_shape_count = shapes.len();

        if total_shape_count > MAX_SHAPES_PER_DRAW {
            let chunk_count = total_shape_count.div_ceil(MAX_SHAPES_PER_DRAW);
            if self.last_shape_chunk_count != chunk_count {
                log::debug!(
                    "Rendering {} shapes in {} chunks (max {} per draw)",
                    total_shape_count,
                    chunk_count,
                    MAX_SHAPES_PER_DRAW
                );
                self.last_shape_chunk_count = chunk_count;
            }
        } else if self.last_shape_chunk_count != 0 {
            self.last_shape_chunk_count = 0;
        }

        // First pass: collect all shape data and gradients across entire scene
        // Also collect filtered shapes (ones that pass clip test) to stay in sync
        self.scratch_gradients.clear();
        self.scratch_shape_data.clear();
        self.scratch_filtered_indices.clear();
        self.scratch_gradients.reserve(total_shape_count);
        self.scratch_shape_data.reserve(total_shape_count);
        self.scratch_filtered_indices.reserve(total_shape_count);

        for (shape_index, shape) in shapes.iter().enumerate() {
            // Skip shapes that belong to an effect layer (will be rendered offscreen)
            if effect_z_ranges
                .iter()
                .any(|range| range.contains(&shape.z_index))
            {
                continue;
            }
            // Skip shapes after effects — they render in a later pass on top
            if max_effect_z_end > 0 && shape.z_index >= max_effect_z_end {
                continue;
            }

            let rect = shape.rect;
            let local_rect = shape.local_rect;

            // Scale to physical pixels
            let x = rect.x * root_scale;
            let y = rect.y * root_scale;
            let w = rect.width * root_scale;
            let h = rect.height * root_scale;

            // Calculate clip rect (scaled to physical pixels) and skip early if fully clipped
            let clip_rect = if let Some(clip) = shape.clip {
                let clip_right = (clip.x + clip.width) * root_scale;
                let clip_bottom = (clip.y + clip.height) * root_scale;
                let shape_right = x + w;
                let shape_bottom = y + h;

                // Skip shapes that are entirely outside the clip rect
                if shape_right <= clip.x * root_scale
                    || x >= clip_right
                    || shape_bottom <= clip.y * root_scale
                    || y >= clip_bottom
                {
                    continue;
                }

                [
                    clip.x * root_scale,
                    clip.y * root_scale,
                    clip.width * root_scale,
                    clip.height * root_scale,
                ]
            } else {
                [0.0, 0.0, 0.0, 0.0] // No clipping
            };

            // Determine gradient parameters and collect stops
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

            // Shape data (radii scaled to physical pixels)
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

            self.scratch_filtered_indices.push(shape_index);
        }

        // Ensure buffers can hold at least one chunk
        self.shape_buffers.ensure_capacity(
            &self.device,
            &self.shape_bind_group_layout,
            MAX_SHAPES_PER_DRAW * 4,             // vertices
            MAX_SHAPES_PER_DRAW * 6,             // indices
            MAX_SHAPES_PER_DRAW,                 // shapes
            self.scratch_gradients.len().max(1), // all gradients (written once)
        );

        // Write gradients once for all chunks
        if !self.scratch_gradients.is_empty() {
            self.queue.write_buffer(
                &self.shape_buffers.gradient_buffer,
                0,
                bytemuck::cast_slice(&self.scratch_gradients),
            );
        }

        // Second pass: render shapes in chunks with proper synchronization
        // Each chunk gets its own encoder+submit to ensure buffer writes complete before next chunk
        // Use filtered indices (after clip culling) to stay in sync with shape data
        let filtered_shape_count = self.scratch_filtered_indices.len();
        let chunk_count = filtered_shape_count.div_ceil(MAX_SHAPES_PER_DRAW);
        let mut has_shape_pass = false;
        let mut pending_shape_encoder: Option<wgpu::CommandEncoder> = None;

        for (chunk_idx, chunk) in self
            .scratch_filtered_indices
            .chunks(MAX_SHAPES_PER_DRAW)
            .enumerate()
        {
            let chunk_len = chunk.len();
            let chunk_start = chunk_idx * MAX_SHAPES_PER_DRAW;

            self.scratch_vertices.clear();
            self.scratch_indices.clear();
            self.scratch_vertices.reserve(chunk_len * 4);
            self.scratch_indices.reserve(chunk_len * 6);

            // Build vertices and indices for this chunk
            for (shape_idx, shape_index) in chunk.iter().enumerate() {
                let shape = &shapes[*shape_index];
                let base_vertex = (shape_idx * 4) as u32;

                // Get color from brush for vertex data
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

                // Vertices for quad (in physical pixels)
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

                // Indices for two triangles
                self.scratch_indices.extend_from_slice(&[
                    base_vertex,
                    base_vertex + 1,
                    base_vertex + 2,
                    base_vertex + 2,
                    base_vertex + 1,
                    base_vertex + 3,
                ]);
            }

            // Get shape data slice for this chunk
            let chunk_shape_data = &self.scratch_shape_data[chunk_start..chunk_start + chunk_len];

            // Write chunk data and render in one encoder (submit after to ensure synchronization)
            if !self.scratch_vertices.is_empty() {
                // Write this chunk's data to buffers
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
                    bytemuck::cast_slice(chunk_shape_data),
                );

                // Create encoder for this chunk
                has_shape_pass = true;
                let mut encoder =
                    self.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("Shape Chunk Encoder"),
                        });

                // Create render pass for this chunk (Clear on first chunk, Load on subsequent)
                {
                    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("Shape Render Pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: if chunk_idx == 0 {
                                    wgpu::LoadOp::Clear(CLEAR_COLOR)
                                } else {
                                    wgpu::LoadOp::Load // Preserve previous chunks
                                },
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });

                    render_pass.set_pipeline(&self.pipeline);
                    render_pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                    render_pass.set_bind_group(1, &self.shape_buffers.bind_group, &[]);

                    // Draw this chunk
                    render_pass.set_vertex_buffer(0, self.shape_buffers.vertex_buffer.slice(..));
                    render_pass.set_index_buffer(
                        self.shape_buffers.index_buffer.slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    render_pass.draw_indexed(0..(chunk_len as u32 * 6), 0, 0..1);
                }

                let is_last_chunk = chunk_idx + 1 == chunk_count;
                if is_last_chunk {
                    pending_shape_encoder = Some(encoder);
                } else {
                    // Submit this chunk immediately to ensure synchronization before next chunk.
                    self.queue.submit(std::iter::once(encoder.finish()));
                }
            }
        }

        let mut has_image_pass = false;
        if !images.is_empty() {
            for image_draw in images {
                self.ensure_image_cached(&image_draw.image)?;
            }

            // Pre-compute all image vertices and per-image draw metadata before
            // starting the render pass. queue.write_buffer is staged and only
            // the last write to a given offset survives until submission, so we
            // must write ALL vertices at distinct offsets before encoding any
            // draw commands.
            //
            // We use absolute vertex indices in the index buffer (instead of
            // base_vertex in draw_indexed) because WebGL2 does not support
            // draw_elements_instanced_base_vertex.
            struct ImageDrawCmd {
                index_start: u32,
                scissor: (u32, u32, u32, u32),
                image_id: u64,
            }
            let mut image_vertices: Vec<Vertex> = Vec::with_capacity(images.len() * 4);
            let mut image_indices: Vec<u32> = Vec::with_capacity(images.len() * 6);
            let mut image_cmds: Vec<ImageDrawCmd> = Vec::with_capacity(images.len());

            for image_draw in images {
                // Skip images that belong to an effect layer
                if effect_z_ranges
                    .iter()
                    .any(|range| range.contains(&image_draw.z_index))
                {
                    continue;
                }
                // Skip images after effects — they render in a later pass on top
                if max_effect_z_end > 0 && image_draw.z_index >= max_effect_z_end {
                    continue;
                }

                let rect = image_draw.rect;
                if rect.width <= 0.0 || rect.height <= 0.0 || image_draw.alpha <= 0.0 {
                    continue;
                }

                let tint = tint_for_image(image_draw.color_filter, image_draw.alpha);
                if tint[3] <= 0.0 {
                    continue;
                }

                let scissor = scissor_rect_for_image(image_draw, root_scale, width, height);
                let Some(scissor) = scissor else {
                    continue;
                };

                // Compute UV coordinates: sub-region or full image
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
                    image_id: image_draw.image.id(),
                });
            }

            if !image_cmds.is_empty() {
                // Resize vertex buffer if needed
                let needed_bytes = (image_vertices.len() * std::mem::size_of::<Vertex>()) as u64;
                if needed_bytes > self.image_vertex_buffer.size() {
                    self.image_vertex_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("Image Vertex Buffer"),
                        size: needed_bytes,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                }

                // Resize index buffer if needed
                let needed_index_bytes = (image_indices.len() * std::mem::size_of::<u32>()) as u64;
                if needed_index_bytes > self.image_index_buffer.size() {
                    self.image_index_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("Image Index Buffer"),
                        size: needed_index_bytes,
                        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                }

                // Write ALL vertices and indices in one call each before
                // encoding any draw commands
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

                let mut image_encoder = pending_shape_encoder.take().unwrap_or_else(|| {
                    self.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("Image Encoder"),
                        })
                });

                {
                    let mut render_pass =
                        image_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("Image Render Pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: if has_shape_pass {
                                        wgpu::LoadOp::Load
                                    } else {
                                        wgpu::LoadOp::Clear(CLEAR_COLOR)
                                    },
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });

                    render_pass.set_pipeline(&self.image_pipeline);
                    render_pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                    render_pass.set_index_buffer(
                        self.image_index_buffer.slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    render_pass.set_vertex_buffer(0, self.image_vertex_buffer.slice(..));

                    for cmd in &image_cmds {
                        let (sx, sy, sw, sh) = cmd.scissor;
                        render_pass.set_scissor_rect(sx, sy, sw, sh);

                        let cached = self
                            .image_texture_cache
                            .get(&cmd.image_id)
                            .ok_or_else(|| "image texture missing from cache".to_string())?;
                        render_pass.set_bind_group(1, &cached.bind_group, &[]);
                        render_pass.draw_indexed(cmd.index_start..(cmd.index_start + 6), 0, 0..1);
                    }
                    has_image_pass = true;
                }

                pending_shape_encoder = Some(image_encoder);
            }
        }

        // Prepare text rendering - create buffers and text areas (with caching)
        let mut font_system = self.font_system.lock().unwrap();
        let mut text_cache = self.text_cache.lock().unwrap();

        // Prepare text buffers (with caching for performance)
        // Font size in physical pixels for glyphon
        self.scratch_text_entries.clear();
        self.scratch_text_entries.reserve(texts.len());

        for (text_index, text_draw) in texts.iter().enumerate() {
            // Skip texts that belong to an effect layer
            if effect_z_ranges
                .iter()
                .any(|range| range.contains(&text_draw.z_index))
            {
                continue;
            }
            // Skip texts after effects — they render in a later pass on top
            if max_effect_z_end > 0 && text_draw.z_index >= max_effect_z_end {
                continue;
            }

            // Skip empty text or zero-sized rects
            if text_draw.text.is_empty()
                || text_draw.rect.width <= 0.0
                || text_draw.rect.height <= 0.0
            {
                continue;
            }

            // Scale font size to physical pixels: font_size is in dp, scale by text zoom and DPI
            let font_size_px = text_draw.font_size * text_draw.scale * root_scale;
            let key = TextCacheKey::for_node(text_draw.node_id, font_size_px);

            // Create or update buffer in cache
            let buffer = text_cache.entry(key.clone()).or_insert_with(|| {
                let buffer = glyphon::Buffer::new(
                    &mut font_system,
                    Metrics::new(font_size_px, font_size_px * 1.4),
                );
                SharedTextBuffer {
                    buffer,
                    text: String::new(),
                    font_size: 0.0,
                    cached_size: None,
                }
            });

            // Ensure buffer has the correct text
            buffer.ensure(
                &mut font_system,
                text_draw.text.as_ref(),
                font_size_px,
                Attrs::new(),
            );

            self.scratch_text_entries.push((text_index, key));
        }

        // Create text areas using cached buffers
        let mut text_areas = Vec::with_capacity(self.scratch_text_entries.len());

        for (text_index, key) in self.scratch_text_entries.iter() {
            let text_draw = &texts[*text_index];
            let cached = text_cache.get(key).expect("Text should be in cache");

            let color = GlyphonColor::rgba(
                (text_draw.color.r() * 255.0) as u8,
                (text_draw.color.g() * 255.0) as u8,
                (text_draw.color.b() * 255.0) as u8,
                (text_draw.color.a() * 255.0) as u8,
            );

            // Scale text position and bounds to physical pixels
            let left_px = text_draw.rect.x * root_scale;
            let top_px = text_draw.rect.y * root_scale;

            let bounds = TextBounds {
                left: text_draw
                    .clip
                    .map(|c| (c.x * root_scale) as i32)
                    .unwrap_or(0),
                top: text_draw
                    .clip
                    .map(|c| (c.y * root_scale) as i32)
                    .unwrap_or(0),
                right: text_draw
                    .clip
                    .map(|c| ((c.x + c.width) * root_scale) as i32)
                    .unwrap_or(width as i32),
                bottom: text_draw
                    .clip
                    .map(|c| ((c.y + c.height) * root_scale) as i32)
                    .unwrap_or(height as i32),
            };

            text_areas.push(TextArea {
                buffer: &cached.buffer,
                left: left_px,
                top: top_px,
                // Use scale 1.0 since font_size and position are already in physical pixels
                scale: 1.0,
                bounds,
                default_color: color,
                custom_glyphs: &[],
            });
        }

        let has_text = !text_areas.is_empty();

        // Prepare all text at once
        if has_text {
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

            self.text_atlas.trim();
        }

        drop(font_system);
        drop(text_cache);

        let mut submitted = false;
        if has_text {
            let mut text_encoder = pending_shape_encoder.take().unwrap_or_else(|| {
                self.device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("Text Encoder"),
                    })
            });

            {
                let mut text_pass = text_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Text Render Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: if has_shape_pass || has_image_pass {
                                wgpu::LoadOp::Load
                            } else {
                                wgpu::LoadOp::Clear(CLEAR_COLOR)
                            },
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });

                self.text_renderer
                    .render(&self.text_atlas, &self.text_viewport, &mut text_pass)
                    .map_err(|e| format!("Text render error: {:?}", e))?;
            }

            self.queue.submit(std::iter::once(text_encoder.finish()));
            submitted = true;
        }

        if !submitted {
            if let Some(shape_encoder) = pending_shape_encoder.take() {
                self.queue.submit(std::iter::once(shape_encoder.finish()));
            } else if !has_shape_pass && !has_image_pass {
                let mut clear_encoder =
                    self.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("Clear Encoder"),
                        });
                {
                    let _clear_pass =
                        clear_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("Clear Render Pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(CLEAR_COLOR),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                }
                self.queue.submit(std::iter::once(clear_encoder.finish()));
            }
        }

        if !self.scratch_text_entries.is_empty() {
            let mut text_cache = self.text_cache.lock().unwrap();
            crate::trim_text_cache(&mut text_cache);
        }

        // === Effect layer processing ===
        // For each effect layer, render its items to an offscreen target,
        // apply the effect, and composite the result onto the surface.
        if !effect_layers.is_empty() {
            self.render_effect_layers(
                view,
                shapes,
                images,
                texts,
                effect_layers,
                width,
                height,
                root_scale,
            )?;
        }

        Ok(())
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

        // Double-buffer path: accumulate everything into an intermediate texture.
        let accum = self
            .effect_renderer
            .offscreen_pool
            .acquire(&self.device, width, height);
        self.clear_target_view(&accum.view, CLEAR_COLOR);

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
        )?;

        self.effect_renderer.composite_to_view(
            &self.device,
            &self.queue,
            &accum,
            surface_view,
            wgpu::LoadOp::Clear(CLEAR_COLOR),
        );
        self.effect_renderer.offscreen_pool.release(accum);

        let mut text_cache = self.text_cache.lock().unwrap();
        crate::trim_text_cache(&mut text_cache);

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
    ) -> Result<(), String> {
        if z_start >= z_end {
            return Ok(());
        }

        let effect_z_ranges =
            collect_effect_ranges(effect_layers, z_start, z_end, excluded_effect_layer);
        let events = collect_layer_events(
            effect_layers,
            backdrop_layers,
            z_start,
            z_end,
            excluded_effect_layer,
        );

        let mut cursor_z = z_start;
        for event in events {
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
                )?;
                cursor_z = event.z_index;
            } else if event.z_index < cursor_z {
                // Already consumed by a previously composited effect range.
                continue;
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
            )?;
        }

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
    ) -> Result<(), String> {
        let ordered_items = collect_non_effect_segment_items(
            shapes,
            images,
            texts,
            shadow_draws,
            z_start,
            z_end,
            effect_z_ranges,
        );
        if ordered_items.is_empty() {
            return Ok(());
        }

        let mut cursor = 0usize;
        while cursor < ordered_items.len() {
            match ordered_items[cursor] {
                SegmentDrawItem::Shape(index) => {
                    let blend_mode = supported_blend_mode(shapes[index].blend_mode);
                    let start = cursor;
                    cursor += 1;
                    while cursor < ordered_items.len() {
                        match ordered_items[cursor] {
                            SegmentDrawItem::Shape(next_index)
                                if supported_blend_mode(shapes[next_index].blend_mode)
                                    == blend_mode =>
                            {
                                cursor += 1;
                            }
                            _ => break,
                        }
                    }

                    let shape_batch: Vec<&DrawShape> = ordered_items[start..cursor]
                        .iter()
                        .map(|item| match item {
                            SegmentDrawItem::Shape(shape_index) => &shapes[*shape_index],
                            _ => unreachable!("shape batch contains only shape items"),
                        })
                        .collect();
                    self.render_shapes_to_offscreen(
                        target_view,
                        &shape_batch,
                        blend_mode,
                        width,
                        height,
                        root_scale,
                        true,
                    );
                }
                SegmentDrawItem::Image(index) => {
                    let blend_mode = supported_blend_mode(images[index].blend_mode);
                    let start = cursor;
                    cursor += 1;
                    while cursor < ordered_items.len() {
                        match ordered_items[cursor] {
                            SegmentDrawItem::Image(next_index)
                                if supported_blend_mode(images[next_index].blend_mode)
                                    == blend_mode =>
                            {
                                cursor += 1;
                            }
                            _ => break,
                        }
                    }

                    let image_batch: Vec<&ImageDraw> = ordered_items[start..cursor]
                        .iter()
                        .map(|item| match item {
                            SegmentDrawItem::Image(image_index) => &images[*image_index],
                            _ => unreachable!("image batch contains only image items"),
                        })
                        .collect();
                    self.render_images_to_offscreen(
                        target_view,
                        &image_batch,
                        blend_mode,
                        width,
                        height,
                        root_scale,
                        true,
                    )?;
                }
                SegmentDrawItem::Text(_) => {
                    let start = cursor;
                    cursor += 1;
                    while cursor < ordered_items.len() {
                        match ordered_items[cursor] {
                            SegmentDrawItem::Text(_) => {
                                cursor += 1;
                            }
                            _ => break,
                        }
                    }

                    let text_batch: Vec<&TextDraw> = ordered_items[start..cursor]
                        .iter()
                        .map(|item| match item {
                            SegmentDrawItem::Text(text_index) => &texts[*text_index],
                            _ => unreachable!("text batch contains only text items"),
                        })
                        .collect();
                    self.render_text_to_offscreen(
                        target_view,
                        &text_batch,
                        width,
                        height,
                        root_scale,
                        true,
                    )?;
                }
                SegmentDrawItem::Shadow(index) => {
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
        if shadow.shapes.is_empty() {
            return;
        }

        let shape_bounds = shadow
            .shapes
            .iter()
            .map(|(shape, _)| shape.rect)
            .reduce(|a, b| Rect {
                x: a.x.min(b.x),
                y: a.y.min(b.y),
                width: (a.x + a.width).max(b.x + b.width) - a.x.min(b.x),
                height: (a.y + a.height).max(b.y + b.height) - a.y.min(b.y),
            });
        let Some(shape_bounds) = shape_bounds else {
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
            let Some(intersection) = intersect_rect(blur_bounds, clip_expanded) else {
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
                    true,
                );
            }
            return;
        }

        // 1. Acquire offscreen source (full viewport for coordinate alignment).
        let source = self
            .effect_renderer
            .offscreen_pool
            .acquire(&self.device, width, height);
        self.clear_target_view(&source.view, wgpu::Color::TRANSPARENT);

        // 2. Render shadow shapes to offscreen, preserving per-shape blend modes.
        for (shape, blend_mode) in &shadow.shapes {
            self.render_shapes_to_offscreen(
                &source.view,
                &[shape],
                *blend_mode,
                width,
                height,
                root_scale,
                true,
            );
        }

        // 3. Apply Gaussian blur.
        let dest = self
            .effect_renderer
            .offscreen_pool
            .acquire(&self.device, width, height);
        let pixel_radius = shadow.blur_radius * root_scale;
        self.effect_renderer.apply_blur_scissored(
            &self.device,
            &self.queue,
            &source,
            &dest.view,
            pixel_radius,
            pixel_radius,
            TileMode::Decal,
            processing_scissor,
        );
        self.effect_renderer.offscreen_pool.release(source);

        // 4. Composite blurred result onto target with optional clip.
        let clip_scissor = shadow
            .clip
            .and_then(|clip| scissor_rect_for_rect(clip, root_scale, width, height));
        let scissor = clip_scissor.or(processing_scissor);
        let rounded_mask = inner_shadow_composite_mask(shadow, root_scale);
        self.effect_renderer
            .composite_to_view_scissored_with_alpha_and_mask(
                &self.device,
                &self.queue,
                &dest,
                target_view,
                1.0,
                wgpu::LoadOp::Load,
                scissor,
                rounded_mask,
            );
        self.effect_renderer.offscreen_pool.release(dest);
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

        let source = self
            .effect_renderer
            .offscreen_pool
            .acquire(&self.device, width, height);
        self.clear_target_view(&source.view, wgpu::Color::TRANSPARENT);

        // Nested backdrop layers inside this effect-isolated subtree should still
        // be able to sample the true scene content behind the subtree.
        let has_nested_backdrop =
            has_backdrop_layer_in_range(backdrop_layers, layer.z_start, layer.z_end);
        let layer_underlay = if has_nested_backdrop {
            let underlay = self
                .effect_renderer
                .offscreen_pool
                .acquire(&self.device, width, height);

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
        );

        if let Some(underlay) = layer_underlay {
            self.effect_renderer.offscreen_pool.release(underlay);
        }

        render_result?;

        let dest = self
            .effect_renderer
            .offscreen_pool
            .acquire(&self.device, width, height);

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

        self.effect_renderer.composite_to_view_scissored_with_alpha(
            &self.device,
            &self.queue,
            &dest,
            &target.view,
            layer.composite_alpha,
            wgpu::LoadOp::Load,
            Some(scissor),
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

        let snapshot = self
            .effect_renderer
            .offscreen_pool
            .acquire(&self.device, width, height);
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

        let dest = self
            .effect_renderer
            .offscreen_pool
            .acquire(&self.device, width, height);
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

    fn clear_target_view(&self, target_view: &wgpu::TextureView, color: wgpu::Color) {
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
                        load: wgpu::LoadOp::Clear(color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        self.queue.submit(std::iter::once(encoder.finish()));
    }

    /// Render effect layers: for each layer, render its shapes, images, and text
    /// to an offscreen target, apply the effect, then composite onto the surface.
    #[allow(clippy::too_many_arguments)]
    fn render_effect_layers(
        &mut self,
        surface_view: &wgpu::TextureView,
        shapes: &[DrawShape],
        images: &[ImageDraw],
        texts: &[TextDraw],
        effect_layers: &[EffectLayer],
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Result<(), String> {
        for layer in effect_layers {
            let z_range = layer.z_start..layer.z_end;

            // Collect items in this effect layer's z-range
            let layer_shapes: Vec<&DrawShape> = shapes
                .iter()
                .filter(|s| z_range.contains(&s.z_index))
                .collect();
            let layer_images: Vec<&ImageDraw> = images
                .iter()
                .filter(|i| z_range.contains(&i.z_index))
                .collect();
            let layer_texts: Vec<&TextDraw> = texts
                .iter()
                .filter(|t| z_range.contains(&t.z_index))
                .collect();

            if layer_shapes.is_empty() && layer_images.is_empty() && layer_texts.is_empty() {
                continue;
            }

            // Acquire offscreen target (full viewport size for coordinate compatibility)
            let source = self
                .effect_renderer
                .offscreen_pool
                .acquire(&self.device, width, height);

            // Render all content types to offscreen target.
            // Shapes first (with Clear), then images and text (with Load).
            let mut has_content = false;

            if !layer_shapes.is_empty() {
                self.render_shapes_to_offscreen(
                    &source.view,
                    &layer_shapes,
                    BlendMode::SrcOver,
                    width,
                    height,
                    root_scale,
                    false,
                );
                has_content = true;
            }

            if !layer_images.is_empty() {
                self.render_images_to_offscreen(
                    &source.view,
                    &layer_images,
                    BlendMode::SrcOver,
                    width,
                    height,
                    root_scale,
                    has_content,
                )?;
                has_content = true;
            }

            if !layer_texts.is_empty() {
                self.render_text_to_offscreen(
                    &source.view,
                    &layer_texts,
                    width,
                    height,
                    root_scale,
                    has_content,
                )?;
            }

            // Acquire destination for effect output
            let dest = self
                .effect_renderer
                .offscreen_pool
                .acquire(&self.device, width, height);

            // Compute effect layer pixel rect for shader coordinate mapping
            let layer_pixel_rect = [
                layer.rect.x * root_scale,
                layer.rect.y * root_scale,
                layer.rect.width * root_scale,
                layer.rect.height * root_scale,
            ];

            // Apply the effect (if present): source → dest
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

            // Composite the effected result onto the surface with alpha blending
            self.effect_renderer.composite_to_view_with_alpha(
                &self.device,
                &self.queue,
                &dest,
                surface_view,
                layer.composite_alpha,
                wgpu::LoadOp::Load,
            );

            // Return offscreen targets to pool
            self.effect_renderer.offscreen_pool.release(source);
            self.effect_renderer.offscreen_pool.release(dest);
        }

        // Render items that come AFTER effect layers in z-order.
        // These were excluded from the main render pass so they draw on top
        // of the composited effect result.
        let max_z_end = effect_layers.iter().map(|l| l.z_end).max().unwrap_or(0);

        let after_shapes: Vec<&DrawShape> =
            shapes.iter().filter(|s| s.z_index >= max_z_end).collect();
        let after_images: Vec<&ImageDraw> =
            images.iter().filter(|i| i.z_index >= max_z_end).collect();
        let after_texts: Vec<&TextDraw> = texts.iter().filter(|t| t.z_index >= max_z_end).collect();

        if !after_shapes.is_empty() {
            self.render_shapes_to_offscreen(
                surface_view,
                &after_shapes,
                BlendMode::SrcOver,
                width,
                height,
                root_scale,
                true,
            );
        }
        if !after_images.is_empty() {
            self.render_images_to_offscreen(
                surface_view,
                &after_images,
                BlendMode::SrcOver,
                width,
                height,
                root_scale,
                true,
            )?;
        }
        if !after_texts.is_empty() {
            self.render_text_to_offscreen(
                surface_view,
                &after_texts,
                width,
                height,
                root_scale,
                true,
            )?;
        }

        Ok(())
    }

    /// Render a subset of shapes to a target view.
    ///
    /// Uses the same shape pipeline and uniforms as the main render path.
    /// When `has_prior_content` is false, clears to transparent first;
    /// when true, preserves existing content (LoadOp::Load).
    #[allow(clippy::too_many_arguments)]
    fn render_shapes_to_offscreen(
        &mut self,
        target_view: &wgpu::TextureView,
        layer_shapes: &[&DrawShape],
        blend_mode: BlendMode,
        width: u32,
        height: u32,
        root_scale: f32,
        has_prior_content: bool,
    ) {
        if layer_shapes.is_empty() {
            return;
        }

        // Update viewport uniforms (should already be set, but ensure correctness)
        let uniforms = Uniforms {
            viewport: [width as f32, height as f32],
            _padding: [0.0, 0.0],
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

        // Create render pass targeting the view
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Effect Layer Shape Encoder"),
            });
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Effect Layer Shape Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if has_prior_content {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                        },
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
        self.queue.submit(std::iter::once(encoder.finish()));
    }

    /// Render a subset of images to an offscreen target view.
    #[allow(clippy::too_many_arguments)]
    fn render_images_to_offscreen(
        &mut self,
        target_view: &wgpu::TextureView,
        layer_images: &[&ImageDraw],
        blend_mode: BlendMode,
        width: u32,
        height: u32,
        root_scale: f32,
        has_prior_content: bool,
    ) -> Result<(), String> {
        // Ensure all images are cached
        for image_draw in layer_images {
            self.ensure_image_cached(&image_draw.image)?;
        }

        // Build vertices and draw commands (same batching approach as main render)
        struct ImageDrawCmd {
            index_start: u32,
            scissor: (u32, u32, u32, u32),
            image_id: u64,
        }
        let mut image_vertices: Vec<Vertex> = Vec::with_capacity(layer_images.len() * 4);
        let mut image_indices: Vec<u32> = Vec::with_capacity(layer_images.len() * 6);
        let mut image_cmds: Vec<ImageDrawCmd> = Vec::with_capacity(layer_images.len());

        for image_draw in layer_images {
            let rect = image_draw.rect;
            if rect.width <= 0.0 || rect.height <= 0.0 || image_draw.alpha <= 0.0 {
                continue;
            }

            let tint = tint_for_image(image_draw.color_filter, image_draw.alpha);
            if tint[3] <= 0.0 {
                continue;
            }

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
                image_id: image_draw.image.id(),
            });
        }

        if image_cmds.is_empty() {
            return Ok(());
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

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Effect Layer Image Encoder"),
            });
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Effect Layer Image Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if has_prior_content {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                        },
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

            for cmd in &image_cmds {
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
        self.queue.submit(std::iter::once(encoder.finish()));

        Ok(())
    }

    /// Render a subset of text to an offscreen target view.
    #[allow(clippy::too_many_arguments)]
    fn render_text_to_offscreen(
        &mut self,
        target_view: &wgpu::TextureView,
        layer_texts: &[&TextDraw],
        width: u32,
        height: u32,
        root_scale: f32,
        has_prior_content: bool,
    ) -> Result<(), String> {
        let mut font_system = self.font_system.lock().unwrap();
        let mut text_cache = self.text_cache.lock().unwrap();

        // Prepare text buffers for this subset
        let mut text_keys: Vec<TextCacheKey> = Vec::with_capacity(layer_texts.len());

        for text_draw in layer_texts {
            if text_draw.text.is_empty()
                || text_draw.rect.width <= 0.0
                || text_draw.rect.height <= 0.0
            {
                continue;
            }

            let font_size_px = text_draw.font_size * text_draw.scale * root_scale;
            let key = TextCacheKey::for_node(text_draw.node_id, font_size_px);

            let buffer = text_cache.entry(key.clone()).or_insert_with(|| {
                let buffer = glyphon::Buffer::new(
                    &mut font_system,
                    Metrics::new(font_size_px, font_size_px * 1.4),
                );
                SharedTextBuffer {
                    buffer,
                    text: String::new(),
                    font_size: 0.0,
                    cached_size: None,
                }
            });

            buffer.ensure(
                &mut font_system,
                text_draw.text.as_ref(),
                font_size_px,
                Attrs::new(),
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

            let bounds = TextBounds {
                left: text_draw
                    .clip
                    .map(|c| (c.x * root_scale) as i32)
                    .unwrap_or(0),
                top: text_draw
                    .clip
                    .map(|c| (c.y * root_scale) as i32)
                    .unwrap_or(0),
                right: text_draw
                    .clip
                    .map(|c| ((c.x + c.width) * root_scale) as i32)
                    .unwrap_or(width as i32),
                bottom: text_draw
                    .clip
                    .map(|c| ((c.y + c.height) * root_scale) as i32)
                    .unwrap_or(height as i32),
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

        // Prepare and render text to offscreen target
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
            .map_err(|e| format!("Effect text prepare error: {:?}", e))?;

        drop(font_system);
        drop(text_cache);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Effect Layer Text Encoder"),
            });
        {
            let mut text_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Effect Layer Text Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if has_prior_content {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                        },
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
        self.queue.submit(std::iter::once(encoder.finish()));

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

fn collect_non_effect_segment_items(
    shapes: &[DrawShape],
    images: &[ImageDraw],
    texts: &[TextDraw],
    shadow_draws: &[ShadowDraw],
    z_start: usize,
    z_end: usize,
    effect_z_ranges: &[Range<usize>],
) -> Vec<SegmentDrawItem> {
    let mut ordered_items =
        Vec::with_capacity(shapes.len() + images.len() + texts.len() + shadow_draws.len());

    for (index, shape) in shapes.iter().enumerate() {
        if shape.z_index >= z_start
            && shape.z_index < z_end
            && !is_in_effect_range(shape.z_index, effect_z_ranges)
        {
            ordered_items.push((shape.z_index, SegmentDrawItem::Shape(index)));
        }
    }

    for (index, image) in images.iter().enumerate() {
        if image.z_index >= z_start
            && image.z_index < z_end
            && !is_in_effect_range(image.z_index, effect_z_ranges)
        {
            ordered_items.push((image.z_index, SegmentDrawItem::Image(index)));
        }
    }

    for (index, text) in texts.iter().enumerate() {
        if text.z_index >= z_start
            && text.z_index < z_end
            && !is_in_effect_range(text.z_index, effect_z_ranges)
        {
            ordered_items.push((text.z_index, SegmentDrawItem::Text(index)));
        }
    }

    for (index, shadow) in shadow_draws.iter().enumerate() {
        if shadow.z_index >= z_start
            && shadow.z_index < z_end
            && !is_in_effect_range(shadow.z_index, effect_z_ranges)
        {
            ordered_items.push((shadow.z_index, SegmentDrawItem::Shadow(index)));
        }
    }

    ordered_items.sort_by_key(|(z_index, _)| *z_index);
    ordered_items.into_iter().map(|(_, item)| item).collect()
}

fn effect_layer_in_range(layer: &EffectLayer, z_start: usize, z_end: usize) -> bool {
    layer.z_start >= z_start && layer.z_start < z_end && layer.z_end <= z_end
}

fn collect_effect_ranges(
    effect_layers: &[EffectLayer],
    z_start: usize,
    z_end: usize,
    excluded_effect_layer: Option<usize>,
) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = effect_layers
        .iter()
        .enumerate()
        .filter_map(|(index, layer)| {
            if Some(index) == excluded_effect_layer {
                return None;
            }
            if effect_layer_in_range(layer, z_start, z_end) {
                Some(layer.z_start..layer.z_end)
            } else {
                None
            }
        })
        .collect();
    ranges.sort_by_key(|range| range.start);
    ranges
}

fn collect_layer_events(
    effect_layers: &[EffectLayer],
    backdrop_layers: &[BackdropLayer],
    z_start: usize,
    z_end: usize,
    excluded_effect_layer: Option<usize>,
) -> Vec<LayerEvent> {
    let mut events = Vec::with_capacity(effect_layers.len() + backdrop_layers.len());
    for (index, layer) in backdrop_layers.iter().enumerate() {
        if layer.z_index >= z_start && layer.z_index < z_end {
            events.push(LayerEvent {
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
            events.push(LayerEvent {
                z_index: layer.z_start,
                kind: LayerEventKind::Effect(index),
            });
        }
    }
    events.sort_by(|a, b| {
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
    events
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
        Some(clip_rect) => intersect_rect(rect, clip_rect)?,
        None => rect,
    };

    scissor_rect_for_rect(clipped_rect, root_scale, width, height)
}

fn intersect_rect(a: Rect, b: Rect) -> Option<Rect> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);

    if right <= left || bottom <= top {
        return None;
    }

    Some(Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

fn tint_for_image(color_filter: Option<ColorFilter>, alpha: f32) -> [f32; 4] {
    let alpha = alpha.clamp(0.0, 1.0);
    match color_filter {
        Some(ColorFilter::Tint(tint)) => [
            tint.r().clamp(0.0, 1.0),
            tint.g().clamp(0.0, 1.0),
            tint.b().clamp(0.0, 1.0),
            (tint.a() * alpha).clamp(0.0, 1.0),
        ],
        None => [1.0, 1.0, 1.0, alpha],
    }
}

fn scissor_rect_for_image(
    image: &ImageDraw,
    root_scale: f32,
    width: u32,
    height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let mut left = image.rect.x * root_scale;
    let mut top = image.rect.y * root_scale;
    let mut right = (image.rect.x + image.rect.width) * root_scale;
    let mut bottom = (image.rect.y + image.rect.height) * root_scale;

    if let Some(clip) = image.clip {
        left = left.max(clip.x * root_scale);
        top = top.max(clip.y * root_scale);
        right = right.min((clip.x + clip.width) * root_scale);
        bottom = bottom.min((clip.y + clip.height) * root_scale);
    }

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
            text: std::rc::Rc::<str>::from("t"),
            color: Color::WHITE,
            font_size: 12.0,
            scale: 1.0,
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
    fn collect_effect_ranges_respects_excluded_effect() {
        let layers = vec![effect_layer(10, 40), effect_layer(20, 30)];
        let ranges = collect_effect_ranges(&layers, 10, 40, Some(0));
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], 20..30);
    }

    #[test]
    fn collect_layer_events_includes_nested_when_parent_excluded() {
        let effects = vec![effect_layer(10, 40), effect_layer(20, 30)];
        let backdrops = vec![backdrop_layer(25)];
        let events = collect_layer_events(&effects, &backdrops, 10, 40, Some(0));
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
        let events = collect_layer_events(&effects, &backdrops, 0, 30, None);
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
        let events = collect_layer_events(&effects, &[], 0, 50, None);

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
        let events = collect_layer_events(&effects, &[], 0, 30, None);

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

        let items = collect_non_effect_segment_items(&shapes, &images, &texts, &shadows, 0, 4, &[]);
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

        let items = collect_non_effect_segment_items(
            &shapes,
            &images,
            &texts,
            &shadows,
            0,
            5,
            &effect_ranges,
        );
        assert_eq!(
            items,
            vec![SegmentDrawItem::Shape(0), SegmentDrawItem::Text(0)]
        );
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
