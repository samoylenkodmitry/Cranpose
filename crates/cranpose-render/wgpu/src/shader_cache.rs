use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    sync::Arc,
};

use cranpose_ui_graphics::{FxBuildHasher, RuntimeShader, ShaderTarget};
use naga::ShaderStage;

use crate::{
    debug_toggles::DebugToggle,
    lazy_resource::LazyGpuResource,
    pipeline_compiler::{CompileLane, PipelineCompiler},
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum RuntimeShaderPipelineMode {
    Replace,
    PremultipliedSrcOver,
}

impl RuntimeShaderPipelineMode {
    /// The pipeline a shader draws through at `target`: a layer's own
    /// texture takes the shader's output as is, the page beneath composites
    /// it.
    pub(crate) fn for_target(target: ShaderTarget) -> Self {
        match target {
            ShaderTarget::Layer => Self::Replace,
            ShaderTarget::Page => Self::PremultipliedSrcOver,
        }
    }

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

/// How the pipeline a lookup returned fits the draw: the specialization it
/// asked for, the shader's general pipeline because the draw asked for
/// nothing more, or that general pipeline standing in while the
/// specialization compiles. The general pipeline lands on the same bytes,
/// shading every feature the specialization folded away.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShaderPipelineFit {
    Specialized,
    General,
    Fallback,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct PipelineKey {
    source: u64,
    overrides: u64,
    forced: u64,
    mode: RuntimeShaderPipelineMode,
    split: Option<(&'static str, ShaderDrawVariant)>,
}

impl PipelineKey {
    fn general(self) -> Self {
        Self {
            overrides: 0,
            split: None,
            ..self
        }
    }

    fn is_general(self) -> bool {
        self == self.general()
    }
}

struct ShaderSource {
    text: Arc<str>,
    module: LazyGpuResource<Option<wgpu::ShaderModule>>,
}

#[derive(Clone)]
struct PipelineFactory {
    device: wgpu::Device,
    pipeline_cache: Option<wgpu::PipelineCache>,
    layout: wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    backend: wgpu::Backend,
    #[cfg(test)]
    counters: Arc<BuildCounters>,
}

#[cfg(test)]
#[derive(Default)]
struct BuildCounters {
    modules: std::sync::atomic::AtomicUsize,
    pipelines: std::sync::atomic::AtomicUsize,
}

struct PipelineJob {
    factory: PipelineFactory,
    source: Arc<str>,
    key: PipelineKey,
    module: LazyGpuResource<Option<wgpu::ShaderModule>>,
    constants: Vec<(&'static str, f64)>,
    variant: ShaderDrawVariant,
}

impl PipelineJob {
    fn build(self) -> Option<wgpu::RenderPipeline> {
        let PipelineJob {
            factory,
            source,
            key,
            module,
            constants,
            variant,
        } = self;
        let module = module
            .get_or_init(factory.backend, || {
                #[cfg(test)]
                factory
                    .counters
                    .modules
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                create_runtime_shader_module(&factory.device, &source, key.source, factory.backend)
            })
            .as_ref()?;
        let mode = key.mode;
        #[cfg(test)]
        factory
            .counters
            .pipelines
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Some(crate::render::create_fullscreen_strip_pipeline(
            &factory.device,
            factory.pipeline_cache.as_ref(),
            &format!(
                "runtime-shader mode={mode:?} variant={variant:?} source={:016x} \
                 overrides={:016x} forced={:016x} constants={}",
                key.source,
                key.overrides,
                key.forced,
                constants.len(),
            ),
            "RuntimeShader Effect Pipeline",
            &factory.layout,
            module,
            "effect_fs",
            &constants,
            wgpu::ColorTargetState {
                format: factory.format,
                blend: Some(mode.blend_state()),
                write_mask: wgpu::ColorWrites::ALL,
            },
        ))
    }
}

/// The compiled pipelines of every runtime shader a frame has drawn or a
/// warm-up named, keyed by source, override set and draw. Specializations
/// compile on the background compiler while the shader's general pipeline
/// draws in their place; a general pipeline the frame needs before its
/// warm-up finished is waited for, or built on the spot when none was
/// queued.
pub(crate) struct ShaderPipelineCache {
    factory: PipelineFactory,
    compiler: PipelineCompiler,
    sources: HashMap<u64, ShaderSource, FxBuildHasher>,
    pipelines: HashMap<PipelineKey, LazyGpuResource<Option<wgpu::RenderPipeline>>, FxBuildHasher>,
    /// Pipelines a frame has asked for, queued on the demanded lane even
    /// when a warm-up of theirs already waits on the other.
    demanded: HashSet<PipelineKey, FxBuildHasher>,
    forced: Vec<&'static str>,
    forced_hash: u64,
}

impl ShaderPipelineCache {
    pub fn new(
        device: &wgpu::Device,
        compiler: PipelineCompiler,
        pipeline_cache: Option<wgpu::PipelineCache>,
        backend: wgpu::Backend,
        format: wgpu::TextureFormat,
        texture_bind_group_layout: &wgpu::BindGroupLayout,
        uniform_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Effect Pipeline Layout"),
            bind_group_layouts: &[
                Some(texture_bind_group_layout),
                Some(uniform_bind_group_layout),
            ],
            immediate_size: 0,
        });
        Self {
            factory: PipelineFactory {
                device: device.clone(),
                pipeline_cache,
                layout,
                format,
                backend,
                #[cfg(test)]
                counters: Arc::default(),
            },
            compiler,
            sources: HashMap::default(),
            pipelines: HashMap::default(),
            demanded: HashSet::default(),
            forced: Vec::new(),
            forced_hash: 0,
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
        forced: &[&'static str],
        source: &str,
        constants: &mut Vec<(&'static str, f64)>,
    ) {
        for flag in forced {
            if !source.contains(&format!("override {flag}:")) {
                continue;
            }
            match constants.iter_mut().find(|(name, _)| name == flag) {
                Some(constant) => constant.1 = 1.0,
                None => constants.push((flag, 1.0)),
            }
        }
    }

    fn key(
        &self,
        shader: &RuntimeShader,
        mode: RuntimeShaderPipelineMode,
        variant: ShaderDrawVariant,
    ) -> PipelineKey {
        let specialize = shader_specialization_enabled();
        PipelineKey {
            source: shader.source_hash(),
            overrides: if specialize {
                shader.overrides_hash()
            } else {
                0
            },
            forced: self.forced_hash,
            mode,
            split: shader
                .draw_split()
                .filter(|_| specialize)
                .zip(variant.constant())
                .map(|(name, _)| (name, variant)),
        }
    }

    fn job(&mut self, shader: &RuntimeShader, key: PipelineKey) -> PipelineJob {
        let source = self
            .sources
            .entry(key.source)
            .or_insert_with(|| ShaderSource {
                text: Arc::from(shader.source()),
                module: LazyGpuResource::new("runtime-shader module"),
            });
        let mut constants = if key.overrides == 0 {
            Vec::new()
        } else {
            shader.overrides().to_vec()
        };
        Self::force_declared_flags(&self.forced, &source.text, &mut constants);
        let variant = match key.split {
            Some((name, variant)) => {
                constants.push((name, variant.constant().unwrap_or(0.0)));
                variant
            }
            None => ShaderDrawVariant::Whole,
        };
        PipelineJob {
            factory: self.factory.clone(),
            source: Arc::clone(&source.text),
            key,
            module: source.module.clone(),
            constants,
            variant,
        }
    }

    fn slot(&mut self, key: PipelineKey) -> LazyGpuResource<Option<wgpu::RenderPipeline>> {
        self.pipelines
            .entry(key)
            .or_insert_with(|| LazyGpuResource::new("runtime-shader"))
            .clone()
    }

    fn ready(&self, key: PipelineKey) -> bool {
        self.pipelines
            .get(&key)
            .is_some_and(|slot| slot.get().is_some())
    }

    fn request(&mut self, shader: &RuntimeShader, key: PipelineKey, lane: CompileLane) {
        let queued = self.pipelines.contains_key(&key);
        let first_demand = lane == CompileLane::Demanded && self.demanded.insert(key);
        if queued && !first_demand {
            return;
        }
        let job = self.job(shader, key);
        self.slot(key)
            .queue(&self.compiler, lane, self.factory.backend, || job.build());
    }

    /// Queues the pipeline drawing `shader` whole for `mode` on the
    /// background compiler, overrides included, so its first draw finds the
    /// pipeline ready; a shader without overrides warms its general one.
    pub fn warm(&mut self, shader: &RuntimeShader, mode: RuntimeShaderPipelineMode) {
        if !self.compiler.is_active() {
            return;
        }
        let key = self.key(shader, mode, ShaderDrawVariant::Whole);
        self.request(shader, key, CompileLane::WarmUp);
    }

    /// The pipeline drawing `shader` as `variant`, or `None` when the shader
    /// failed validation. The fit says whether the draw got the
    /// specialization it asked for or the general pipeline standing in.
    pub fn get_or_create(
        &mut self,
        shader: &RuntimeShader,
        mode: RuntimeShaderPipelineMode,
        variant: ShaderDrawVariant,
    ) -> Option<(&wgpu::RenderPipeline, ShaderPipelineFit)> {
        let key = self.key(shader, mode, variant);
        let general = key.general();
        let (build, fit) = if self.ready(key)
            || key == general
            || !self.compiler.is_active()
            || !shader.specialization_exact()
        {
            let fit = if key.is_general() {
                ShaderPipelineFit::General
            } else {
                ShaderPipelineFit::Specialized
            };
            (key, fit)
        } else {
            self.request(shader, key, CompileLane::Demanded);
            (general, ShaderPipelineFit::Fallback)
        };
        if self.ready(build) {
            return self.pipelines[&build]
                .get()
                .and_then(Option::as_ref)
                .map(|pipeline| (pipeline, fit));
        }
        let job = self.job(shader, build);
        let backend = self.factory.backend;
        self.pipelines
            .entry(build)
            .or_insert_with(|| LazyGpuResource::new("runtime-shader"))
            .get_or_init(backend, || job.build())
            .as_ref()
            .map(|pipeline| (pipeline, fit))
    }
}

fn create_runtime_shader_module(
    device: &wgpu::Device,
    source: &str,
    source_hash: u64,
    backend: wgpu::Backend,
) -> Option<wgpu::ShaderModule> {
    if let Err(err) = validate_runtime_shader_source(source, backend) {
        log::warn!(
            "Disabling RuntimeShader (hash={source_hash}): {err}. Falling back to pass-through."
        );
        return None;
    }
    Some(device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("RuntimeShader Effect"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
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
#[path = "tests/shader_cache_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/shader_cache_target_tests.rs"]
mod target_tests;
