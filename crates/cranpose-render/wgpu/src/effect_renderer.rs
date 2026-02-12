//! Effect rendering infrastructure: blur, custom shaders, offscreen passes.
//!
//! This module ties together the offscreen pool, blur pipeline, and shader
//! pipeline cache to apply `RenderEffect`s to subtree-rendered textures.

use crate::offscreen::{OffscreenPool, OffscreenTarget};
use crate::shader_cache::ShaderPipelineCache;
use crate::shaders;
use cranpose_ui_graphics::{RenderEffect, RuntimeShader, TileMode};

/// Manages GPU resources for applying render effects (blur, custom shaders).
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

    // Blit pipeline for compositing offscreen targets to the surface
    blit_pipeline: wgpu::RenderPipeline,
    blit_uniform_buffer: wgpu::Buffer,
    blit_uniform_bind_group: wgpu::BindGroup,

    // Shared bind group layouts for effect texture + uniform access
    pub effect_texture_bind_group_layout: wgpu::BindGroupLayout,
    pub effect_uniform_bind_group_layout: wgpu::BindGroupLayout,

    // Shared effect uniform buffer for RuntimeShader (64 vec4s = 1024 bytes)
    pub effect_uniform_buffer: wgpu::Buffer,
    effect_uniform_bind_group: wgpu::BindGroup,

    // Sampler for effect textures
    pub effect_sampler: wgpu::Sampler,

    surface_format: wgpu::TextureFormat,
}

/// Blur uniform data matching the WGSL `BlurUniforms` struct.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct BlurUniforms {
    direction: [f32; 2],
    radius: [f32; 2],
    texture_size: [f32; 2],
    tile_mode: f32,
    _padding: f32,
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
}

/// Optional rounded-rectangle clip mask applied during fullscreen blit.
#[derive(Copy, Clone, Debug)]
pub(crate) struct RoundedCompositeMask {
    /// Clip rect in destination pixel coordinates: x, y, width, height.
    pub rect: [f32; 4],
    /// Corner radii in pixels: top_left, top_right, bottom_left, bottom_right.
    pub radii: [f32; 4],
}

