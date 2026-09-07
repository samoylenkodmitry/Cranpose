use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
};

use cranpose_ui_graphics::RuntimeShader;
use naga::ShaderStage;

use crate::debug_toggles::DebugToggle;

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

static NO_SHADER_SPECIALIZATION: DebugToggle =
    DebugToggle::new("CRANPOSE_NO_SHADER_SPECIALIZATION");

pub(crate) fn shader_specialization_enabled() -> bool {
    !NO_SHADER_SPECIALIZATION.equals("1")
}

/// Which of a shader's draws a pipeline serves: the one draw, or the
/// interior and the rim of a shader that declared a draw split, each
/// compiled with the split override set to its number.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ShaderDrawVariant {
    Whole,
    Interior,
    Rim,
}

impl ShaderDrawVariant {
    fn constant(self) -> Option<f64> {
        match self {
            Self::Whole => None,
            Self::Interior => Some(1.0),
            Self::Rim => Some(2.0),
        }
    }
}

type PipelineKey = (
    u64,
    u64,
    u64,
    RuntimeShaderPipelineMode,
    Option<(&'static str, ShaderDrawVariant)>,
);

pub(crate) struct ShaderPipelineCache {
    backend: wgpu::Backend,
    cache: HashMap<PipelineKey, wgpu::RenderPipeline>,
    disabled: HashSet<u64>,
    pipeline_cache: Option<wgpu::PipelineCache>,
    forced: Vec<&'static str>,
    forced_hash: u64,
}

impl ShaderPipelineCache {
    pub fn new(backend: wgpu::Backend, pipeline_cache: Option<wgpu::PipelineCache>) -> Self {
        Self {
            backend,
            cache: HashMap::new(),
            disabled: HashSet::new(),
            forced: Vec::new(),
            forced_hash: 0,
            pipeline_cache,
        }
    }

    pub fn set_forced_flags(&mut self, flags: impl Iterator<Item = &'static str>) {
        let mut forced: Vec<&'static str> = flags.collect();
        forced.sort_unstable();
        if forced == self.forced {
            return;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        forced.hash(&mut hasher);
        self.forced_hash = if forced.is_empty() {
            0
        } else {
            hasher.finish()
        };
        self.forced = forced;
    }

    fn force_declared_flags(
        &self,
        shader: &RuntimeShader,
        constants: &mut Vec<(&'static str, f64)>,
    ) {
        let source = shader.source();
        for flag in &self.forced {
            if !source.contains(&format!("override {flag}:")) {
                continue;
            }
            match constants.iter_mut().find(|(name, _)| name == flag) {
                Some(constant) => constant.1 = 1.0,
                None => constants.push((flag, 1.0)),
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn get_or_create(
        &mut self,
        device: &wgpu::Device,
        shader: &RuntimeShader,
        format: wgpu::TextureFormat,
        texture_bind_group_layout: &wgpu::BindGroupLayout,
        uniform_bind_group_layout: &wgpu::BindGroupLayout,
        mode: RuntimeShaderPipelineMode,
        variant: ShaderDrawVariant,
    ) -> Option<&wgpu::RenderPipeline> {
        let source_hash = shader.source_hash();
        let mut constants: Vec<(&'static str, f64)> = if shader_specialization_enabled() {
            shader.overrides().to_vec()
        } else {
            Vec::new()
        };
        let overrides_hash = if constants.is_empty() {
            0
        } else {
            shader.overrides_hash()
        };
        self.force_declared_flags(shader, &mut constants);
        let split = shader.draw_split().zip(variant.constant());
        if let Some((name, value)) = split {
            constants.push((name, value));
        }
        let cache_key = (
            source_hash,
            overrides_hash,
            self.forced_hash,
            mode,
            split.map(|(name, _)| (name, variant)),
        );
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

            crate::render::create_fullscreen_strip_pipeline(
                device,
                self.pipeline_cache.as_ref(),
                &format!("runtime-shader mode={mode:?} variant={variant:?}"),
                "RuntimeShader Effect Pipeline",
                &pipeline_layout,
                &shader_module,
                "effect_fs",
                &constants,
                wgpu::ColorTargetState {
                    format,
                    blend: Some(mode.blend_state()),
                    write_mask: wgpu::ColorWrites::ALL,
                },
            )
        };

        self.cache.insert(cache_key, pipeline);
        self.cache.get(&cache_key)
    }
}

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

    let (module, module_info) = naga::back::pipeline_constants::process_overrides(
        module,
        module_info,
        Some((shader_stage, entry_point)),
        &naga::back::PipelineConstants::default(),
    )
    .map_err(|err| format!("override resolution failed for `{entry_point}`: {err}"))?;
    let mut writer = glsl::Writer::new(
        &mut glsl_source,
        &module,
        &module_info,
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
    use cranpose_ui_graphics::{
        GRADIENT_BLUR_WGSL, GRADIENT_CUT_MASK_WGSL, GRADIENT_FADE_DST_OUT_WGSL, LIQUID_GLASS_WGSL,
        ROUNDED_ALPHA_MASK_WGSL,
    };

    use super::validate_runtime_shader_source;
    use crate::pipeline::GPU_TEXT_BRUSH_EFFECT_SHADER;

    fn valid_shader() -> String {
        format!(
            "{}\n{}",
            cranpose_ui_graphics::RUNTIME_SHADER_PRELUDE_WGSL,
            r#"@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(input_texture, input_sampler, input.uv);
}
"#
        )
    }

    #[test]
    fn validator_accepts_valid_runtime_shader() {
        assert!(validate_runtime_shader_source(&valid_shader(), wgpu::Backend::Vulkan).is_ok());
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
            ("gradient_blur", GRADIENT_BLUR_WGSL),
            ("gradient_cut_mask", GRADIENT_CUT_MASK_WGSL),
            ("rounded_alpha_mask", ROUNDED_ALPHA_MASK_WGSL),
            ("gradient_fade_dst_out", GRADIENT_FADE_DST_OUT_WGSL),
            ("liquid_glass", LIQUID_GLASS_WGSL),
            ("gpu_text_brush_effect", GPU_TEXT_BRUSH_EFFECT_SHADER),
        ] {
            let result = validate_runtime_shader_source(source, wgpu::Backend::Gl);
            assert!(
                result.is_ok(),
                "{name} should remain GL-portable: {}",
                result.err().map(|e| e.to_string()).unwrap_or_default()
            );
        }
    }
}
