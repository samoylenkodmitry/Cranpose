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
    blur_uniform_buffer: wgpu::Buffer,
    blur_uniform_bind_group_layout: wgpu::BindGroupLayout,

    // Offset pipeline (compiled once)
    offset_pipeline: wgpu::RenderPipeline,
    offset_uniform_buffer: wgpu::Buffer,
    offset_uniform_bind_group_layout: wgpu::BindGroupLayout,

    // Blit pipeline for compositing offscreen targets to the surface
    blit_pipeline: wgpu::RenderPipeline,

    // Shared bind group layouts for effect texture + uniform access
    pub effect_texture_bind_group_layout: wgpu::BindGroupLayout,
    pub effect_uniform_bind_group_layout: wgpu::BindGroupLayout,

    // Shared effect uniform buffer for RuntimeShader (64 vec4s = 1024 bytes)
    pub effect_uniform_buffer: wgpu::Buffer,

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
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
            bind_group_layouts: &[&effect_texture_bind_group_layout],
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
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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

        // Create uniform buffers
        let blur_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Blur Uniform Buffer"),
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

        let effect_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Effect Uniform Buffer"),
            size: (RuntimeShader::MAX_UNIFORMS * std::mem::size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
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
            blur_uniform_buffer,
            blur_uniform_bind_group_layout,
            offset_pipeline,
            offset_uniform_buffer,
            offset_uniform_bind_group_layout,
            blit_pipeline,
            effect_texture_bind_group_layout,
            effect_uniform_bind_group_layout,
            effect_uniform_buffer,
            effect_sampler,
            surface_format,
        }
    }

    /// Apply a two-pass separable Gaussian blur to a source texture, writing
    /// the result to the destination texture view.
    ///
    /// Uses an intermediate offscreen target for the horizontal pass.
    /// Each pass is submitted separately to avoid the `queue.write_buffer`
    /// staging bug where the second uniform write overwrites the first.
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
        let width = source.width;
        let height = source.height;
        let tile_mode_value = tile_mode_uniform_value(tile_mode);

        // Acquire intermediate target for horizontal pass
        let intermediate = self.offscreen_pool.acquire(device, width, height);

        // === Horizontal blur pass (source → intermediate) ===
        // Each pass gets its own encoder+submit so uniform writes don't conflict.
        // queue.write_buffer is staged and only the last write to a given offset
        // survives until submission, so we must submit between passes.
        {
            let uniforms = BlurUniforms {
                direction: [1.0, 0.0],
                radius: [radius_x, radius_y],
                texture_size: [width as f32, height as f32],
                tile_mode: tile_mode_value,
                _padding: 0.0,
            };
            queue.write_buffer(&self.blur_uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

            let texture_bind_group = OffscreenPool::create_texture_bind_group(
                device,
                &self.effect_texture_bind_group_layout,
                source,
                &self.effect_sampler,
            );
            let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Blur H Uniform Bind Group"),
                layout: &self.blur_uniform_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.blur_uniform_buffer.as_entire_binding(),
                }],
            });

            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Blur Horizontal Encoder"),
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
                pass.set_bind_group(0, &texture_bind_group, &[]);
                pass.set_bind_group(1, &uniform_bind_group, &[]);
                pass.draw(0..4, 0..1);
            }
            queue.submit(std::iter::once(encoder.finish()));
        }

        // === Vertical blur pass (intermediate → dest) ===
        {
            let uniforms = BlurUniforms {
                direction: [0.0, 1.0],
                radius: [radius_x, radius_y],
                texture_size: [width as f32, height as f32],
                tile_mode: tile_mode_value,
                _padding: 0.0,
            };
            queue.write_buffer(&self.blur_uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

            let texture_bind_group = OffscreenPool::create_texture_bind_group(
                device,
                &self.effect_texture_bind_group_layout,
                &intermediate,
                &self.effect_sampler,
            );
            let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Blur V Uniform Bind Group"),
                layout: &self.blur_uniform_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.blur_uniform_buffer.as_entire_binding(),
                }],
            });

            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Blur Vertical Encoder"),
            });
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
                pass.set_bind_group(0, &texture_bind_group, &[]);
                pass.set_bind_group(1, &uniform_bind_group, &[]);
                pass.draw(0..4, 0..1);
            }
            queue.submit(std::iter::once(encoder.finish()));
        }

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
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Offset Uniform Bind Group"),
            layout: &self.offset_uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.offset_uniform_buffer.as_entire_binding(),
            }],
        });

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
            pass.set_bind_group(1, &uniform_bind_group, &[]);
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
        // Upload uniforms with injected effect layer rect at slot 62
        let mut padded = shader.uniforms_padded();
        padded[248] = layer_pixel_rect[0];
        padded[249] = layer_pixel_rect[1];
        padded[250] = layer_pixel_rect[2];
        padded[251] = layer_pixel_rect[3];
        queue.write_buffer(
            &self.effect_uniform_buffer,
            0,
            bytemuck::cast_slice(&padded),
        );

        // Get or compile pipeline
        let source_hash = shader.source_hash();
        let pipeline = self.shader_cache.get_or_create(
            device,
            source_hash,
            shader.source(),
            self.surface_format,
            &self.effect_texture_bind_group_layout,
            &self.effect_uniform_bind_group_layout,
        );

        // Create bind groups
        let texture_bind_group = OffscreenPool::create_texture_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            source,
            &self.effect_sampler,
        );

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Effect Uniform Bind Group"),
            layout: &self.effect_uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.effect_uniform_buffer.as_entire_binding(),
            }],
        });

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
            pass.set_bind_group(1, &uniform_bind_group, &[]);
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

    /// Composite an offscreen target onto a destination view using alpha blending.
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
        self.composite_to_view_scissored(device, queue, source, dest_view, load_op, None);
    }

    /// Composite an offscreen target onto a destination view using alpha blending
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
        TileMode::Decal => 1.0,
    }
}
