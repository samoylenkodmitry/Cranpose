//! Effect rendering infrastructure: blur, custom shaders, offscreen passes.
//!
//! This module ties together the offscreen pool, blur pipeline, and shader
//! pipeline cache to apply `RenderEffect`s to subtree-rendered textures.

use crate::offscreen::{OffscreenPool, OffscreenTarget};
use crate::shader_cache::ShaderPipelineCache;
use crate::shaders;
use cranpose_ui_graphics::{BlendMode, RenderEffect, RuntimeShader, TileMode};

use crate::gpu_stats::FrameStats;
use std::cell::Cell;

pub(crate) struct EffectRenderer {
    pub offscreen_pool: OffscreenPool,
    pub shader_cache: ShaderPipelineCache,

    // Blur pipeline (compiled once)
    blur_pipeline: wgpu::RenderPipeline,
    blur_uniform_buffer_horizontal: wgpu::Buffer,
    blur_uniform_buffer_vertical: wgpu::Buffer,
    blur_uniform_bind_group_horizontal: wgpu::BindGroup,
    blur_uniform_bind_group_vertical: wgpu::BindGroup,

    // Offset pipeline (compiled once)
    offset_pipeline: wgpu::RenderPipeline,
    offset_uniform_buffer: wgpu::Buffer,
    offset_uniform_bind_group: wgpu::BindGroup,

    // Blit pipelines for compositing offscreen targets to the surface
    blit_pipeline: wgpu::RenderPipeline,
    blit_pipeline_dst_out: wgpu::RenderPipeline,
    blit_uniform_buffer: wgpu::Buffer,
    blit_uniform_bind_group: wgpu::BindGroup,
    projective_blit_pipeline: wgpu::RenderPipeline,
    projective_blit_pipeline_dst_out: wgpu::RenderPipeline,
    projective_blit_uniform_buffer: wgpu::Buffer,
    projective_blit_uniform_bind_group: wgpu::BindGroup,
    projective_blit_vertex_buffer: wgpu::Buffer,

    // Shared bind group layouts for effect texture + uniform access
    pub effect_texture_bind_group_layout: wgpu::BindGroupLayout,
    pub effect_uniform_bind_group_layout: wgpu::BindGroupLayout,

    // Shared effect uniform buffer for RuntimeShader (64 vec4s = 1024 bytes)
    pub effect_uniform_buffer: wgpu::Buffer,
    effect_uniform_bind_group: wgpu::BindGroup,

    // Sampler for effect textures
    pub effect_linear_sampler: wgpu::Sampler,

    surface_format: wgpu::TextureFormat,

    // Debug counters (reset each frame by GpuRenderer)
    pub(crate) debug_submits: Cell<u32>,
    pub(crate) debug_blurs: Cell<u32>,
    pub(crate) debug_composites: Cell<u32>,
    pub(crate) debug_effects: Cell<u32>,
    pub(crate) debug_upload_bytes: Cell<u64>,
}

/// Blur uniform data matching the WGSL `BlurUniforms` struct.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct BlurUniforms {
    direction_and_radius: [f32; 4],
    texture_size_and_tile_mode: [f32; 4],
}

/// Offset uniform data matching the WGSL `OffsetUniforms` struct.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct OffsetUniforms {
    offset: [f32; 2],
    _padding: [f32; 2],
}

