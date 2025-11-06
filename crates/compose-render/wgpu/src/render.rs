//! GPU rendering implementation using WGPU

use crate::font::{PreferredFont, DEFAULT_FONT_SIZE, DEFAULT_LINE_HEIGHT};
use crate::scene::{DrawShape, TextDraw};
use crate::shaders;
use bytemuck::{Pod, Zeroable};
use compose_ui_graphics::{Brush, Color, Rect};
use glyphon::{
    fontdb, Attrs, Buffer, Color as GlyphonColor, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer,
};
use std::collections::BTreeMap;
use std::mem;
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
    rect: [f32; 4],      // x, y, width, height
    radii: [f32; 4],     // top_left, top_right, bottom_left, bottom_right
    brush_type: u32,     // 0=solid, 1=linear_gradient, 2=radial_gradient
    gradient_start: u32, // Starting index in gradient buffer
    gradient_count: u32, // Number of gradient stops
    _padding: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct GradientStop {
    color: [f32; 4],
}

struct ShapeGpuResources {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    shape_buffer: wgpu::Buffer,
    gradient_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    gradient_capacity: usize,
}

impl ShapeGpuResources {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> Self {
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Vertex Buffer"),
            size: (mem::size_of::<Vertex>() * 4) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Index Buffer"),
            size: (mem::size_of::<u16>() * 6) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let shape_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shape Buffer"),
            size: mem::size_of::<ShapeData>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let gradient_capacity = 1usize;
        let gradient_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Gradient Buffer"),
            size: (mem::size_of::<GradientStop>() * gradient_capacity) as u64,
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
            gradient_capacity,
        }
    }

    fn ensure_gradient_capacity(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        required: usize,
    ) {
        let required_capacity = required.max(1);
        if required_capacity <= self.gradient_capacity {
            return;
        }

        let new_capacity = required_capacity.next_power_of_two();
        self.gradient_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Gradient Buffer"),
            size: (mem::size_of::<GradientStop>() * new_capacity) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.gradient_capacity = new_capacity;
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

    fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        shape: &DrawShape,
    ) {
        let rect = shape.rect;

        let (color, gradient_colors) = match &shape.brush {
            Brush::Solid(c) => ([c.r(), c.g(), c.b(), c.a()], Vec::new()),
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

        queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));

        let indices: [u16; 6] = [0, 1, 2, 2, 1, 3];
        queue.write_buffer(&self.index_buffer, 0, bytemuck::cast_slice(&indices));

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

        let (brush_type, gradient_count) = match &shape.brush {
            Brush::Solid(_) => (0, 0),
            Brush::LinearGradient(colors) => (1, colors.len() as u32),
            Brush::RadialGradient { colors, .. } => (2, colors.len() as u32),
        };

        let shape_data = ShapeData {
            rect: [rect.x, rect.y, rect.width, rect.height],
            radii,
            brush_type,
            gradient_start: 0,
            gradient_count,
            _padding: 0,
        };

        queue.write_buffer(&self.shape_buffer, 0, bytemuck::cast_slice(&[shape_data]));

        let gradient_stops: Vec<GradientStop> = gradient_colors
            .iter()
            .map(|c| GradientStop {
                color: [c.r(), c.g(), c.b(), c.a()],
            })
            .collect();

        self.ensure_gradient_capacity(device, layout, gradient_stops.len());

        if gradient_stops.is_empty() {
            let zero_stop = GradientStop {
                color: [0.0, 0.0, 0.0, 0.0],
            };
            queue.write_buffer(&self.gradient_buffer, 0, bytemuck::cast_slice(&[zero_stop]));
        } else {
            queue.write_buffer(
                &self.gradient_buffer,
                0,
                bytemuck::cast_slice(&gradient_stops),
            );
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
    shape_resources: Vec<ShapeGpuResources>,
    font_system: Arc<Mutex<FontSystem>>,
    text_renderer: TextRenderer,
    text_atlas: TextAtlas,
    swash_cache: SwashCache,
    preferred_font: Option<PreferredFont>,
}

impl GpuRenderer {
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

