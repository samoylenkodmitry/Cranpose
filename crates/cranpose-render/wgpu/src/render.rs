//! GPU rendering implementation using WGPU

use crate::scene::{DrawShape, ImageDraw, TextDraw};
use crate::shaders;
use crate::{SharedTextBuffer, SharedTextCache, TextCacheKey};
use bytemuck::{Pod, Zeroable};
use cranpose_ui_graphics::{Brush, Color, ColorFilter, ImageBitmap};
use glyphon::{
    Attrs, Cache, Color as GlyphonColor, FontSystem, Metrics, Resolution, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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
    gradient_params: [f32; 4], // center.x, center.y, radius, unused
    clip_rect: [f32; 4],       // clip_x, clip_y, clip_width, clip_height (0,0,0,0 = no clip)
    brush_type: u32,           // 0=solid, 1=linear_gradient, 2=radial_gradient
    gradient_start: u32,       // Starting index in gradient buffer
    gradient_count: u32,       // Number of gradient stops
    _padding: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct GradientStop {
    color: [f32; 4],
}

struct CachedImageTexture {
    bind_group: wgpu::BindGroup,
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
    shape_bind_group_layout: wgpu::BindGroupLayout,
    image_pipeline: wgpu::RenderPipeline,
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
}

impl GpuRenderer {
    pub fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
        font_system: Arc<Mutex<FontSystem>>,
        text_cache: SharedTextCache,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shape Shader"),
            source: wgpu::ShaderSource::Wgsl(shaders::SHADER.into()),
        });

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

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[&uniform_bind_group_layout, &shape_bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
        });

        let image_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Image Shader"),
            source: wgpu::ShaderSource::Wgsl(shaders::IMAGE_SHADER.into()),
        });

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

        let image_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Image Pipeline Layout"),
                bind_group_layouts: &[&uniform_bind_group_layout, &image_bind_group_layout],
                push_constant_ranges: &[],
            });

        let image_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
        });

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

        Self {
            device,
            queue,
            surface_format,
            pipeline,
            shape_bind_group_layout,
            image_pipeline,
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
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Result<(), String> {
        log::trace!(
            "🎨 Rendering: {} shapes, {} images, {} texts (size: {}x{})",
            shapes.len(),
            images.len(),
            texts.len(),
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
            let rect = shape.rect;

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
            let (brush_type, gradient_start, gradient_count) = match &shape.brush {
                Brush::Solid(_) => (0u32, 0u32, 0u32),
                Brush::LinearGradient(colors) => {
                    let start = self.scratch_gradients.len() as u32;
                    for c in colors {
                        self.scratch_gradients.push(GradientStop {
                            color: [c.r(), c.g(), c.b(), c.a()],
                        });
                    }
                    (1u32, start, colors.len() as u32)
                }
                Brush::RadialGradient {
                    colors,
                    center,
                    radius,
                } => {
                    let start = self.scratch_gradients.len() as u32;
                    for c in colors {
                        self.scratch_gradients.push(GradientStop {
                            color: [c.r(), c.g(), c.b(), c.a()],
                        });
                    }
                    // Store radial gradient parameters (center is relative to rect, scaled to physical)
                    gradient_params = [
                        x + center.x * root_scale,
                        y + center.y * root_scale,
                        (radius * root_scale).max(f32::EPSILON),
                        0.0,
                    ];
                    (2u32, start, colors.len() as u32)
                }
            };

            // Shape data (radii scaled to physical pixels)
            let radii = if let Some(rounded) = shape.shape {
                let resolved = rounded.resolve(rect.width, rect.height);
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
                rect: [x, y, w, h],
                radii,
                gradient_params,
                clip_rect,
                brush_type,
                gradient_start,
                gradient_count,
                _padding: 0,
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
                let rect = shape.rect;
                let base_vertex = (shape_idx * 4) as u32;

                // Get color from brush for vertex data
                let color = match &shape.brush {
                    Brush::Solid(c) => [c.r(), c.g(), c.b(), c.a()],
                    Brush::LinearGradient(colors) => {
                        let first = colors.first().unwrap_or(&Color(1.0, 1.0, 1.0, 1.0));
                        [first.r(), first.g(), first.b(), first.a()]
                    }
                    Brush::RadialGradient { colors, .. } => {
                        let first = colors.first().unwrap_or(&Color(1.0, 1.0, 1.0, 1.0));
                        [first.r(), first.g(), first.b(), first.a()]
                    }
                };

                // Scale logical dp to physical pixels for GPU rendering
                let x = rect.x * root_scale;
                let y = rect.y * root_scale;
                let w = rect.width * root_scale;
                let h = rect.height * root_scale;

                // Vertices for quad (in physical pixels)
                self.scratch_vertices.extend_from_slice(&[
                    Vertex {
                        position: [x, y],
                        color,
                        uv: [0.0, 0.0],
                    },
                    Vertex {
                        position: [x + w, y],
                        color,
                        uv: [1.0, 0.0],
                    },
                    Vertex {
                        position: [x, y + h],
                        color,
                        uv: [0.0, 1.0],
                    },
                    Vertex {
                        position: [x + w, y + h],
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
                let rect = image_draw.rect;
                if rect.width <= 0.0 || rect.height <= 0.0 || image_draw.alpha <= 0.0 {
                    continue;
                }

                let x = rect.x * root_scale;
                let y = rect.y * root_scale;
                let w = rect.width * root_scale;
                let h = rect.height * root_scale;
                if w <= 0.0 || h <= 0.0 {
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
                        position: [x, y],
                        color: tint,
                        uv: [u_min, v_min],
                    },
                    Vertex {
                        position: [x + w, y],
                        color: tint,
                        uv: [u_max, v_min],
                    },
                    Vertex {
                        position: [x, y + h],
                        color: tint,
                        uv: [u_min, v_max],
                    },
                    Vertex {
                        position: [x + w, y + h],
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

        Ok(())
    }
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
