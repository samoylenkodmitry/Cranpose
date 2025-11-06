//! GPU rendering implementation using WGPU

use crate::scene::{DrawShape, TextDraw};
use crate::shaders;
use bytemuck::{Pod, Zeroable};
use compose_ui_graphics::{Brush, Color};
use glyphon::{
    Attrs, Buffer, Color as GlyphonColor, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer,
};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use wgpu::util::DeviceExt;

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

/// Cached GPU resources for a shape
struct CachedShapeBuffers {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    shape_bind_group: wgpu::BindGroup,
}

/// Cached text buffer for text shaping
struct CachedTextBuffer {
    buffer: Buffer,
    text: String,
    scale: f32,
}

/// Hash key for shape caching
#[derive(Hash, Eq, PartialEq, Clone)]
struct ShapeKey {
    rect_bits: [u32; 4],  // f32 as bits for hashing
    radii_bits: [u32; 4],
    brush_hash: u64,
    z_index: usize,
}

/// Hash key for text caching
#[derive(Hash, Eq, PartialEq, Clone)]
struct TextKey {
    text: String,
    rect_bits: [u32; 4],
    scale_bits: u32,
    z_index: usize,
}

pub struct GpuRenderer {
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    pipeline: wgpu::RenderPipeline,
    uniform_bind_group_layout: wgpu::BindGroupLayout,
    shape_bind_group_layout: wgpu::BindGroupLayout,
    font_system: Arc<Mutex<FontSystem>>,
    text_renderer: TextRenderer,
    text_atlas: TextAtlas,
    swash_cache: SwashCache,
    // Caches for GPU resources
    shape_cache: HashMap<ShapeKey, CachedShapeBuffers>,
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