        Self {
            device,
            queue,
            pipeline,
            shape_bind_group_layout,
            uniform_buffer,
            uniform_bind_group,
            shape_resources: Vec::new(),
            font_system,
            text_renderer,
            text_atlas,
            swash_cache,
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
        if self.shape_resources.len() < shape_count {
            let device = &*self.device;
            let layout = &self.shape_bind_group_layout;
            self.shape_resources.extend(
                (self.shape_resources.len()..shape_count)
                    .map(|_| ShapeGpuResources::new(device, layout)),
            );
        }

        let device = &*self.device;
        let queue = &*self.queue;
        let layout = &self.shape_bind_group_layout;
        for (shape, resources) in sorted_shapes
            .iter()
            .zip(self.shape_resources.iter_mut())
            .take(shape_count)
        {
            resources.update(device, queue, layout, shape);
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
            for resources in self.shape_resources.iter().take(shape_count) {
                render_pass.set_bind_group(1, &resources.bind_group, &[]);
                render_pass.set_vertex_buffer(0, resources.vertex_buffer.slice(..));
                render_pass
                    .set_index_buffer(resources.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                render_pass.draw_indexed(0..6, 0, 0..1);
            }
        }

        // Render text
        {
            let mut font_system = self.font_system.lock().unwrap();
            let mut text_buffers = Vec::with_capacity(sorted_texts.len());
            let mut text_layout = Vec::with_capacity(sorted_texts.len());
            let total_texts = sorted_texts.len();
            let mut skipped_zero_scale = 0usize;
            let mut skipped_invalid_clip = 0usize;
            let mut skipped_invalid_size = 0usize;

            let preferred_font = self.preferred_font.as_ref();

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
                let mut buffer = Buffer::new(&mut font_system, metrics);
                let buffer_height = text_draw.rect.height.max(metrics.line_height);
                buffer.set_size(&mut font_system, f32::MAX, buffer_height);
                let attrs = match preferred_font {
                    Some(font) => Attrs::new()
                        .family(Family::Name(&font.family))
                        .weight(font.weight),
                    None => Attrs::new().family(Family::SansSerif),
                };
                buffer.set_text(&mut font_system, &text_draw.text, attrs, Shaping::Advanced);
                buffer.shape_until_scroll(&mut font_system);

                log_text_glyphs(&font_system, &buffer, text_draw, text_scale);

                let to_u8 = |value: f32| -> u8 { (value.clamp(0.0, 1.0) * 255.0).round() as u8 };

                let color = GlyphonColor::rgba(
                    to_u8(text_draw.color.r()),
                    to_u8(text_draw.color.g()),
                    to_u8(text_draw.color.b()),
                    to_u8(text_draw.color.a()),
                );

                text_buffers.push(buffer);
                text_layout.push((text_draw.rect.x, text_draw.rect.y, bounds, color));
            }

            let prepared_texts = text_buffers.len();
            if total_texts > 0 {
                log::info!(
                    "GPU text prepare: prepared {} of {} draw(s) (skipped {} zero scale, {} invalid size, {} clipped)",
                    prepared_texts,
                    total_texts,
                    skipped_zero_scale,
                    skipped_invalid_size,
                    skipped_invalid_clip
                );
                if prepared_texts == 0 {
                    log::warn!(
                        "GPU text renderer prepared zero draws; text will not appear this frame"
                    );
                } else if log::log_enabled!(log::Level::Debug) {
                    log::debug!(
                        "Preparing {} text area(s) for resolution {}x{}",
                        prepared_texts,
                        width,
                        height
                    );
                }
            }

            let text_areas: Vec<TextArea> = text_buffers
                .iter()
                .zip(text_layout.iter())
                .map(|(buffer, &(left, top, bounds, color))| TextArea {
                    buffer,
                    left,
                    top,
                    scale: 1.0,
                    bounds,
                    default_color: color,
                })
                .collect();

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
    let mut glyph_total = 0usize;
    let mut run_count = 0usize;
    let mut fonts: BTreeMap<fontdb::ID, usize> = BTreeMap::new();

    for run in buffer.layout_runs() {
        run_count += 1;
        glyph_total += run.glyphs.len();
        for glyph in run.glyphs.iter() {
            *fonts.entry(glyph.font_id).or_insert(0) += 1;
        }
    }

    let font_db = font_system.db();
    let font_details: Vec<String> = fonts
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

    let fonts_label = if font_details.is_empty() {
        String::from("none")
    } else {
        font_details.join(", ")
    };

    let text_snippet = info_text_preview(&text_draw.text);

    if glyph_total == 0 {
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
    } else {
        log::info!(
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
