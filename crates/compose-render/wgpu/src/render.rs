//! GPU rendering implementation using WGPU

use crate::scene::{DrawShape, TextDraw};
use crate::shaders;
use bytemuck::{Pod, Zeroable};
use compose_ui_graphics::{Brush, Color};
use glyphon::{
    Attrs, Buffer, Color as GlyphonColor, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer,
};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

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
    rect: [f32; 4],        // x, y, width, height
    radii: [f32; 4],       // top_left, top_right, bottom_left, bottom_right
    brush_type: u32,       // 0=solid, 1=linear_gradient, 2=radial_gradient
    gradient_start: u32,   // Starting index in gradient buffer
    gradient_count: u32,   // Number of gradient stops
    _padding: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct GradientStop {
    color: [f32; 4],
}

/// Cached text buffer for text shaping
struct CachedTextBuffer {
    buffer: Buffer,
}

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
        let initial_vertex_cap = 64;  // 16 shapes * 4 vertices
        let initial_index_cap = 96;   // 16 shapes * 6 indices
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
            size: (std::mem::size_of::<u16>() * initial_index_cap) as u64,
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

    /// Ensure buffers have enough capacity, resizing if needed
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

        if vertices_needed > self.vertex_capacity {
            let new_cap = vertices_needed.next_power_of_two();
            self.vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Shape Vertex Buffer"),
                size: (std::mem::size_of::<Vertex>() * new_cap) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.vertex_capacity = new_cap;
        }

        if indices_needed > self.index_capacity {
            let new_cap = indices_needed.next_power_of_two();
            self.index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Shape Index Buffer"),
                size: (std::mem::size_of::<u16>() * new_cap) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.index_capacity = new_cap;
        }

        if shapes_needed > self.shape_capacity {
            let new_cap = shapes_needed.next_power_of_two();
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
            let new_cap = gradients_needed.max(1).next_power_of_two();
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

/// Hash key for text caching - only content matters, not position
#[derive(Hash, Eq, PartialEq, Clone)]
struct TextKey {
    text: String,
    scale_bits: u32,
    // NOTE: Removed rect and z_index - text shaping only depends on content + scale
    // Position is applied during rendering, not shaping
}

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
    // Text cache (content-based, not GPU buffers)
    text_cache: HashMap<TextKey, CachedTextBuffer>,
}