/// Blit uniform data matching the WGSL `BlitUniforms` struct.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct BlitUniforms {
    alpha: [f32; 4],
    mask_rect: [f32; 4],
    mask_radii: [f32; 4],
    mask_enabled: [f32; 4],
    sampling: [f32; 4],
    dest_viewport: [f32; 4],
    resolve_span: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct ProjectiveBlitUniforms {
    viewport: [f32; 2],
    source_size: [f32; 2],
    inverse_row0: [f32; 4],
    inverse_row1: [f32; 4],
    inverse_row2: [f32; 4],
    alpha: [f32; 4],
    sampling: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct ProjectiveBlitVertex {
    position: [f32; 2],
}

/// Optional rounded-rectangle clip mask applied during fullscreen blit.
#[derive(Copy, Clone, Debug)]
pub(crate) struct RoundedCompositeMask {
    /// Clip rect in destination pixel coordinates: x, y, width, height.
    pub rect: [f32; 4],
    /// Corner radii in pixels: top_left, top_right, bottom_left, bottom_right.
    pub radii: [f32; 4],
}

#[derive(Clone, Copy)]
struct CompositePassOptions {
    alpha: f32,
    load_op: wgpu::LoadOp<wgpu::Color>,
    scissor: Option<(u32, u32, u32, u32)>,
    rounded_mask: Option<RoundedCompositeMask>,
    blend_mode: BlendMode,
    dest_viewport: Option<(f32, f32, f32, f32)>,
    sample_mode: CompositeSampleMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompositeSampleMode {
    Linear,
    Box4,
}

impl EffectRenderer {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        adapter_backend: wgpu::Backend,
    ) -> Self {
        // Create shared bind group layouts
        let effect_texture_bind_group_layout = OffscreenPool::texture_bind_group_layout(device);
        let effect_uniform_bind_group_layout = OffscreenPool::uniform_bind_group_layout(device);

        // Blur-specific uniform layout
        let blur_uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Blur Uniform Bind Group Layout"),
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
            });

        // Offset-specific uniform layout
        let offset_uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Offset Uniform Bind Group Layout"),
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
            });

        // Blit-specific uniform layout
        let blit_uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Blit Uniform Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        // Compile blur pipeline
        let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Blur Shader"),
            source: wgpu::ShaderSource::Wgsl(shaders::blur_shader().into()),
        });

        let blur_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Blur Pipeline Layout"),
            bind_group_layouts: &[
                &effect_texture_bind_group_layout,
                &blur_uniform_bind_group_layout,
            ],
            immediate_size: 0,
        });

        let blur_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Blur Pipeline"),
            layout: Some(&blur_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &blur_shader,
                entry_point: Some("fullscreen_vs"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &blur_shader,
                entry_point: Some("blur_fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // Compile offset pipeline
        let offset_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Offset Shader"),
            source: wgpu::ShaderSource::Wgsl(shaders::offset_shader().into()),
        });

        let offset_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Offset Pipeline Layout"),
                bind_group_layouts: &[
                    &effect_texture_bind_group_layout,
                    &offset_uniform_bind_group_layout,
                ],
                immediate_size: 0,
            });

        let offset_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Offset Pipeline"),
            layout: Some(&offset_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &offset_shader,
                entry_point: Some("fullscreen_vs"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &offset_shader,
                entry_point: Some("offset_fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // Compile blit pipeline
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Blit Shader"),
            source: wgpu::ShaderSource::Wgsl(shaders::blit_shader().into()),
        });

        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Blit Pipeline Layout"),
            bind_group_layouts: &[
                &effect_texture_bind_group_layout,
                &blit_uniform_bind_group_layout,
            ],
            immediate_size: 0,
        });

        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Blit Pipeline"),
            layout: Some(&blit_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &blit_shader,
                entry_point: Some("fullscreen_vs"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader,
                entry_point: Some("blit_fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let blit_pipeline_dst_out =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Blit Pipeline DstOut"),
                layout: Some(&blit_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &blit_shader,
                    entry_point: Some("fullscreen_vs"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &blit_shader,
                    entry_point: Some("blit_fs"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState {
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
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });

        let projective_blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Projective Blit Shader"),
            source: wgpu::ShaderSource::Wgsl(shaders::projective_blit_shader().into()),
        });
        let projective_blit_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Projective Blit Pipeline Layout"),
                bind_group_layouts: &[
                    &effect_texture_bind_group_layout,
                    &blit_uniform_bind_group_layout,
                ],
                immediate_size: 0,
            });
        let projective_blit_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ProjectiveBlitVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            }],
        };
        let projective_blit_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Projective Blit Pipeline"),
                layout: Some(&projective_blit_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &projective_blit_shader,
                    entry_point: Some("projective_blit_vs"),
                    buffers: std::slice::from_ref(&projective_blit_vertex_layout),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &projective_blit_shader,
                    entry_point: Some("projective_blit_fs"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });
        let projective_blit_pipeline_dst_out =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Projective Blit Pipeline DstOut"),
                layout: Some(&projective_blit_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &projective_blit_shader,
                    entry_point: Some("projective_blit_vs"),
                    buffers: &[projective_blit_vertex_layout],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &projective_blit_shader,
                    entry_point: Some("projective_blit_fs"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState {
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
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });

        // Create uniform buffers.
        // Blur keeps independent horizontal/vertical buffers so both writes can
        // happen before a single submit without staging collisions.
        let blur_uniform_buffer_horizontal = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Blur Horizontal Uniform Buffer"),
            size: std::mem::size_of::<BlurUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let blur_uniform_buffer_vertical = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Blur Vertical Uniform Buffer"),
            size: std::mem::size_of::<BlurUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let offset_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Offset Uniform Buffer"),
            size: std::mem::size_of::<OffsetUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let blit_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Blit Uniform Buffer"),
            size: std::mem::size_of::<BlitUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let projective_blit_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Projective Blit Uniform Buffer"),
            size: std::mem::size_of::<ProjectiveBlitUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let projective_blit_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Projective Blit Vertex Buffer"),
            size: (std::mem::size_of::<ProjectiveBlitVertex>() * 4) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let effect_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Effect Uniform Buffer"),
            size: (RuntimeShader::MAX_UNIFORMS * std::mem::size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let blur_uniform_bind_group_horizontal =
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Blur Horizontal Uniform Bind Group"),
                layout: &blur_uniform_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: blur_uniform_buffer_horizontal.as_entire_binding(),
                }],
            });
        let blur_uniform_bind_group_vertical =
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Blur Vertical Uniform Bind Group"),
                layout: &blur_uniform_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: blur_uniform_buffer_vertical.as_entire_binding(),
                }],
            });
        let offset_uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Offset Uniform Bind Group"),
            layout: &offset_uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: offset_uniform_buffer.as_entire_binding(),
            }],
        });
        let blit_uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Blit Uniform Bind Group"),
            layout: &blit_uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: blit_uniform_buffer.as_entire_binding(),
            }],
        });
        let projective_blit_uniform_bind_group =
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Projective Blit Uniform Bind Group"),
                layout: &blit_uniform_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: projective_blit_uniform_buffer.as_entire_binding(),
                }],
            });
        let effect_uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Effect Uniform Bind Group"),
            layout: &effect_uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: effect_uniform_buffer.as_entire_binding(),
            }],
        });

        let effect_linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Effect Linear Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            offscreen_pool: OffscreenPool::new(device, surface_format),
            shader_cache: ShaderPipelineCache::new(adapter_backend),
            blur_pipeline,
            blur_uniform_buffer_horizontal,
            blur_uniform_buffer_vertical,
            blur_uniform_bind_group_horizontal,
            blur_uniform_bind_group_vertical,
            offset_pipeline,
            offset_uniform_buffer,
            offset_uniform_bind_group,
            blit_pipeline,
            blit_pipeline_dst_out,
            blit_uniform_buffer,
            blit_uniform_bind_group,
            projective_blit_pipeline,
            projective_blit_pipeline_dst_out,
            projective_blit_uniform_buffer,
            projective_blit_uniform_bind_group,
            projective_blit_vertex_buffer,
            effect_texture_bind_group_layout,
            effect_uniform_bind_group_layout,
            effect_uniform_buffer,
            effect_uniform_bind_group,
            effect_linear_sampler,
            surface_format,
            debug_submits: Cell::new(0),
            debug_blurs: Cell::new(0),
            debug_composites: Cell::new(0),
            debug_effects: Cell::new(0),
            debug_upload_bytes: Cell::new(0),
        }
    }

    fn sampler_for_mode(&self, _sample_mode: CompositeSampleMode) -> &wgpu::Sampler {
        &self.effect_linear_sampler
    }

    pub(crate) fn merge_and_reset_debug_counters(&self, stats: &FrameStats) {
        stats
            .submits
            .set(stats.submits.get() + self.debug_submits.get());
        stats
            .blur_passes
            .set(stats.blur_passes.get() + self.debug_blurs.get());
        stats
            .composite_passes
            .set(stats.composite_passes.get() + self.debug_composites.get());
        stats
            .effect_applies
            .set(stats.effect_applies.get() + self.debug_effects.get());
        stats.record_upload_bytes(self.debug_upload_bytes.get());
        self.debug_submits.set(0);
        self.debug_blurs.set(0);
        self.debug_composites.set(0);
        self.debug_effects.set(0);
        self.debug_upload_bytes.set(0);
    }

    fn write_buffer_at_zero_offset(
        &self,
        queue: &wgpu::Queue,
        buffer: &wgpu::Buffer,
        bytes: &[u8],
    ) {
        queue.write_buffer(buffer, 0, bytes);
        self.debug_upload_bytes.set(
            self.debug_upload_bytes
                .get()
                .saturating_add(bytes.len() as u64),
        );
    }

    /// Apply a two-pass separable Gaussian blur to a source texture, writing
    /// the result to the destination texture view.
    ///
    /// Uses an intermediate offscreen target for the horizontal pass and
    /// submits both passes together for lower command submission overhead.
    #[allow(clippy::too_many_arguments)]
    fn apply_blur(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        radius_x: f32,
        radius_y: f32,
        tile_mode: TileMode,
    ) {
        self.apply_blur_scissored(
            device, queue, source, dest_view, radius_x, radius_y, tile_mode, None,
        );
    }

    /// Apply a two-pass separable Gaussian blur to a source texture with an
    /// optional processing scissor in destination pixel coordinates.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_blur_scissored(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        radius_x: f32,
        radius_y: f32,
        tile_mode: TileMode,
        scissor: Option<(u32, u32, u32, u32)>,
    ) {
        let width = source.width;
        let height = source.height;
        let tile_mode_value = tile_mode_uniform_value(tile_mode);

        if radius_x <= 0.0 && radius_y <= 0.0 {
            self.composite_to_view(
                device,
                queue,
                source,
                dest_view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                CompositeSampleMode::Linear,
            );
            return;
        }

        // Acquire intermediate target for horizontal pass
        let intermediate = self.offscreen_pool.acquire(device, width, height, None);

        // Upload both pass uniforms up front and execute both passes in a single submit.
        let horizontal_uniforms = BlurUniforms {
            direction_and_radius: [1.0, 0.0, radius_x, radius_y],
            texture_size_and_tile_mode: [width as f32, height as f32, tile_mode_value, 0.0],
        };
        let vertical_uniforms = BlurUniforms {
            direction_and_radius: [0.0, 1.0, radius_x, radius_y],
            texture_size_and_tile_mode: [width as f32, height as f32, tile_mode_value, 0.0],
        };
        self.write_buffer_at_zero_offset(
            queue,
            &self.blur_uniform_buffer_horizontal,
            bytemuck::bytes_of(&horizontal_uniforms),
        );
        self.write_buffer_at_zero_offset(
            queue,
            &self.blur_uniform_buffer_vertical,
            bytemuck::bytes_of(&vertical_uniforms),
        );

        let source_bind_group = source.get_or_create_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            &self.effect_linear_sampler,
        );
        let intermediate_bind_group = intermediate.get_or_create_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            &self.effect_linear_sampler,
        );

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Blur Effect Encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Blur Horizontal Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &intermediate.view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(&self.blur_pipeline);
            pass.set_bind_group(0, &*source_bind_group, &[]);
            pass.set_bind_group(1, &self.blur_uniform_bind_group_horizontal, &[]);
            if let Some((x, y, w, h)) = scissor {
                pass.set_scissor_rect(x, y, w, h);
            }
            pass.draw(0..4, 0..1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Blur Vertical Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dest_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(&self.blur_pipeline);
            pass.set_bind_group(0, &*intermediate_bind_group, &[]);
            pass.set_bind_group(1, &self.blur_uniform_bind_group_vertical, &[]);
            if let Some((x, y, w, h)) = scissor {
                pass.set_scissor_rect(x, y, w, h);
            }
            pass.draw(0..4, 0..1);
        }
        drop(source_bind_group);
        drop(intermediate_bind_group);
        queue.submit(std::iter::once(encoder.finish()));
        self.debug_submits.set(self.debug_submits.get() + 1);
        self.debug_blurs.set(self.debug_blurs.get() + 1);

        // Return intermediate to pool
        self.offscreen_pool.release(intermediate);
    }

    /// Apply a fixed pixel offset to a source texture.
    pub fn apply_offset(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        offset_x: f32,
        offset_y: f32,
    ) {
        let uniforms = OffsetUniforms {
            offset: [offset_x, offset_y],
            _padding: [0.0; 2],
        };
        self.write_buffer_at_zero_offset(
            queue,
            &self.offset_uniform_buffer,
            bytemuck::bytes_of(&uniforms),
        );

        let texture_bind_group = source.get_or_create_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            &self.effect_linear_sampler,
        );

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Offset Effect Encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Offset Effect Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dest_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            pass.set_pipeline(&self.offset_pipeline);
            pass.set_bind_group(0, &*texture_bind_group, &[]);
            pass.set_bind_group(1, &self.offset_uniform_bind_group, &[]);
            pass.draw(0..4, 0..1);
        }
        queue.submit(std::iter::once(encoder.finish()));
        self.debug_submits.set(self.debug_submits.get() + 1);
        self.debug_effects.set(self.debug_effects.get() + 1);
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_shader_pass(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        shader: &RuntimeShader,
        layer_pixel_rect: [f32; 4],
    ) -> bool {
        let mut padded = shader.uniforms_padded();
        let slot = RuntimeShader::RESERVED_UNIFORM_START;
        padded[slot] = layer_pixel_rect[0];
        padded[slot + 1] = layer_pixel_rect[1];
        padded[slot + 2] = layer_pixel_rect[2];
        padded[slot + 3] = layer_pixel_rect[3];
        self.write_buffer_at_zero_offset(
            queue,
            &self.effect_uniform_buffer,
            bytemuck::cast_slice(&padded),
        );

        let Some(pipeline) = self.shader_cache.get_or_create(
            device,
            shader,
            self.surface_format,
            &self.effect_texture_bind_group_layout,
            &self.effect_uniform_bind_group_layout,
        ) else {
            return false;
        };

        let texture_bind_group = source.get_or_create_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            &self.effect_linear_sampler,
        );

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Shader Effect Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: dest_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });

        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &*texture_bind_group, &[]);
        pass.set_bind_group(1, &self.effect_uniform_bind_group, &[]);
        pass.draw(0..4, 0..1);
        true
    }

    /// Apply a custom RuntimeShader effect to a source texture.
    ///
    /// `layer_pixel_rect` is `[x, y, width, height]` in the current effect
    /// input surface's pixel space, injected at uniform slot 62
    /// (indices 248..252) so the shader can compute correct dp->pixel scaling
    /// and local coordinates.
    pub fn apply_shader(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        shader: &RuntimeShader,
        layer_pixel_rect: [f32; 4],
    ) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Shader Effect Encoder"),
        });
        if !self.encode_shader_pass(
            device,
            queue,
            &mut encoder,
            source,
            dest_view,
            shader,
            layer_pixel_rect,
        ) {
            self.composite_to_view(
                device,
                queue,
                source,
                dest_view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                CompositeSampleMode::Linear,
            );
            return;
        }
        queue.submit(std::iter::once(encoder.finish()));
        self.debug_submits.set(self.debug_submits.get() + 1);
        self.debug_effects.set(self.debug_effects.get() + 1);
    }

    /// Recursively apply a RenderEffect chain.
    ///
    /// For Chain effects, applies first then second using ping-pong offscreen targets.
    /// `layer_pixel_rect` is forwarded to shader effects for coordinate mapping
    /// in the current input surface.
    pub fn apply_effect(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        effect: &RenderEffect,
        layer_pixel_rect: [f32; 4],
    ) {
        match effect {
            RenderEffect::Blur {
                radius_x,
                radius_y,
                edge_treatment,
            } => {
                self.apply_blur(
                    device,
                    queue,
                    source,
                    dest_view,
                    *radius_x,
                    *radius_y,
                    *edge_treatment,
                );
            }
            RenderEffect::Offset { offset_x, offset_y } => {
                self.apply_offset(device, queue, source, dest_view, *offset_x, *offset_y);
            }
            RenderEffect::Shader { shader } => {
                self.apply_shader(device, queue, source, dest_view, shader, layer_pixel_rect);
            }
            RenderEffect::Chain { first, second } => {
                // Apply first effect: source → intermediate
                let width = source.width;
                let height = source.height;
                let intermediate = self.offscreen_pool.acquire(device, width, height, None);
                self.apply_effect(
                    device,
                    queue,
                    source,
                    &intermediate.view,
                    first,
                    layer_pixel_rect,
                );

                // Apply second effect: intermediate → dest
                self.apply_effect(
                    device,
                    queue,
                    &intermediate,
                    dest_view,
                    second,
                    layer_pixel_rect,
                );

                self.offscreen_pool.release(intermediate);
            }
        }
    }

    /// Composite an offscreen target onto a destination view using premultiplied alpha blending.
    ///
    /// Uses a fullscreen quad blit with the blit pipeline. The load_op controls
    /// whether existing content is preserved (Load) or cleared (Clear).
    pub fn composite_to_view(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        load_op: wgpu::LoadOp<wgpu::Color>,
        sample_mode: CompositeSampleMode,
    ) {
        self.composite_to_view_scissored_with_alpha(
            device,
            queue,
            source,
            dest_view,
            1.0,
            load_op,
            None,
            sample_mode,
        );
    }

    fn encode_composite_to_view_pass(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        options: CompositePassOptions,
    ) {
        let (mask_rect, mask_radii, mask_enabled) = if let Some(mask) = options.rounded_mask {
            (mask.rect, mask.radii, [1.0, 0.0, 0.0, 0.0])
        } else {
            (
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
            )
        };
        let dest_viewport_uniform = options.dest_viewport.unwrap_or((0.0, 0.0, 0.0, 0.0));
        let resolve_span = options
            .dest_viewport
            .filter(|(_, _, width, height)| *width > 0.0 && *height > 0.0)
            .map(|(_, _, width, height)| {
                (source.width as f32 / width, source.height as f32 / height)
            })
            .unwrap_or((0.0, 0.0));
        let uniforms = BlitUniforms {
            alpha: [options.alpha.clamp(0.0, 1.0), 0.0, 0.0, 0.0],
            mask_rect,
            mask_radii,
            mask_enabled,
            sampling: [
                composite_sampling_mode_value(options.sample_mode),
                0.0,
                0.0,
                0.0,
            ],
            dest_viewport: [
                dest_viewport_uniform.0,
                dest_viewport_uniform.1,
                dest_viewport_uniform.2,
                dest_viewport_uniform.3,
            ],
            resolve_span: [resolve_span.0, resolve_span.1, 0.0, 0.0],
        };
        self.write_buffer_at_zero_offset(
            queue,
            &self.blit_uniform_buffer,
            bytemuck::bytes_of(&uniforms),
        );

        let sampler = self.sampler_for_mode(options.sample_mode);
        let texture_bind_group = source.get_or_create_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            sampler,
        );

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Blit Composite Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: dest_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: options.load_op,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });

        pass.set_pipeline(match options.blend_mode {
            BlendMode::DstOut => &self.blit_pipeline_dst_out,
            _ => &self.blit_pipeline,
        });
        pass.set_bind_group(0, &*texture_bind_group, &[]);
        pass.set_bind_group(1, &self.blit_uniform_bind_group, &[]);
        if let Some((x, y, w, h)) = options.scissor {
            pass.set_scissor_rect(x, y, w, h);
        }
        pass.draw(0..4, 0..1);
    }

    /// Composite an offscreen target onto a destination view with an optional scissor region
    /// and explicit alpha multiplication.
    #[allow(clippy::too_many_arguments)]
    fn composite_to_view_scissored_with_alpha(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        sample_mode: CompositeSampleMode,
    ) {
        self.composite_to_view_scissored_with_alpha_and_mask(
            device,
            queue,
            source,
            dest_view,
            alpha,
            load_op,
            scissor,
            None,
            sample_mode,
        );
    }

    /// Composite an offscreen target onto a destination view with optional
    /// scissor and optional rounded-rectangle clip mask.
    #[allow(clippy::too_many_arguments)]
    fn composite_to_view_scissored_with_alpha_and_mask(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        rounded_mask: Option<RoundedCompositeMask>,
        sample_mode: CompositeSampleMode,
    ) {
        self.composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
            device,
            queue,
            source,
            dest_view,
            alpha,
            load_op,
            scissor,
            rounded_mask,
            BlendMode::SrcOver,
            None,
            sample_mode,
        );
    }

    /// Composite an offscreen target onto a destination view with optional
    /// scissor and optional rounded-rectangle clip mask using an explicit blend mode.
    ///
    /// When `dest_viewport` is `Some((x, y, w, h))`, the fullscreen blit quad is
    /// mapped to that sub-region of the destination instead of covering the entire
    /// surface.  This allows a small source texture to be placed at the correct
    /// position in a larger destination.
    #[allow(clippy::too_many_arguments)]
    pub fn composite_to_view_scissored_with_alpha_and_mask_and_blend_mode(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
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
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Blit Composite Encoder"),
        });
        self.encode_composite_to_view_pass(
            device,
            queue,
            &mut encoder,
            source,
            dest_view,
            CompositePassOptions {
                alpha,
                load_op,
                scissor,
                rounded_mask,
                blend_mode,
                dest_viewport,
                sample_mode,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));
        self.debug_submits.set(self.debug_submits.get() + 1);
        self.debug_composites.set(self.debug_composites.get() + 1);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn apply_shader_and_composite_to_view_scissored_with_alpha_and_blend_mode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &OffscreenTarget,
        intermediate: &OffscreenTarget,
        shader: &RuntimeShader,
        layer_pixel_rect: [f32; 4],
        dest_view: &wgpu::TextureView,
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        blend_mode: BlendMode,
        dest_viewport: Option<(f32, f32, f32, f32)>,
        sample_mode: CompositeSampleMode,
    ) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Shader Effect Composite Encoder"),
        });
        let composite_source = if self.encode_shader_pass(
            device,
            queue,
            &mut encoder,
            source,
            &intermediate.view,
            shader,
            layer_pixel_rect,
        ) {
            self.debug_effects.set(self.debug_effects.get() + 1);
            intermediate
        } else {
            source
        };
        self.encode_composite_to_view_pass(
            device,
            queue,
            &mut encoder,
            composite_source,
            dest_view,
            CompositePassOptions {
                alpha,
                load_op,
                scissor,
                rounded_mask: None,
                blend_mode,
                dest_viewport,
                sample_mode,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));
        self.debug_submits.set(self.debug_submits.get() + 1);
        self.debug_composites.set(self.debug_composites.get() + 1);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn composite_to_view_projective(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
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
        let min_x = dest_bounds
            .iter()
            .map(|point| point[0])
            .fold(f32::INFINITY, f32::min);
        let min_y = dest_bounds
            .iter()
            .map(|point| point[1])
            .fold(f32::INFINITY, f32::min);
        let max_x = dest_bounds
            .iter()
            .map(|point| point[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let max_y = dest_bounds
            .iter()
            .map(|point| point[1])
            .fold(f32::NEG_INFINITY, f32::max);
        if !min_x.is_finite()
            || !min_y.is_finite()
            || !max_x.is_finite()
            || !max_y.is_finite()
            || max_x <= min_x
            || max_y <= min_y
        {
            return;
        }

        let vertices = [
            ProjectiveBlitVertex {
                position: [min_x, min_y],
            },
            ProjectiveBlitVertex {
                position: [max_x, min_y],
            },
            ProjectiveBlitVertex {
                position: [min_x, max_y],
            },
            ProjectiveBlitVertex {
                position: [max_x, max_y],
            },
        ];
        self.write_buffer_at_zero_offset(
            queue,
            &self.projective_blit_vertex_buffer,
            bytemuck::cast_slice(&vertices),
        );

        let uniforms = ProjectiveBlitUniforms {
            viewport: [viewport.0 as f32, viewport.1 as f32],
            source_size: [source_size.0.max(0.0), source_size.1.max(0.0)],
            inverse_row0: [
                inverse_matrix[0][0],
                inverse_matrix[0][1],
                inverse_matrix[0][2],
                0.0,
            ],
            inverse_row1: [
                inverse_matrix[1][0],
                inverse_matrix[1][1],
                inverse_matrix[1][2],
                0.0,
            ],
            inverse_row2: [
                inverse_matrix[2][0],
                inverse_matrix[2][1],
                inverse_matrix[2][2],
                0.0,
            ],
            alpha: [alpha.clamp(0.0, 1.0), 0.0, 0.0, 0.0],
            sampling: [composite_sampling_mode_value(sample_mode), 0.0, 0.0, 0.0],
        };
        self.write_buffer_at_zero_offset(
            queue,
            &self.projective_blit_uniform_buffer,
            bytemuck::bytes_of(&uniforms),
        );

        let sampler = self.sampler_for_mode(sample_mode);
        let texture_bind_group = source.get_or_create_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            sampler,
        );
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Projective Blit Encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Projective Blit Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dest_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(match blend_mode {
                BlendMode::DstOut => &self.projective_blit_pipeline_dst_out,
                _ => &self.projective_blit_pipeline,
            });
            pass.set_bind_group(0, &*texture_bind_group, &[]);
            pass.set_bind_group(1, &self.projective_blit_uniform_bind_group, &[]);
            pass.set_vertex_buffer(0, self.projective_blit_vertex_buffer.slice(..));
            if let Some((x, y, w, h)) = scissor {
                pass.set_scissor_rect(x, y, w, h);
            }
            pass.draw(0..4, 0..1);
        }
        queue.submit(std::iter::once(encoder.finish()));
        self.debug_submits.set(self.debug_submits.get() + 1);
        self.debug_composites.set(self.debug_composites.get() + 1);
    }
}

fn tile_mode_uniform_value(tile_mode: TileMode) -> f32 {
    match tile_mode {
        TileMode::Clamp => 0.0,
        TileMode::Repeated => 1.0,
        TileMode::Mirror => 2.0,
        TileMode::Decal => 3.0,
    }
}

fn composite_sampling_mode_value(sample_mode: CompositeSampleMode) -> f32 {
    match sample_mode {
        CompositeSampleMode::Linear => 0.0,
        CompositeSampleMode::Box4 => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::BlurUniforms;

    #[test]
    fn blur_uniforms_use_vec4_packing_for_gl_backends() {
        assert_eq!(std::mem::size_of::<BlurUniforms>(), 32);
        assert_eq!(std::mem::offset_of!(BlurUniforms, direction_and_radius), 0);
        assert_eq!(
            std::mem::offset_of!(BlurUniforms, texture_size_and_tile_mode),
            16
        );
    }
}