        Self {
            device,
            queue,
            pipeline,
            uniform_bind_group_layout,
            shape_bind_group_layout,
            font_system,
            text_renderer,
            text_atlas,
            swash_cache,
            shape_cache: HashMap::new(),
            text_cache: HashMap::new(),
        }
    }

    fn hash_brush(brush: &Brush) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();

        match brush {
            Brush::Solid(c) => {
                0u8.hash(&mut hasher);
                c.r().to_bits().hash(&mut hasher);
                c.g().to_bits().hash(&mut hasher);
                c.b().to_bits().hash(&mut hasher);
                c.a().to_bits().hash(&mut hasher);
            }
            Brush::LinearGradient(colors) => {
                1u8.hash(&mut hasher);
                for c in colors {
                    c.r().to_bits().hash(&mut hasher);
                    c.g().to_bits().hash(&mut hasher);
                    c.b().to_bits().hash(&mut hasher);
                    c.a().to_bits().hash(&mut hasher);
                }
            }
            Brush::RadialGradient { colors, center, radius } => {
                2u8.hash(&mut hasher);
                center.x.to_bits().hash(&mut hasher);
                center.y.to_bits().hash(&mut hasher);
                radius.to_bits().hash(&mut hasher);
                for c in colors {
                    c.r().to_bits().hash(&mut hasher);
                    c.g().to_bits().hash(&mut hasher);
                    c.b().to_bits().hash(&mut hasher);
                    c.a().to_bits().hash(&mut hasher);
                }
            }
        }

        hasher.finish()
    }

    fn create_shape_key(shape: &DrawShape) -> ShapeKey {
        let radii_bits = if let Some(rounded) = shape.shape {
            let resolved = rounded.resolve(shape.rect.width, shape.rect.height);
            [
                resolved.top_left.to_bits(),
                resolved.top_right.to_bits(),
                resolved.bottom_left.to_bits(),
                resolved.bottom_right.to_bits(),
            ]
        } else {
            [0, 0, 0, 0]
        };

        ShapeKey {
            rect_bits: [
                shape.rect.x.to_bits(),
                shape.rect.y.to_bits(),
                shape.rect.width.to_bits(),
                shape.rect.height.to_bits(),
            ],
            radii_bits,
            brush_hash: Self::hash_brush(&shape.brush),
            z_index: shape.z_index,
        }
    }

    fn create_text_key(text: &TextDraw) -> TextKey {
        TextKey {
            text: text.text.clone(),
            rect_bits: [
                text.rect.x.to_bits(),
                text.rect.y.to_bits(),
                text.rect.width.to_bits(),
                text.rect.height.to_bits(),
            ],
            scale_bits: text.scale.to_bits(),
            z_index: text.z_index,
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

        // Create uniform buffer
        let uniforms = Uniforms {
            viewport: [width as f32, height as f32],
            _padding: [0.0, 0.0],
        };
        let uniform_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Uniform Buffer"),
                contents: bytemuck::cast_slice(&[uniforms]),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        let uniform_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Uniform Bind Group"),
            layout: &self.uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // Prepare all shape buffers (with caching)
        // First, collect keys for current frame shapes using HashSet for O(1) lookups
        let current_keys: HashSet<ShapeKey> = sorted_shapes
            .iter()
            .map(|shape| Self::create_shape_key(shape))
            .collect();

        // Debug: track cache stats
        let cache_size_before = self.shape_cache.len();

        // Remove cache entries for shapes no longer present (O(n) instead of O(n²))
        self.shape_cache.retain(|key, _| current_keys.contains(key));

        let removed = cache_size_before - self.shape_cache.len();
        if removed > 0 {
            println!("Shape cache: removed {} stale entries", removed);
        }

        // First pass: populate cache for missing shapes
        let mut cache_misses = 0;
        for shape in &sorted_shapes {
            let key = Self::create_shape_key(shape);

            if !self.shape_cache.contains_key(&key) {
                // Not in cache, create new buffers
                cache_misses += 1;
                let (vertex_buffer, index_buffer, shape_bind_group) =
                    self.prepare_shape_buffers(shape)?;

                self.shape_cache.insert(
                    key,
                    CachedShapeBuffers {
                        vertex_buffer,
                        index_buffer,
                        shape_bind_group,
                    },
                );
            }
        }

        println!("Shape cache: {} shapes, {} misses, {} hits",
            sorted_shapes.len(), cache_misses, sorted_shapes.len() - cache_misses);

        // Second pass: collect references from cache
        let shape_data: Vec<_> = sorted_shapes
            .iter()
            .map(|shape| {
                let key = Self::create_shape_key(shape);
                let cached = self.shape_cache.get(&key).unwrap();
                (
                    &cached.vertex_buffer,
                    &cached.index_buffer,
                    &cached.shape_bind_group,
                )
            })
            .collect();

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
            render_pass.set_bind_group(0, &uniform_bind_group, &[]);

            // Render each shape
            for (vertex_buffer, index_buffer, shape_bind_group) in &shape_data {
                render_pass.set_bind_group(1, shape_bind_group, &[]);
                render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                render_pass.draw_indexed(0..6, 0, 0..1);
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

        // Debug: track text cache stats
        let text_cache_size_before = self.text_cache.len();

        // Remove cache entries for text no longer present (O(n) instead of O(n²))
        self.text_cache.retain(|key, _| current_text_keys.contains(key));

        let text_removed = text_cache_size_before - self.text_cache.len();
        if text_removed > 0 {
            println!("Text cache: removed {} stale entries", text_removed);
        }

        // Create or get cached text buffers
        let mut text_cache_misses = 0;
        for text_draw in &sorted_texts {
            // Skip empty text or zero-sized rects
            if text_draw.text.is_empty() || text_draw.rect.width <= 0.0 || text_draw.rect.height <= 0.0 {
                continue;
            }

            let key = Self::create_text_key(text_draw);

            if !self.text_cache.contains_key(&key) {
                // Not in cache, create new buffer
                text_cache_misses += 1;
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
                        text: text_draw.text.clone(),
                        scale: text_draw.scale,
                    },
                );
            }
        }

        let valid_texts = sorted_texts.iter()
            .filter(|t| !t.text.is_empty() && t.rect.width > 0.0 && t.rect.height > 0.0)
            .count();
        println!("Text cache: {} texts, {} misses, {} hits",
            valid_texts, text_cache_misses, valid_texts - text_cache_misses);

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

    fn prepare_shape_buffers(
        &self,
        shape: &DrawShape,
    ) -> Result<(wgpu::Buffer, wgpu::Buffer, wgpu::BindGroup), String> {
        let rect = shape.rect;

        // Create vertices for a quad
        let (color, gradient_colors) = match &shape.brush {
            Brush::Solid(c) => ([c.r(), c.g(), c.b(), c.a()], vec![]),
            Brush::LinearGradient(colors) => {
                let first = colors.first().unwrap_or(&Color(1.0, 1.0, 1.0, 1.0));
                ([first.r(), first.g(), first.b(), first.a()], colors.clone())
            }
            Brush::RadialGradient { colors, .. } => {
                let first = colors.first().unwrap_or(&Color(1.0, 1.0, 1.0, 1.0));
                ([first.r(), first.g(), first.b(), first.a()], colors.clone())
            }
        };

        let vertices = [
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
        ];

        let indices: [u16; 6] = [0, 1, 2, 2, 1, 3];

        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

        let index_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Index Buffer"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            });

        // Create shape data
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

        let (brush_type, gradient_start, gradient_count) = match &shape.brush {
            Brush::Solid(_) => (0, 0, 0),
            Brush::LinearGradient(colors) => (1, 0, colors.len() as u32),
            Brush::RadialGradient { colors, .. } => (2, 0, colors.len() as u32),
        };

        let shape_data = ShapeData {
            rect: [rect.x, rect.y, rect.width, rect.height],
            radii,
            brush_type,
            gradient_start,
            gradient_count,
            _padding: 0,
        };

        let shape_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Shape Buffer"),
                contents: bytemuck::cast_slice(&[shape_data]),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        // Create gradient buffer (even if empty)
        let gradient_stops: Vec<GradientStop> = gradient_colors
            .iter()
            .map(|c| GradientStop {
                color: [c.r(), c.g(), c.b(), c.a()],
            })
            .collect();

        let gradient_buffer = if gradient_stops.is_empty() {
            // Create a dummy buffer with at least one element
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Gradient Buffer"),
                    contents: bytemuck::cast_slice(&[GradientStop {
                        color: [0.0, 0.0, 0.0, 0.0],
                    }]),
                    usage: wgpu::BufferUsages::STORAGE,
                })
        } else {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Gradient Buffer"),
                    contents: bytemuck::cast_slice(&gradient_stops),
                    usage: wgpu::BufferUsages::STORAGE,
                })
        };

        let shape_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Shape Bind Group"),
            layout: &self.shape_bind_group_layout,
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

        Ok((vertex_buffer, index_buffer, shape_bind_group))
    }
}
