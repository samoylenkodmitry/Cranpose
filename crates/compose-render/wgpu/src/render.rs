//! GPU rendering implementation using WGPU

use crate::font::{PreferredFont, DEFAULT_FONT_SIZE, DEFAULT_LINE_HEIGHT};
use crate::scene::{DrawShape, TextDraw};
use crate::shaders;
use bytemuck::{Pod, Zeroable};
use compose_ui_graphics::{Brush, Rect};
use glyphon::{
    fontdb, Attrs, Buffer, Color as GlyphonColor, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer,
};
use lru::LruCache;
use std::collections::BTreeMap;
use std::mem;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

const TEXT_CACHE_INITIAL_CAPACITY: usize = 128;
const TEXT_CACHE_MAX_CAPACITY: usize = 4096;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
    shape_index: u32,
    _padding: u32,
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4, 2 => Uint32];

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

#[derive(Clone, Hash, PartialEq, Eq)]
struct TextCacheKey {
    text: String,
    scale_key: u32,
}

impl TextCacheKey {
    fn new(text: &str, scale: f32) -> Self {
        let scaled = (scale * 1000.0).round().max(0.0);
        let scale_key = scaled.min(u32::MAX as f32) as u32;
        Self {
            text: text.to_string(),
            scale_key,
        }
    }

    fn scale_key(&self) -> u32 {
        self.scale_key
    }
}

struct CachedTextBuffer {
    buffer: Buffer,
    metrics: Metrics,
    scale_key: u32,
    height: f32,
    text: String,
}

impl CachedTextBuffer {
    fn new(
        font_system: &mut FontSystem,
        metrics: Metrics,
        scale_key: u32,
        height: f32,
        text: &str,
        attrs: Attrs,
    ) -> Self {
        let mut buffer = Buffer::new(font_system, metrics);
        buffer.set_size(font_system, f32::MAX, height);
        buffer.set_text(font_system, text, attrs, Shaping::Advanced);
        buffer.shape_until_scroll(font_system);
        Self {
            buffer,
            metrics,
            scale_key,
            height,
            text: text.to_string(),
        }
    }

    fn ensure(
        &mut self,
        font_system: &mut FontSystem,
        metrics: Metrics,
        scale_key: u32,
        height: f32,
        text: &str,
        attrs: Attrs,
    ) -> bool {
        const HEIGHT_EPSILON: f32 = 0.5;

        let mut reshaped = false;

        if self.scale_key != scale_key || self.metrics != metrics {
            self.buffer.set_metrics(font_system, metrics);
            self.metrics = metrics;
            self.scale_key = scale_key;
            reshaped = true;
        }

        if (height - self.height).abs() > HEIGHT_EPSILON {
            self.buffer.set_size(font_system, f32::MAX, height);
            self.height = height;
            reshaped = true;
        }

        if self.text != text {
            self.buffer
                .set_text(font_system, text, attrs, Shaping::Advanced);
            self.text.clear();
            self.text.push_str(text);
            reshaped = true;
        }

        if reshaped {
            self.buffer.shape_until_scroll(font_system);
        }

        reshaped
    }
}

