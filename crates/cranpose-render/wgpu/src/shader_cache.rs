//! Pipeline cache for RuntimeShader effects.
//!
//! Compiles and caches `wgpu::RenderPipeline` objects keyed by WGSL source hash,
//! so the same shader with different uniform values reuses its pipeline.

use naga::ShaderStage;
use std::collections::{HashMap, HashSet};

/// Caches compiled render pipelines for custom WGSL shader effects.
pub(crate) struct ShaderPipelineCache {
    cache: HashMap<u64, wgpu::RenderPipeline>,
    disabled: HashSet<u64>,
}

impl ShaderPipelineCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            disabled: HashSet::new(),
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
    ) -> Option<&wgpu::RenderPipeline> {
        if self.disabled.contains(&source_hash) {
            return None;
        }

        if self.cache.contains_key(&source_hash) {
            return self.cache.get(&source_hash);
        }

        if let Err(err) = validate_runtime_shader_source(source) {
            log::warn!(
                "Disabling RuntimeShader (hash={}): {}. Falling back to pass-through.",
                source_hash,
                err
            );
            self.disabled.insert(source_hash);
            return None;
        }

        let pipeline = {
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
        };

        self.cache.insert(source_hash, pipeline);
        self.cache.get(&source_hash)
    }
}

fn validate_runtime_shader_source(source: &str) -> Result<(), String> {
    let module =
        naga::front::wgsl::parse_str(source).map_err(|err| format!("WGSL parse error: {err}"))?;

    let has_fullscreen_vs = module
        .entry_points
        .iter()
        .any(|ep| ep.stage == ShaderStage::Vertex && ep.name == "fullscreen_vs");
    if !has_fullscreen_vs {
        return Err("missing required vertex entry point `fullscreen_vs`".to_string());
    }

    let has_effect_fs = module
        .entry_points
        .iter()
        .any(|ep| ep.stage == ShaderStage::Fragment && ep.name == "effect_fs");
    if !has_effect_fs {
        return Err("missing required fragment entry point `effect_fs`".to_string());
    }

    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .map_err(|err| format!("WGSL validation error: {err}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_runtime_shader_source;

    const VALID_SHADER: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn fullscreen_vs(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var output: VertexOutput;
    let x = f32(i32(vertex_index & 1u) * 2 - 1);
    let y = f32(i32(vertex_index >> 1u) * 2 - 1);
    output.uv = vec2<f32>(x * 0.5 + 0.5, 1.0 - (y * 0.5 + 0.5));
    output.position = vec4<f32>(x, y, 0.0, 1.0);
    return output;
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> u: array<vec4<f32>, 64>;

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(input_texture, input_sampler, input.uv);
}
"#;

    #[test]
    fn validator_accepts_valid_runtime_shader() {
        assert!(validate_runtime_shader_source(VALID_SHADER).is_ok());
    }

    #[test]
    fn validator_rejects_invalid_wgsl() {
        let invalid = "this is not wgsl";
        assert!(validate_runtime_shader_source(invalid).is_err());
    }

    #[test]
    fn validator_rejects_missing_required_entry_points() {
        let missing_effect_fs = r#"
@vertex
fn fullscreen_vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(i & 1u) * 2 - 1);
    let y = f32(i32(i >> 1u) * 2 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}
"#;
        assert!(validate_runtime_shader_source(missing_effect_fs).is_err());
    }
}
