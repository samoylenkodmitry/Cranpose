//! GPU rendering implementation using WGPU

use crate::scene::{DrawShape, TextDraw};
use crate::shaders;
use crate::{SharedTextBuffer, SharedTextCache, TextCacheKey};
use bytemuck::{Pod, Zeroable};
use compose_ui_graphics::{Brush, Color};
use glyphon::{
    Attrs, Color as GlyphonColor, FontSystem, Metrics, Resolution, Shaping, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer,
};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

// Chunked rendering constants for robustness with large scenes
const MAX_SHAPES_PER_DRAW: usize = 16384; // 16k shapes per draw call
const HARD_MAX_BUFFER_MB: usize = 64; // Maximum 64MB per buffer

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
        let initial_vertex_cap = 64; // 16 shapes * 4 vertices
        let initial_index_cap = 96; // 16 shapes * 6 indices
        let initial_shape_cap = 16;
        let initial_gradient_cap = 16;

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

        let shape_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shape Data Buffer"),
            size: (std::mem::size_of::<ShapeData>() * initial_shape_cap) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let gradient_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Gradient Buffer"),
            size: (std::mem::size_of::<GradientStop>() * initial_gradient_cap) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
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
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
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
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
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
    pipeline: wgpu::RenderPipeline,
    shape_bind_group_layout: wgpu::BindGroupLayout,
    font_system: Arc<Mutex<FontSystem>>,
    text_renderer: TextRenderer,
    text_atlas: TextAtlas,
    swash_cache: SwashCache,
    // Persistent GPU buffers (reused across frames)
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    shape_buffers: ShapeBatchBuffers,
    // Shared text cache used by both measurement and rendering
    text_cache: SharedTextCache,
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

        let shape_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Shape Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
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
                entry_point: "vs_main",
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
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
        });

        let swash_cache = SwashCache::new();
        let mut text_atlas = TextAtlas::new(&device, &queue, surface_format);

        log::info!("=== Text Atlas Created ===");
        log::info!("  Format: {:?}", surface_format);
        log::info!("  Atlas kind: {:?}", text_atlas.kind());
        log::info!("  Atlas size: {}x{}", text_atlas.width(), text_atlas.height());

        let text_renderer = TextRenderer::new(
            &mut text_atlas,
            &device,
            wgpu::MultisampleState::default(),
            None,
        );

        log::info!("  TextRenderer created successfully");

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

        Self {
            device,
            queue,
            pipeline,
            shape_bind_group_layout,
            font_system,
            text_renderer,
            text_atlas,
            swash_cache,
            uniform_buffer,
            uniform_bind_group,
            shape_buffers,
            text_cache,
        }
    }

    pub fn render(
        &mut self,
        view: &wgpu::TextureView,
        shapes: &[DrawShape],
        texts: &[TextDraw],
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        // Sort by z-index
        let mut sorted_shapes = shapes.to_vec();
        sorted_shapes.sort_by_key(|s| s.z_index);

        let mut sorted_texts = texts.to_vec();
        sorted_texts.sort_by_key(|t| t.z_index);

        // Update uniform buffer with viewport dimensions
        let uniforms = Uniforms {
            viewport: [width as f32, height as f32],
            _padding: [0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        // Chunked rendering for robustness with large scenes
        let total_shape_count = sorted_shapes.len();

        if total_shape_count > MAX_SHAPES_PER_DRAW {
            eprintln!(
                "INFO: Rendering {} shapes in {} chunks (max {} per draw)",
                total_shape_count,
                total_shape_count.div_ceil(MAX_SHAPES_PER_DRAW),
                MAX_SHAPES_PER_DRAW
            );
        }

        // First pass: collect all shape data and gradients across entire scene
        let mut all_gradients = Vec::new();
        let mut all_shape_data = Vec::new();

        for shape in &sorted_shapes {
            let rect = shape.rect;

            // Determine gradient parameters and collect stops
            let mut gradient_params = [0.0f32; 4];
            let (brush_type, gradient_start, gradient_count) = match &shape.brush {
                Brush::Solid(_) => (0u32, 0u32, 0u32),
                Brush::LinearGradient(colors) => {
                    let start = all_gradients.len() as u32;
                    for c in colors {
                        all_gradients.push(GradientStop {
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
                    let start = all_gradients.len() as u32;
                    for c in colors {
                        all_gradients.push(GradientStop {
                            color: [c.r(), c.g(), c.b(), c.a()],
                        });
                    }
                    // Store radial gradient parameters (center is relative to rect)
                    gradient_params = [
                        rect.x + center.x,
                        rect.y + center.y,
                        radius.max(f32::EPSILON),
                        0.0,
                    ];
                    (2u32, start, colors.len() as u32)
                }
            };

            // Shape data
            let radii = if let Some(rounded) = shape.shape {
                let resolved = rounded.resolve(rect.width, rect.height);
                [
                    resolved.top_left,
                    resolved.top_right,
                    resolved.bottom_left,
                    resolved.bottom_right,
                ]
            } else {
                [0.0, 0.0, 0.0, 0.0]
            };

            all_shape_data.push(ShapeData {
                rect: [rect.x, rect.y, rect.width, rect.height],
                radii,
                gradient_params,
                brush_type,
                gradient_start,
                gradient_count,
                _padding: 0,
            });
        }

        // Ensure buffers can hold at least one chunk
        self.shape_buffers.ensure_capacity(
            &self.device,
            &self.shape_bind_group_layout,
            MAX_SHAPES_PER_DRAW * 4,    // vertices
            MAX_SHAPES_PER_DRAW * 6,    // indices
            MAX_SHAPES_PER_DRAW,        // shapes
            all_gradients.len().max(1), // all gradients (written once)
        );

        // Write gradients once for all chunks
        if !all_gradients.is_empty() {
            self.queue.write_buffer(
                &self.shape_buffers.gradient_buffer,
                0,
                bytemuck::cast_slice(&all_gradients),
            );
        }

        // Second pass: render shapes in chunks with proper synchronization
        // Each chunk gets its own encoder+submit to ensure buffer writes complete before next chunk
        for (chunk_idx, chunk) in sorted_shapes.chunks(MAX_SHAPES_PER_DRAW).enumerate() {
            let chunk_len = chunk.len();
            let chunk_start = chunk_idx * MAX_SHAPES_PER_DRAW;

            let mut vertices = Vec::with_capacity(chunk_len * 4);
            let mut indices = Vec::with_capacity(chunk_len * 6);

            // Build vertices and indices for this chunk
            for (shape_idx, shape) in chunk.iter().enumerate() {
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

                // Vertices for quad
                vertices.extend_from_slice(&[
                    Vertex {
                        position: [rect.x, rect.y],
                        color,
                        uv: [0.0, 0.0],
                    },
                    Vertex {
                        position: [rect.x + rect.width, rect.y],
                        color,
                        uv: [1.0, 0.0],
                    },
                    Vertex {
                        position: [rect.x, rect.y + rect.height],
                        color,
                        uv: [0.0, 1.0],
                    },
                    Vertex {
                        position: [rect.x + rect.width, rect.y + rect.height],
                        color,
                        uv: [1.0, 1.0],
                    },
                ]);

                // Indices for two triangles
                indices.extend_from_slice(&[
                    base_vertex,
                    base_vertex + 1,
                    base_vertex + 2,
                    base_vertex + 2,
                    base_vertex + 1,
                    base_vertex + 3,
                ]);
            }

            // Get shape data slice for this chunk
            let chunk_shape_data = &all_shape_data[chunk_start..chunk_start + chunk_len];

            // Write chunk data and render in one encoder (submit after to ensure synchronization)
            if !vertices.is_empty() {
                // Write this chunk's data to buffers
                self.queue.write_buffer(
                    &self.shape_buffers.vertex_buffer,
                    0,
                    bytemuck::cast_slice(&vertices),
                );
                self.queue.write_buffer(
                    &self.shape_buffers.index_buffer,
                    0,
                    bytemuck::cast_slice(&indices),
                );
                self.queue.write_buffer(
                    &self.shape_buffers.shape_buffer,
                    0,
                    bytemuck::cast_slice(chunk_shape_data),
                );

                // Create encoder for this chunk
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
                                    wgpu::LoadOp::Clear(wgpu::Color {
                                        r: 18.0 / 255.0,
                                        g: 18.0 / 255.0,
                                        b: 24.0 / 255.0,
                                        a: 1.0,
                                    })
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

                // Submit this chunk immediately to ensure synchronization before next chunk
                self.queue.submit(std::iter::once(encoder.finish()));
            }
        }

        // Prepare text rendering - create buffers and text areas (with caching)
        let mut font_system = self.font_system.lock().unwrap();

        // Collect keys for current frame text using HashSet for O(1) lookups
        // Using 32.0 base font size (~2.3x default 14.0) for better visibility on high-DPI Android screens
        const BASE_FONT_SIZE: f32 = 32.0;
        let current_text_keys: HashSet<TextCacheKey> = sorted_texts
            .iter()
            .filter(|t| !t.text.is_empty() && t.rect.width > 0.0 && t.rect.height > 0.0)
            .map(|text| TextCacheKey::new(&text.text, BASE_FONT_SIZE * text.scale))
            .collect();

        // ANDROID TEST: Disable caching completely - always create fresh buffers
        // Clear cache entirely each frame
        self.text_cache.lock().unwrap().clear();

        // Always create fresh text buffers (no caching, no reuse)
        for text_draw in &sorted_texts {
            // Skip empty text or zero-sized rects
            if text_draw.text.is_empty()
                || text_draw.rect.width <= 0.0
                || text_draw.rect.height <= 0.0
            {
                continue;
            }

            let key = TextCacheKey::new(&text_draw.text, BASE_FONT_SIZE * text_draw.scale);
            let font_size = BASE_FONT_SIZE * text_draw.scale;

            // Use moderate size for Android compatibility (4096 causes atlas corruption)
            const MAX_LAYOUT_SIZE: f32 = 2048.0;

            // ALWAYS create new buffer (never reuse from cache)
            log::info!("Creating FRESH text buffer '{}' with layout size {}, font_size={}",
                text_draw.text.chars().take(10).collect::<String>(),
                MAX_LAYOUT_SIZE, font_size);

            let mut buffer = glyphon::Buffer::new(
                &mut font_system,
                Metrics::new(font_size, font_size * 1.4),
            );
            // Use large but finite dimensions (prevents GPU precision issues)
            buffer.set_size(&mut font_system, MAX_LAYOUT_SIZE, MAX_LAYOUT_SIZE);
            buffer.set_text(
                &mut font_system,
                &text_draw.text,
                Attrs::new(),
                Shaping::Advanced,
            );
            buffer.shape_until_scroll(&mut font_system);

            let runs = buffer.layout_runs().count();
            log::info!("  Fresh buffer shaped: runs={}", runs);

            let mut text_cache = self.text_cache.lock().unwrap();
            text_cache.insert(
                key,
                SharedTextBuffer {
                    buffer,
                    text: text_draw.text.clone(),
                    font_size,
                    cached_size: None,
                },
            );
        }

        // Collect text data from cache
        let text_data: Vec<(&TextDraw, TextCacheKey)> = sorted_texts
            .iter()
            .filter(|t| !t.text.is_empty() && t.rect.width > 0.0 && t.rect.height > 0.0)
            .map(|text| (text, TextCacheKey::new(&text.text, BASE_FONT_SIZE * text.scale)))
            .collect();

        // Create text areas using cached buffers
        let mut text_areas = Vec::new();
        let text_cache = self.text_cache.lock().unwrap();

        log::info!("=== Creating {} text areas ===", text_data.len());

        for (idx, (text_draw, key)) in text_data.iter().enumerate() {
            let cached = text_cache.get(key).expect("Text should be in cache");

            // Log buffer state and detailed glyph information
            let (buf_w, buf_h) = cached.buffer.size();
            let layout_runs: Vec<_> = cached.buffer.layout_runs().collect();
            let buf_runs = layout_runs.len();

            // Log detailed glyph info for first run
            if let Some(first_run) = layout_runs.first() {
                log::info!("  Buffer '{}': line_w={:.1}, line_y={:.1}, glyphs={}",
                    text_draw.text.chars().take(10).collect::<String>(),
                    first_run.line_w, first_run.line_y, first_run.glyphs.len());

                // Log first few glyphs
                for (glyph_idx, glyph) in first_run.glyphs.iter().take(3).enumerate() {
                    log::info!("    Glyph {}: x={:.1} y={:.1} w={} glyph_id={} font_id={}",
                        glyph_idx, glyph.x, glyph.y, glyph.w, glyph.glyph_id, glyph.font_id);
                }
            }

            let color = GlyphonColor::rgba(
                (text_draw.color.r() * 255.0) as u8,
                (text_draw.color.g() * 255.0) as u8,
                (text_draw.color.b() * 255.0) as u8,
                (text_draw.color.a() * 255.0) as u8,
            );

            let bounds = TextBounds {
                left: text_draw.clip.map(|c| c.x as i32).unwrap_or(0),
                top: text_draw.clip.map(|c| c.y as i32).unwrap_or(0),
                right: text_draw
                    .clip
                    .map(|c| (c.x + c.width) as i32)
                    .unwrap_or(width as i32),
                bottom: text_draw
                    .clip
                    .map(|c| (c.y + c.height) as i32)
                    .unwrap_or(height as i32),
            };

            log::info!("  Text {}: '{}' pos=({:.1}, {:.1}) scale={:.2} color=({}, {}, {}, {}) bounds=({}, {}, {}, {}) buffer={:.0}x{:.0} runs={}",
                idx,
                text_draw.text.chars().take(10).collect::<String>(),
                text_draw.rect.x, text_draw.rect.y,
                text_draw.scale,
                color.r(), color.g(), color.b(), color.a(),
                bounds.left, bounds.top, bounds.right, bounds.bottom,
                buf_w, buf_h, buf_runs);

            text_areas.push(TextArea {
                buffer: &cached.buffer,
                left: text_draw.rect.x,
                top: text_draw.rect.y,
                // Use scale 1.0 since font_size already incorporates text_draw.scale
                // Double-scaling causes corruption/clipping on Android
                scale: 1.0,
                bounds,
                default_color: color,
            });
        }

        // NOTE: Do NOT call text_atlas.trim() here!
        // Calling trim() every frame causes race conditions where the GPU tries to read
        // glyphs that have been cleared by the CPU, resulting in corrupted/missing text.
        // The atlas will auto-grow as needed and glyphon handles memory internally.

        // Prepare all text at once
        if !text_areas.is_empty() {
            log::info!("=== Calling text_renderer.prepare() for {} text areas ===", text_areas.len());

            // Log atlas state BEFORE prepare
            log::info!("  Atlas BEFORE prepare: {}x{}, kind={:?}",
                self.text_atlas.width(), self.text_atlas.height(), self.text_atlas.kind());

            // Create fresh SwashCache each frame to avoid stale rasterization data
            // This fixes flickering and ensures clean glyph rendering on Android
            let mut fresh_swash_cache = SwashCache::new();

            let prepare_result = self.text_renderer
                .prepare(
                    &self.device,
                    &self.queue,
                    &mut font_system,
                    &mut self.text_atlas,
                    Resolution { width, height },
                    text_areas.iter().cloned(),
                    &mut fresh_swash_cache,
                );

            match prepare_result {
                Ok(_) => {
                    log::info!("  Text prepare SUCCESS - glyphs uploaded to GPU atlas");
                    log::info!("  Atlas AFTER prepare: {}x{}, kind={:?}",
                        self.text_atlas.width(), self.text_atlas.height(), self.text_atlas.kind());
                }
                Err(ref e) => {
                    log::error!("  Text prepare FAILED: {:?}", e);
                    return Err(format!("Text prepare error: {:?}", e));
                }
            }
        }

        drop(font_system);

        // Create encoder for text rendering
        let mut text_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Text Encoder"),
                });

        {
            let mut text_pass = text_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Text Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            log::info!("=== Calling text_renderer.render() ===");
            log::info!("  Viewport: {}x{}", width, height);
            log::info!("  Atlas for render: {}x{}", self.text_atlas.width(), self.text_atlas.height());

            let render_result = self.text_renderer
                .render(&self.text_atlas, &mut text_pass);

            match render_result {
                Ok(_) => log::info!("  Text render SUCCESS"),
                Err(ref e) => {
                    log::error!("  Text render FAILED: {:?}", e);
                    return Err(format!("Text render error: {:?}", e));
                }
            }
        }

        log::info!("=== Submitting text encoder ===");
        self.queue.submit(std::iter::once(text_encoder.finish()));

        Ok(())
    }
}