impl EffectRenderer {
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
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
                    visibility: wgpu::ShaderStages::FRAGMENT,
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
            source: wgpu::ShaderSource::Wgsl(shaders::BLUR_SHADER.into()),
        });

        let blur_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Blur Pipeline Layout"),
            bind_group_layouts: &[
                &effect_texture_bind_group_layout,
                &blur_uniform_bind_group_layout,
            ],
            push_constant_ranges: &[],
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
            multiview: None,
            cache: None,
        });

        // Compile offset pipeline
        let offset_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Offset Shader"),
            source: wgpu::ShaderSource::Wgsl(shaders::OFFSET_SHADER.into()),
        });

        let offset_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Offset Pipeline Layout"),
                bind_group_layouts: &[
                    &effect_texture_bind_group_layout,
                    &offset_uniform_bind_group_layout,
                ],
                push_constant_ranges: &[],
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
            multiview: None,
            cache: None,
        });

        // Compile blit pipeline
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Blit Shader"),
            source: wgpu::ShaderSource::Wgsl(shaders::BLIT_SHADER.into()),
        });

        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Blit Pipeline Layout"),
            bind_group_layouts: &[
                &effect_texture_bind_group_layout,
                &blit_uniform_bind_group_layout,
            ],
            push_constant_ranges: &[],
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
            multiview: None,
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
        let effect_uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Effect Uniform Bind Group"),
            layout: &effect_uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: effect_uniform_buffer.as_entire_binding(),
            }],
        });

        let effect_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Effect Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Self {
            offscreen_pool: OffscreenPool::new(surface_format),
            shader_cache: ShaderPipelineCache::new(),
            blur_pipeline,
            blur_uniform_buffer_horizontal,
            blur_uniform_buffer_vertical,
            blur_uniform_bind_group_horizontal,
            blur_uniform_bind_group_vertical,
            offset_pipeline,
            offset_uniform_buffer,
            offset_uniform_bind_group,
            blit_pipeline,
            blit_uniform_buffer,
            blit_uniform_bind_group,
            effect_texture_bind_group_layout,
            effect_uniform_bind_group_layout,
            effect_uniform_buffer,
            effect_uniform_bind_group,
            effect_sampler,
            surface_format,
        }
    }

    /// Apply a two-pass separable Gaussian blur to a source texture, writing
    /// the result to the destination texture view.
    ///
    /// Uses an intermediate offscreen target for the horizontal pass and
    /// submits both passes together for lower command submission overhead.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_blur(
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
            );
            return;
        }

        // Acquire intermediate target for horizontal pass
        let intermediate = self.offscreen_pool.acquire(device, width, height);

        // Upload both pass uniforms up front and execute both passes in a single submit.
        let horizontal_uniforms = BlurUniforms {
            direction: [1.0, 0.0],
            radius: [radius_x, radius_y],
            texture_size: [width as f32, height as f32],
            tile_mode: tile_mode_value,
            _padding: 0.0,
        };
        let vertical_uniforms = BlurUniforms {
            direction: [0.0, 1.0],
            radius: [radius_x, radius_y],
            texture_size: [width as f32, height as f32],
            tile_mode: tile_mode_value,
            _padding: 0.0,
        };
        queue.write_buffer(
            &self.blur_uniform_buffer_horizontal,
            0,
            bytemuck::bytes_of(&horizontal_uniforms),
        );
        queue.write_buffer(
            &self.blur_uniform_buffer_vertical,
            0,
            bytemuck::bytes_of(&vertical_uniforms),
        );

        let source_texture_bind_group = OffscreenPool::create_texture_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            source,
            &self.effect_sampler,
        );
        let intermediate_texture_bind_group = OffscreenPool::create_texture_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            &intermediate,
            &self.effect_sampler,
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
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(&self.blur_pipeline);
            pass.set_bind_group(0, &source_texture_bind_group, &[]);
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
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(&self.blur_pipeline);
            pass.set_bind_group(0, &intermediate_texture_bind_group, &[]);
            pass.set_bind_group(1, &self.blur_uniform_bind_group_vertical, &[]);
            if let Some((x, y, w, h)) = scissor {
                pass.set_scissor_rect(x, y, w, h);
            }
            pass.draw(0..4, 0..1);
        }
        queue.submit(std::iter::once(encoder.finish()));

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
        queue.write_buffer(
            &self.offset_uniform_buffer,
            0,
            bytemuck::bytes_of(&uniforms),
        );

        let texture_bind_group = OffscreenPool::create_texture_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            source,
            &self.effect_sampler,
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
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            pass.set_pipeline(&self.offset_pipeline);
            pass.set_bind_group(0, &texture_bind_group, &[]);
            pass.set_bind_group(1, &self.offset_uniform_bind_group, &[]);
            pass.draw(0..4, 0..1);
        }
        queue.submit(std::iter::once(encoder.finish()));
    }

    /// Apply a custom RuntimeShader effect to a source texture.
    ///
    /// `layer_pixel_rect` is `[x, y, width, height]` of the effect layer in
    /// viewport pixels, injected at uniform slot 62 (indices 248..252) so the
    /// shader can compute correct dp→pixel scaling and local coordinates.
    pub fn apply_shader(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        shader: &RuntimeShader,
        layer_pixel_rect: [f32; 4],
    ) {
        // Upload uniforms with injected effect layer rect at slot 62.
        let mut padded = shader.uniforms_padded();
        let slot = RuntimeShader::RESERVED_UNIFORM_START;
        padded[slot] = layer_pixel_rect[0];
        padded[slot + 1] = layer_pixel_rect[1];
        padded[slot + 2] = layer_pixel_rect[2];
        padded[slot + 3] = layer_pixel_rect[3];
        queue.write_buffer(
            &self.effect_uniform_buffer,
            0,
            bytemuck::cast_slice(&padded),
        );

        // Get or compile pipeline
        let source_hash = shader.source_hash();
        let Some(pipeline) = self.shader_cache.get_or_create(
            device,
            source_hash,
            shader.source(),
            self.surface_format,
            &self.effect_texture_bind_group_layout,
            &self.effect_uniform_bind_group_layout,
        ) else {
            // Invalid or unsupported shader source: degrade gracefully by rendering
            // the original source texture without applying an effect.
            self.composite_to_view(
                device,
                queue,
                source,
                dest_view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            );
            return;
        };

        // Create bind groups
        let texture_bind_group = OffscreenPool::create_texture_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            source,
            &self.effect_sampler,
        );

        // Render pass
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Shader Effect Encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shader Effect Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dest_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &texture_bind_group, &[]);
            pass.set_bind_group(1, &self.effect_uniform_bind_group, &[]);
            pass.draw(0..4, 0..1);
        }
        queue.submit(std::iter::once(encoder.finish()));
    }

    /// Recursively apply a RenderEffect chain.
    ///
    /// For Chain effects, applies first then second using ping-pong offscreen targets.
    /// `layer_pixel_rect` is forwarded to shader effects for coordinate mapping.
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
                let intermediate = self.offscreen_pool.acquire(device, width, height);
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
    ) {
        self.composite_to_view_with_alpha(device, queue, source, dest_view, 1.0, load_op);
    }

    /// Composite an offscreen target with explicit alpha multiplication.
    pub fn composite_to_view_with_alpha(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
    ) {
        self.composite_to_view_scissored_with_alpha(
            device, queue, source, dest_view, alpha, load_op, None,
        );
    }

    /// Composite an offscreen target onto a destination view using premultiplied alpha blending
    /// with an optional scissor region.
    pub fn composite_to_view_scissored(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
    ) {
        self.composite_to_view_scissored_with_alpha(
            device, queue, source, dest_view, 1.0, load_op, scissor,
        );
    }

    /// Composite an offscreen target onto a destination view with an optional scissor region
    /// and explicit alpha multiplication.
    #[allow(clippy::too_many_arguments)]
    pub fn composite_to_view_scissored_with_alpha(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
    ) {
        self.composite_to_view_scissored_with_alpha_and_mask(
            device, queue, source, dest_view, alpha, load_op, scissor, None,
        );
    }

    /// Composite an offscreen target onto a destination view with optional
    /// scissor and optional rounded-rectangle clip mask.
    #[allow(clippy::too_many_arguments)]
    pub fn composite_to_view_scissored_with_alpha_and_mask(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        rounded_mask: Option<RoundedCompositeMask>,
    ) {
        let (mask_rect, mask_radii, mask_enabled) = if let Some(mask) = rounded_mask {
            (mask.rect, mask.radii, [1.0, 0.0, 0.0, 0.0])
        } else {
            (
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
            )
        };
        let uniforms = BlitUniforms {
            alpha: [alpha.clamp(0.0, 1.0), 0.0, 0.0, 0.0],
            mask_rect,
            mask_radii,
            mask_enabled,
        };
        queue.write_buffer(&self.blit_uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let texture_bind_group = OffscreenPool::create_texture_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            source,
            &self.effect_sampler,
        );

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Blit Composite Encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Blit Composite Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dest_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, &texture_bind_group, &[]);
            pass.set_bind_group(1, &self.blit_uniform_bind_group, &[]);
            if let Some((x, y, w, h)) = scissor {
                pass.set_scissor_rect(x, y, w, h);
            }
            pass.draw(0..4, 0..1);
        }
        queue.submit(std::iter::once(encoder.finish()));
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
