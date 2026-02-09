//! Pipeline cache for RuntimeShader effects.
//!
//! Compiles and caches `wgpu::RenderPipeline` objects keyed by WGSL source hash,
//! so the same shader with different uniform values reuses its pipeline.

use std::collections::HashMap;

/// Caches compiled render pipelines for custom WGSL shader effects.
pub(crate) struct ShaderPipelineCache {
    cache: HashMap<u64, wgpu::RenderPipeline>,
}

impl ShaderPipelineCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Get or compile a render pipeline for the given WGSL source.
    ///
    /// The pipeline is cached by the source hash, so repeated calls with
    /// the same shader source (but potentially different uniforms) reuse
    /// the compiled pipeline.
    pub fn get_or_create(
        &mut self,
        device: &wgpu::Device,
        source_hash: u64,
        source: &str,
        format: wgpu::TextureFormat,
        texture_bind_group_layout: &wgpu::BindGroupLayout,
        uniform_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> &wgpu::RenderPipeline {
        self.cache.entry(source_hash).or_insert_with(|| {
            let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("RuntimeShader Effect"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });

            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Effect Pipeline Layout"),
                bind_group_layouts: &[texture_bind_group_layout, uniform_bind_group_layout],
                push_constant_ranges: &[],
            });

            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("RuntimeShader Effect Pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader_module,
                    entry_point: Some("fullscreen_vs"),
                    buffers: &[], // Fullscreen quad from vertex_index
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader_module,
                    entry_point: Some("effect_fs"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
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
            })
        })
    }
}