struct PreparedTextArea {
    key: TextCacheKey,
    left: f32,
    top: f32,
    bounds: TextBounds,
    color: GlyphonColor,
    scale: f32,
    buffer_height: f32,
    fallback: Option<Box<Buffer>>,
}

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
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> Self {
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Vertex Buffer"),
            size: (mem::size_of::<Vertex>() * 4) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Index Buffer"),
            size: (mem::size_of::<u32>() * 6) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let shape_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shape Buffer"),
            size: mem::size_of::<ShapeData>() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let gradient_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Gradient Buffer"),
            size: mem::size_of::<GradientStop>() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Shape Bind Group"),
            layout,
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
            vertex_capacity: 4,
            index_capacity: 6,
            shape_capacity: 1,
            gradient_capacity: 1,
        }
    }

    fn ensure_capacities(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        vertices: usize,
        indices: usize,
        shapes: usize,
        gradients: usize,
    ) {
        let mut rebuild_bind_group = false;

        if vertices > self.vertex_capacity {
            let new_capacity = vertices.next_power_of_two();
            self.vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Vertex Buffer"),
                size: (mem::size_of::<Vertex>() * new_capacity) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.vertex_capacity = new_capacity;
        }

        if indices > self.index_capacity {
            let new_capacity = indices.next_power_of_two();
            self.index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Index Buffer"),
                size: (mem::size_of::<u32>() * new_capacity) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.index_capacity = new_capacity;
        }

        if shapes > self.shape_capacity {
            let new_capacity = shapes.max(1).next_power_of_two();
            self.shape_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Shape Buffer"),
                size: (mem::size_of::<ShapeData>() * new_capacity) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.shape_capacity = new_capacity;
            rebuild_bind_group = true;
        }

        if gradients > self.gradient_capacity {
            let new_capacity = gradients.max(1).next_power_of_two();
            self.gradient_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Gradient Buffer"),
                size: (mem::size_of::<GradientStop>() * new_capacity) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.gradient_capacity = new_capacity;
            rebuild_bind_group = true;
        }

        if rebuild_bind_group {
            self.bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Shape Bind Group"),
                layout,
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

pub struct GpuRenderer {
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    pipeline: wgpu::RenderPipeline,
    shape_bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    shape_buffers: ShapeBatchBuffers,
    font_system: Arc<Mutex<FontSystem>>,
    text_renderer: TextRenderer,
    text_atlas: TextAtlas,
    swash_cache: SwashCache,
    text_cache: LruCache<TextCacheKey, Box<CachedTextBuffer>>,
    preferred_font: Option<PreferredFont>,
}

impl GpuRenderer {
    fn grow_text_cache(&mut self) {
        let current_cap = self.text_cache.cap().get();
        if current_cap >= TEXT_CACHE_MAX_CAPACITY {
            return;
        }

        let mut new_cap = (current_cap * 2).max(TEXT_CACHE_INITIAL_CAPACITY);
        if new_cap > TEXT_CACHE_MAX_CAPACITY {
            new_cap = TEXT_CACHE_MAX_CAPACITY;
        }

        if let Some(capacity) = NonZeroUsize::new(new_cap) {
            self.text_cache.resize(capacity);
        }
    }

    pub fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
        font_system: Arc<Mutex<FontSystem>>,
        preferred_font: Option<PreferredFont>,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shape Shader"),
            source: wgpu::ShaderSource::Wgsl(
                format!("{}\n{}", shaders::VERTEX_SHADER, shaders::FRAGMENT_SHADER).into(),
            ),
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
                            min_binding_size: Some(
                                std::num::NonZeroU64::new(mem::size_of::<ShapeData>() as u64)
                                    .unwrap(),
                            ),
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

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Uniform Buffer"),
            size: mem::size_of::<Uniforms>() as u64,
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

        let swash_cache = SwashCache::new();
        let mut text_atlas = TextAtlas::new(&device, &queue, surface_format);
        let text_renderer = TextRenderer::new(
            &mut text_atlas,
            &device,
            wgpu::MultisampleState::default(),
            None,
        );

        let shape_buffers = ShapeBatchBuffers::new(&device, &shape_bind_group_layout);

        let text_cache = LruCache::new(NonZeroUsize::new(TEXT_CACHE_INITIAL_CAPACITY).unwrap());

        Self {
            device,
            queue,
            pipeline,
            shape_bind_group_layout,
            uniform_buffer,
            uniform_bind_group,
            shape_buffers,
            font_system,
            text_renderer,
            text_atlas,
            swash_cache,
            text_cache,
            preferred_font,
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

        let uniforms = Uniforms {
            viewport: [width as f32, height as f32],
            _padding: [0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let shape_count = sorted_shapes.len();
        let mut vertex_data = Vec::with_capacity(shape_count * 4);
        let mut index_data: Vec<u32> = Vec::with_capacity(shape_count * 6);
        let mut shape_data_entries = Vec::with_capacity(shape_count);
        let mut gradient_data = Vec::new();

        for (i, shape) in sorted_shapes.iter().enumerate() {
            let rect = shape.rect;
            let shape_index = i as u32;

            let mut gradient_params = [0.0f32; 4];
            let (color, brush_type, gradient_start, gradient_count) = match &shape.brush {
                Brush::Solid(c) => ([c.r(), c.g(), c.b(), c.a()], 0u32, 0u32, 0u32),
                Brush::LinearGradient(colors) => {
                    let start = gradient_data.len() as u32;
                    if colors.is_empty() {
                        ([1.0, 1.0, 1.0, 1.0], 0u32, 0u32, 0u32)
                    } else {
                        for color in colors {
                            gradient_data.push(GradientStop {
                                color: [color.r(), color.g(), color.b(), color.a()],
                            });
                        }
                        let first = colors.first().unwrap();
                        (
                            [first.r(), first.g(), first.b(), first.a()],
                            1u32,
                            start,
                            colors.len() as u32,
                        )
                    }
                }
                Brush::RadialGradient {
                    colors,
                    center,
                    radius,
                } => {
                    if colors.is_empty() {
                        ([1.0, 1.0, 1.0, 1.0], 0u32, 0u32, 0u32)
                    } else {
                        let start = gradient_data.len() as u32;
                        for color in colors {
                            gradient_data.push(GradientStop {
                                color: [color.r(), color.g(), color.b(), color.a()],
                            });
                        }
                        let first = colors.first().unwrap();
                        gradient_params = [
                            rect.x + center.x,
                            rect.y + center.y,
                            radius.max(f32::EPSILON),
                            0.0,
                        ];
                        (
                            [first.r(), first.g(), first.b(), first.a()],
                            2u32,
                            start,
                            colors.len() as u32,
                        )
                    }
                }
            };

            vertex_data.extend_from_slice(&[
                Vertex {
                    position: [rect.x, rect.y],
                    color,
                    shape_index,
                    _padding: 0,
                },
                Vertex {
                    position: [rect.x + rect.width, rect.y],
                    color,
                    shape_index,
                    _padding: 0,
                },
                Vertex {
                    position: [rect.x, rect.y + rect.height],
                    color,
                    shape_index,
                    _padding: 0,
                },
                Vertex {
                    position: [rect.x + rect.width, rect.y + rect.height],
                    color,
                    shape_index,
                    _padding: 0,
                },
            ]);

            let base_index = (i * 4) as u32;
            index_data.extend_from_slice(&[
                base_index,
                base_index + 1,
                base_index + 2,
                base_index + 2,
                base_index + 1,
                base_index + 3,
            ]);

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

            shape_data_entries.push(ShapeData {
                rect: [rect.x, rect.y, rect.width, rect.height],
                radii,
                gradient_params,
                brush_type,
                gradient_start,
                gradient_count,
                _padding: 0,
            });
        }

        if shape_count > 0 {
            let total_vertices = vertex_data.len();
            let total_indices = index_data.len();
            let total_shapes = shape_data_entries.len();
            let total_gradients = gradient_data.len().max(1);

            self.shape_buffers.ensure_capacities(
                &self.device,
                &self.shape_bind_group_layout,
                total_vertices,
                total_indices,
                total_shapes,
                total_gradients,
            );

            self.queue.write_buffer(
                &self.shape_buffers.vertex_buffer,
                0,
                bytemuck::cast_slice(&vertex_data),
            );
            self.queue.write_buffer(
                &self.shape_buffers.index_buffer,
                0,
                bytemuck::cast_slice(&index_data),
            );

            self.queue.write_buffer(
                &self.shape_buffers.shape_buffer,
                0,
                bytemuck::cast_slice(&shape_data_entries),
            );

            if !gradient_data.is_empty() {
                self.queue.write_buffer(
                    &self.shape_buffers.gradient_buffer,
                    0,
                    bytemuck::cast_slice(&gradient_data),
                );
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

            // Render each shape
            if shape_count > 0 {
                render_pass.set_bind_group(1, &self.shape_buffers.bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.shape_buffers.vertex_buffer.slice(..));
                render_pass.set_index_buffer(
                    self.shape_buffers.index_buffer.slice(..),
                    wgpu::IndexFormat::Uint32,
                );

                let index_count = (shape_count * 6) as u32;
                render_pass.draw_indexed(0..index_count, 0, 0..1);
            }
        }

        // Render text
        {
            let mut font_system = self.font_system.lock().unwrap();
            let total_texts = sorted_texts.len();
            let mut skipped_zero_scale = 0usize;
            let mut skipped_invalid_clip = 0usize;
            let mut skipped_invalid_size = 0usize;
            let mut prepared_areas = Vec::with_capacity(sorted_texts.len());

            for text_draw in &sorted_texts {
                let original_scale = text_draw.scale;
                let text_scale = original_scale.max(0.0);
                if text_scale == 0.0 {
                    skipped_zero_scale += 1;
                    if log::log_enabled!(log::Level::Debug) {
                        log::debug!(
                            "Skipping text draw with non-positive scale (scale={:.3}): \"{}\"",
                            original_scale,
                            text_preview(&text_draw.text)
                        );
                    }
                    continue;
                }

                if text_draw.rect.width <= 0.0 || text_draw.rect.height <= 0.0 {
                    skipped_invalid_size += 1;
                    if log::log_enabled!(log::Level::Debug) {
                        log::debug!(
                            "Skipping text draw with non-positive size ({:.1}x{:.1}): \"{}\"",
                            text_draw.rect.width,
                            text_draw.rect.height,
                            text_preview(&text_draw.text)
                        );
                    }
                    continue;
                }

                let Some(bounds) = text_bounds_from_clip(text_draw.clip, width, height) else {
                    skipped_invalid_clip += 1;
                    if log::log_enabled!(log::Level::Debug) {
                        log::debug!(
                            "Skipping text draw clipped outside viewport (clip={:?}): \"{}\"",
                            text_draw.clip,
                            text_preview(&text_draw.text)
                        );
                    }
                    continue;
                };

                let metrics = Metrics::new(
                    DEFAULT_FONT_SIZE * text_scale,
                    DEFAULT_LINE_HEIGHT * text_scale,
                );
                let buffer_height = text_draw.rect.height.max(metrics.line_height);

                let key = TextCacheKey::new(&text_draw.text, text_scale);
                let mut reshaped = false;

                if let Some(entry) = self.text_cache.get_mut(&key) {
                    let attrs = match self.preferred_font.as_ref() {
                        Some(font) => Attrs::new()
                            .family(Family::Name(&font.family))
                            .weight(font.weight),
                        None => Attrs::new().family(Family::SansSerif),
                    };
                    reshaped = entry.ensure(
                        &mut font_system,
                        metrics,
                        key.scale_key(),
                        buffer_height,
                        &text_draw.text,
                        attrs,
                    );
                    log_text_glyphs(&font_system, &entry.buffer, text_draw, text_scale);
                } else {
                    if self.text_cache.len() == self.text_cache.cap().get() {
                        drop(font_system);
                        self.grow_text_cache();
                        font_system = self.font_system.lock().unwrap();
                    }

                    let attrs = match self.preferred_font.as_ref() {
                        Some(font) => Attrs::new()
                            .family(Family::Name(&font.family))
                            .weight(font.weight),
                        None => Attrs::new().family(Family::SansSerif),
                    };
                    let cached = CachedTextBuffer::new(
                        &mut font_system,
                        metrics,
                        key.scale_key(),
                        buffer_height,
                        &text_draw.text,
                        attrs,
                    );
                    self.text_cache.put(key.clone(), Box::new(cached));
                    let entry = self
                        .text_cache
                        .get_mut(&key)
                        .expect("text cache entry missing after insertion");
                    log_text_glyphs(&font_system, &entry.buffer, text_draw, text_scale);
                }

                let to_u8 = |value: f32| -> u8 { (value.clamp(0.0, 1.0) * 255.0).round() as u8 };

                let color = GlyphonColor::rgba(
                    to_u8(text_draw.color.r()),
                    to_u8(text_draw.color.g()),
                    to_u8(text_draw.color.b()),
                    to_u8(text_draw.color.a()),
                );

                prepared_areas.push(PreparedTextArea {
                    key: key.clone(),
                    left: text_draw.rect.x,
                    top: text_draw.rect.y,
                    bounds,
                    color,
                    scale: text_scale,
                    buffer_height,
                    fallback: None,
                });

                if reshaped && log::log_enabled!(log::Level::Debug) {
                    log::debug!(
                        "Reshaped GPU text cache entry for \"{}\" at scale {:.2}",
                        text_preview(&text_draw.text),
                        text_scale
                    );
                }
            }

            let prepared_texts = prepared_areas.len();
            if total_texts > 0 {
                if prepared_texts == 0 {
                    log::warn!(
                        "GPU text renderer prepared zero draws; text will not appear this frame"
                    );
                } else if log::log_enabled!(log::Level::Debug) {
                    log::debug!(
                        "GPU text prepare: prepared {} of {} draw(s) (skipped {} zero scale, {} invalid size, {} clipped)",
                        prepared_texts,
                        total_texts,
                        skipped_zero_scale,
                        skipped_invalid_size,
                        skipped_invalid_clip
                    );
                    log::debug!(
                        "Preparing {} text area(s) for resolution {}x{}",
                        prepared_texts,
                        width,
                        height
                    );
                }
            }

            if prepared_texts > 0 {
                let mut text_areas = Vec::with_capacity(prepared_texts);
                for area in &mut prepared_areas {
                    if let Some(entry) = self.text_cache.peek(&area.key) {
                        let buffer_ref: &Buffer = &entry.buffer;
                        text_areas.push(TextArea {
                            buffer: buffer_ref,
                            left: area.left,
                            top: area.top,
                            scale: 1.0,
                            bounds: area.bounds,
                            default_color: area.color,
                        });
                    } else {
                        if log::log_enabled!(log::Level::Debug) {
                            log::debug!(
                                "GPU text cache entry for \"{}\" was evicted before rendering; reshaping on the fly",
                                text_preview(&area.key.text)
                            );
                        }

                        let metrics = Metrics::new(
                            DEFAULT_FONT_SIZE * area.scale,
                            DEFAULT_LINE_HEIGHT * area.scale,
                        );
                        let mut buffer = Buffer::new(&mut font_system, metrics);
                        buffer.set_size(&mut font_system, f32::MAX, area.buffer_height);
                        let attrs = match self.preferred_font.as_ref() {
                            Some(font) => Attrs::new()
                                .family(Family::Name(&font.family))
                                .weight(font.weight),
                            None => Attrs::new().family(Family::SansSerif),
                        };
                        buffer.set_text(&mut font_system, &area.key.text, attrs, Shaping::Advanced);
                        buffer.shape_until_scroll(&mut font_system);
                        area.fallback = Some(Box::new(buffer));
                        let buffer_ref: &Buffer = area
                            .fallback
                            .as_ref()
                            .map(|owned| owned.as_ref())
                            .expect("fallback buffer missing after creation");
                        text_areas.push(TextArea {
                            buffer: buffer_ref,
                            left: area.left,
                            top: area.top,
                            scale: 1.0,
                            bounds: area.bounds,
                            default_color: area.color,
                        });
                    }
                }

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

                self.text_atlas.trim();
            }
        }

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

            if log::log_enabled!(log::Level::Debug) {
                log::debug!("Submitted GPU text render pass");
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));

        Ok(())
    }
}

fn text_bounds_from_clip(clip: Option<Rect>, width: u32, height: u32) -> Option<TextBounds> {
    match clip {
        Some(rect) => {
            let min_x = rect.x.max(0.0);
            let min_y = rect.y.max(0.0);
            let max_x = (rect.x + rect.width).min(width as f32);
            let max_y = (rect.y + rect.height).min(height as f32);

            if max_x <= min_x || max_y <= min_y {
                return None;
            }

            let left = min_x.floor() as i32;
            let top = min_y.floor() as i32;
            let right = max_x.ceil() as i32;
            let bottom = max_y.ceil() as i32;

            Some(TextBounds {
                left: left.clamp(0, width as i32),
                top: top.clamp(0, height as i32),
                right: right.clamp(0, width as i32),
                bottom: bottom.clamp(0, height as i32),
            })
        }
        None => Some(TextBounds {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        }),
    }
}

fn log_text_glyphs(
    font_system: &FontSystem,
    buffer: &Buffer,
    text_draw: &TextDraw,
    text_scale: f32,
) {
    let debug_enabled = log::log_enabled!(log::Level::Debug);
    let mut glyph_total = 0usize;
    let mut run_count = 0usize;
    let mut fonts: Option<BTreeMap<fontdb::ID, usize>> = if debug_enabled {
        Some(BTreeMap::new())
    } else {
        None
    };

    for run in buffer.layout_runs() {
        run_count += 1;
        glyph_total += run.glyphs.len();
        if let Some(font_map) = fonts.as_mut() {
            for glyph in run.glyphs.iter() {
                *font_map.entry(glyph.font_id).or_insert(0) += 1;
            }
        }
        if glyph_total > 0 && !debug_enabled {
            // No need to continue collecting data when only warnings would be emitted.
            break;
        }
    }

    let text_snippet = info_text_preview(&text_draw.text);

    if glyph_total == 0 {
        let fonts_label = if let Some(font_map) = fonts {
            let font_db = font_system.db();
            font_map
                .into_iter()
                .map(|(font_id, count)| {
                    if let Some(face) = font_db.face(font_id) {
                        let family = face
                            .families
                            .first()
                            .map(|(name, _)| name.as_str())
                            .unwrap_or_else(|| face.post_script_name.as_str());
                        format!(
                            "{} ({}; id {}) - {} {}",
                            family,
                            face.post_script_name,
                            font_id,
                            count,
                            glyph_label(count)
                        )
                    } else {
                        format!("id {} - {} {}", font_id, count, glyph_label(count))
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            String::from("none")
        };

        log::warn!(
            "GPU text glyphs: no glyphs formed for \"{}\" @ scale {:.2} (rect {:.1}×{:.1} at {:.1},{:.1}); fonts resolved: {} ({} run(s))",
            text_snippet,
            text_scale,
            text_draw.rect.width,
            text_draw.rect.height,
            text_draw.rect.x,
            text_draw.rect.y,
            fonts_label,
            run_count
        );
    } else if debug_enabled {
        let fonts_label = if let Some(font_map) = fonts {
            let font_db = font_system.db();
            let font_details: Vec<String> = font_map
                .into_iter()
                .map(|(font_id, count)| {
                    if let Some(face) = font_db.face(font_id) {
                        let family = face
                            .families
                            .first()
                            .map(|(name, _)| name.as_str())
                            .unwrap_or_else(|| face.post_script_name.as_str());
                        format!(
                            "{} ({}; id {}) - {} {}",
                            family,
                            face.post_script_name,
                            font_id,
                            count,
                            glyph_label(count)
                        )
                    } else {
                        format!("id {} - {} {}", font_id, count, glyph_label(count))
                    }
                })
                .collect();
            if font_details.is_empty() {
                String::from("none")
            } else {
                font_details.join(", ")
            }
        } else {
            String::from("none")
        };

        log::debug!(
            "GPU text glyphs: formed {} {} across {} run(s) for \"{}\" @ scale {:.2} (rect {:.1}×{:.1} at {:.1},{:.1}); fonts resolved: {}",
            glyph_total,
            glyph_label(glyph_total),
            run_count,
            text_snippet,
            text_scale,
            text_draw.rect.width,
            text_draw.rect.height,
            text_draw.rect.x,
            text_draw.rect.y,
            fonts_label
        );
    }
}

fn glyph_label(count: usize) -> &'static str {
    if count == 1 {
        "glyph"
    } else {
        "glyphs"
    }
}

const TEXT_PREVIEW_LIMIT: usize = 32;

fn info_text_preview(text: &str) -> String {
    truncated_text(text, TEXT_PREVIEW_LIMIT)
}

fn truncated_text(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let mut preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        preview.push('…');
    }
    preview
}

fn text_preview(text: &str) -> String {
    if !log::log_enabled!(log::Level::Debug) {
        return String::new();
    }
    truncated_text(text, TEXT_PREVIEW_LIMIT)
}
