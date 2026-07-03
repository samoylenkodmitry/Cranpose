//! Pipeline cache for RuntimeShader effects.
//!
//! Compiles and caches `wgpu::RenderPipeline` objects keyed by WGSL source hash,
//! so the same shader with different uniform values reuses its pipeline.

use cranpose_ui_graphics::RuntimeShader;
use naga::ShaderStage;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum RuntimeShaderPipelineMode {
    Replace,
    PremultipliedSrcOver,
}

impl RuntimeShaderPipelineMode {
    fn blend_state(self) -> wgpu::BlendState {
        match self {
            Self::Replace => wgpu::BlendState::REPLACE,
            Self::PremultipliedSrcOver => wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        }
    }
}

/// Caches compiled render pipelines for custom WGSL shader effects.
pub(crate) struct ShaderPipelineCache {
    backend: wgpu::Backend,
    cache: HashMap<(u64, RuntimeShaderPipelineMode), wgpu::RenderPipeline>,
    disabled: HashSet<u64>,
}

impl ShaderPipelineCache {
    pub fn new(backend: wgpu::Backend) -> Self {
        Self {
            backend,
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
        shader: &RuntimeShader,
        format: wgpu::TextureFormat,
        texture_bind_group_layout: &wgpu::BindGroupLayout,
        uniform_bind_group_layout: &wgpu::BindGroupLayout,
        mode: RuntimeShaderPipelineMode,
    ) -> Option<&wgpu::RenderPipeline> {
        let source_hash = shader.source_hash();
        let cache_key = (source_hash, mode);
        if self.disabled.contains(&source_hash) {
            return None;
        }

        if self.cache.contains_key(&cache_key) {
            return self.cache.get(&cache_key);
        }

        let Some(shader_module) = create_runtime_shader_module(device, shader, self.backend) else {
            self.disabled.insert(source_hash);
            return None;
        };

        let pipeline = {
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Effect Pipeline Layout"),
                bind_group_layouts: &[
                    Some(texture_bind_group_layout),
                    Some(uniform_bind_group_layout),
                ],
                immediate_size: 0,
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
                        blend: Some(mode.blend_state()),
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
            })
        };

        self.cache.insert(cache_key, pipeline);
        self.cache.get(&cache_key)
    }
}

/// Validate the user-provided source, then hand the WGSL to wgpu.
fn create_runtime_shader_module(
    device: &wgpu::Device,
    shader: &RuntimeShader,
    backend: wgpu::Backend,
) -> Option<wgpu::ShaderModule> {
    if let Err(err) = validate_runtime_shader_source(shader.source(), backend) {
        log::warn!(
            "Disabling RuntimeShader (hash={}): {}. Falling back to pass-through.",
            shader.source_hash(),
            err
        );
        return None;
    }
    Some(device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("RuntimeShader Effect"),
        source: wgpu::ShaderSource::Wgsl(shader.source().into()),
    }))
}

fn validate_runtime_shader_source(source: &str, backend: wgpu::Backend) -> Result<(), String> {
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
    let module_info = validator
        .validate(&module)
        .map_err(|err| format!("WGSL validation error: {err}"))?;

    validate_runtime_shader_backend_support(&module, &module_info, backend)?;

    Ok(())
}

fn validate_runtime_shader_backend_support(
    module: &naga::Module,
    module_info: &naga::valid::ModuleInfo,
    backend: wgpu::Backend,
) -> Result<(), String> {
    if backend != wgpu::Backend::Gl {
        return Ok(());
    }

    validate_glsl_portability(module, module_info, "fullscreen_vs", ShaderStage::Vertex)?;
    validate_glsl_portability(module, module_info, "effect_fs", ShaderStage::Fragment)
}

/// GLSL portability validation exists only when a GL-class backend can be
/// selected at runtime: native builds with the `backend-gles` feature, web
/// builds (WebGL fallback), and unit tests.
#[cfg(any(test, feature = "backend-gles", target_arch = "wasm32"))]
fn validate_glsl_portability(
    module: &naga::Module,
    module_info: &naga::valid::ModuleInfo,
    entry_point: &str,
    shader_stage: ShaderStage,
) -> Result<(), String> {
    use naga::back::glsl;

    let mut glsl_source = String::new();
    let options = glsl::Options {
        version: glsl::Version::new_gles(300),
        writer_flags: glsl::WriterFlags::ADJUST_COORDINATE_SPACE,
        ..Default::default()
    };
    let pipeline_options = glsl::PipelineOptions {
        shader_stage,
        entry_point: entry_point.to_string(),
        multiview: None,
    };

    let mut writer = glsl::Writer::new(
        &mut glsl_source,
        module,
        module_info,
        &options,
        &pipeline_options,
        naga::proc::BoundsCheckPolicies::default(),
    )
    .map_err(|err| format!("GL/WebGL portability validation failed for `{entry_point}`: {err}"))?;

    writer
        .write()
        .map(|_| ())
        .map_err(|err| format!("GL/WebGL portability emission failed for `{entry_point}`: {err}"))
}

/// Without `backend-gles`, wgpu cannot select the GL backend on native
/// targets, so this path is unreachable; reject defensively instead of
/// claiming portability that was never checked.
#[cfg(not(any(test, feature = "backend-gles", target_arch = "wasm32")))]
fn validate_glsl_portability(
    _module: &naga::Module,
    _module_info: &naga::valid::ModuleInfo,
    entry_point: &str,
    _shader_stage: ShaderStage,
) -> Result<(), String> {
    Err(format!(
        "GL backend active for `{entry_point}` but GL support is not compiled in \
         (enable `backend-gles`, or `renderer-wgpu-gles` on the `cranpose` crate)"
    ))
}

#[cfg(test)]
mod tests {
    use super::validate_runtime_shader_source;
    use crate::pipeline::GPU_TEXT_BRUSH_EFFECT_SHADER;
    use cranpose_ui_graphics::{
        GRADIENT_CUT_MASK_WGSL, GRADIENT_FADE_DST_OUT_WGSL, LIQUID_GLASS_WGSL,
        ROUNDED_ALPHA_MASK_WGSL,
    };

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
        assert!(validate_runtime_shader_source(VALID_SHADER, wgpu::Backend::Vulkan).is_ok());
    }

    #[test]
    fn validator_rejects_invalid_wgsl() {
        let invalid = "this is not wgsl";
        assert!(validate_runtime_shader_source(invalid, wgpu::Backend::Vulkan).is_err());
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
        assert!(validate_runtime_shader_source(missing_effect_fs, wgpu::Backend::Vulkan).is_err());
    }

    #[test]
    fn validator_accepts_gl_portable_builtin_runtime_shaders() {
        for (name, source) in [
            ("gradient_cut_mask", GRADIENT_CUT_MASK_WGSL),
            ("rounded_alpha_mask", ROUNDED_ALPHA_MASK_WGSL),
            ("gradient_fade_dst_out", GRADIENT_FADE_DST_OUT_WGSL),
            ("liquid_glass", LIQUID_GLASS_WGSL),
            ("gpu_text_brush_effect", GPU_TEXT_BRUSH_EFFECT_SHADER),
        ] {
            assert!(
                validate_runtime_shader_source(source, wgpu::Backend::Gl).is_ok(),
                "{name} should remain GL-portable"
            );
        }
    }
}