impl GpuRenderer {
    pub fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
        font_system: Arc<Mutex<FontSystem>>,
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
        let text_renderer = TextRenderer::new(
            &mut text_atlas,
            &device,
            wgpu::MultisampleState::default(),
            None,
        );

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
            text_cache: HashMap::new(),
        }
    }

    fn create_text_key(text: &TextDraw) -> TextKey {
        TextKey {
            text: text.text.clone(),
            scale_bits: text.scale.to_bits(),
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
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

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
        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&uniforms),
        );

        // Build batched shape data
        let shape_count = sorted_shapes.len();
        let mut vertices = Vec::with_capacity(shape_count * 4);
        let mut indices = Vec::with_capacity(shape_count * 6);
        let mut shape_data = Vec::with_capacity(shape_count);
        let mut gradients = Vec::new();

        for (shape_idx, shape) in sorted_shapes.iter().enumerate() {
            let rect = shape.rect;
            let base_vertex = (shape_idx * 4) as u16;

            // Determine gradient parameters and collect stops
            let (color, brush_type, gradient_start, gradient_count) = match &shape.brush {
                Brush::Solid(c) => {
                    ([c.r(), c.g(), c.b(), c.a()], 0u32, 0u32, 0u32)
                }
                Brush::LinearGradient(colors) => {
                    let start = gradients.len() as u32;
                    let first = colors.first().unwrap_or(&Color(1.0, 1.0, 1.0, 1.0));
                    for c in colors {
                        gradients.push(GradientStop {
                            color: [c.r(), c.g(), c.b(), c.a()],
                        });
                    }
                    ([first.r(), first.g(), first.b(), first.a()], 1u32, start, colors.len() as u32)
                }
                Brush::RadialGradient { colors, .. } => {
                    let start = gradients.len() as u32;
                    let first = colors.first().unwrap_or(&Color(1.0, 1.0, 1.0, 1.0));
                    for c in colors {
                        gradients.push(GradientStop {
                            color: [c.r(), c.g(), c.b(), c.a()],
                        });
                    }
                    ([first.r(), first.g(), first.b(), first.a()], 2u32, start, colors.len() as u32)
                }
            };

            // Vertices for quad
            vertices.extend_from_slice(&[
                Vertex { position: [rect.x, rect.y], color, uv: [0.0, 0.0] },
                Vertex { position: [rect.x + rect.width, rect.y], color, uv: [1.0, 0.0] },
                Vertex { position: [rect.x, rect.y + rect.height], color, uv: [0.0, 1.0] },
                Vertex { position: [rect.x + rect.width, rect.y + rect.height], color, uv: [1.0, 1.0] },
            ]);

            // Indices for two triangles
            indices.extend_from_slice(&[
                base_vertex, base_vertex + 1, base_vertex + 2,
                base_vertex + 2, base_vertex + 1, base_vertex + 3,
            ]);

            // Shape data
            let radii = if let Some(rounded) = shape.shape {
                let resolved = rounded.resolve(rect.width, rect.height);
                [resolved.top_left, resolved.top_right, resolved.bottom_left, resolved.bottom_right]
            } else {
                [0.0, 0.0, 0.0, 0.0]
            };

            shape_data.push(ShapeData {
                rect: [rect.x, rect.y, rect.width, rect.height],
                radii,
                brush_type,
                gradient_start,
                gradient_count,
                _padding: 0,
            });
        }

        // Ensure capacity and update buffers
        if shape_count > 0 {
            self.shape_buffers.ensure_capacity(
                &self.device,
                &self.shape_bind_group_layout,
                vertices.len(),
                indices.len(),
                shape_data.len(),
                gradients.len().max(1),
            );

            self.queue.write_buffer(&self.shape_buffers.vertex_buffer, 0, bytemuck::cast_slice(&vertices));
            self.queue.write_buffer(&self.shape_buffers.index_buffer, 0, bytemuck::cast_slice(&indices));
            self.queue.write_buffer(&self.shape_buffers.shape_buffer, 0, bytemuck::cast_slice(&shape_data));

            if !gradients.is_empty() {
                self.queue.write_buffer(&self.shape_buffers.gradient_buffer, 0, bytemuck::cast_slice(&gradients));
            }
        }

        // Render shapes
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shape Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 18.0 / 255.0,
                            g: 18.0 / 255.0,
                            b: 24.0 / 255.0,
                            a: 1.0,
                        }),
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

            if shape_count > 0 {
                render_pass.set_vertex_buffer(0, self.shape_buffers.vertex_buffer.slice(..));
                render_pass.set_index_buffer(
                    self.shape_buffers.index_buffer.slice(..),
                    wgpu::IndexFormat::Uint16,
                );
                // Draw all shapes in one call
                render_pass.draw_indexed(0..(shape_count as u32 * 6), 0, 0..1);
            }
        }

        // Prepare text rendering - create buffers and text areas (with caching)
        let mut font_system = self.font_system.lock().unwrap();

        // Collect keys for current frame text using HashSet for O(1) lookups
        let current_text_keys: HashSet<TextKey> = sorted_texts
            .iter()
            .filter(|t| !t.text.is_empty() && t.rect.width > 0.0 && t.rect.height > 0.0)
            .map(|text| Self::create_text_key(text))
            .collect();

        // Remove cache entries for text no longer present (O(n) instead of O(n²))
        self.text_cache.retain(|key, _| current_text_keys.contains(key));

        // Create or get cached text buffers
        for text_draw in &sorted_texts {
            // Skip empty text or zero-sized rects
            if text_draw.text.is_empty() || text_draw.rect.width <= 0.0 || text_draw.rect.height <= 0.0 {
                continue;
            }

            let key = Self::create_text_key(text_draw);

            if !self.text_cache.contains_key(&key) {
                // Not in cache, create new buffer
                let mut buffer = Buffer::new(
                    &mut font_system,
                    Metrics::new(14.0 * text_draw.scale, 20.0 * text_draw.scale),
                );
                // Don't constrain buffer size - let it shape freely
                buffer.set_size(&mut font_system, f32::MAX, f32::MAX);
                buffer.set_text(
                    &mut font_system,
                    &text_draw.text,
                    Attrs::new(),
                    Shaping::Advanced,
                );
                buffer.shape_until_scroll(&mut font_system);

                self.text_cache.insert(
                    key,
                    CachedTextBuffer {
                        buffer,
                    },
                );
            }
        }

        // Collect text data from cache
        let text_data: Vec<(&TextDraw, TextKey)> = sorted_texts
            .iter()
            .filter(|t| !t.text.is_empty() && t.rect.width > 0.0 && t.rect.height > 0.0)
            .map(|text| (text, Self::create_text_key(text)))
            .collect();

        // Create text areas using cached buffers
        let mut text_areas = Vec::new();
        for (text_draw, key) in &text_data {
            let cached = self.text_cache.get(key).expect("Text should be in cache");
            let color = GlyphonColor::rgba(
                (text_draw.color.r() * 255.0) as u8,
                (text_draw.color.g() * 255.0) as u8,
                (text_draw.color.b() * 255.0) as u8,
                (text_draw.color.a() * 255.0) as u8,
            );

            text_areas.push(TextArea {
                buffer: &cached.buffer,
                left: text_draw.rect.x,
                top: text_draw.rect.y,
                scale: 1.0,
                bounds: TextBounds {
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
                },
                default_color: color,
            });
        }

        // Prepare all text at once
        self.text_renderer
            .prepare(
                &self.device,
                &self.queue,
                &mut font_system,
                &mut self.text_atlas,
                Resolution { width, height },
                text_areas.iter().cloned(),
                &mut self.swash_cache,
            )
            .map_err(|e| format!("Text prepare error: {:?}", e))?;

        // Trim the atlas after preparing
        self.text_atlas.trim();

        drop(font_system);

        {
            let mut text_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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

            self.text_renderer
                .render(&self.text_atlas, &mut text_pass)
                .map_err(|e| format!("Text render error: {:?}", e))?;
        }

        self.queue.submit(std::iter::once(encoder.finish()));

        Ok(())
    }
}
