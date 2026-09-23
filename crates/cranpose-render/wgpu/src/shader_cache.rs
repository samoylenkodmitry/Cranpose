use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Arc,
};

use cranpose_ui_graphics::{FxBuildHasher, RuntimeShader, ShaderTarget};
use naga::ShaderStage;

use crate::{
    debug_toggles::DebugToggle, lazy_resource::LazyGpuResource, pipeline_compiler::PipelineCompiler,
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

    fn request(&mut self, shader: &RuntimeShader, key: PipelineKey) {
        if self.pipelines.contains_key(&key) {
            return;
        }
        let job = self.job(shader, key);
        self.slot(key)
            .warm(&self.compiler, self.factory.backend, || job.build());
    }

    /// Queues the pipeline drawing `shader` whole for `mode` on the
    /// background compiler, overrides included, so its first draw finds the
    /// pipeline ready; a shader without overrides warms its general one.
    pub fn warm(&mut self, shader: &RuntimeShader, mode: RuntimeShaderPipelineMode) {
        if !self.compiler.is_active() {
            return;
        }
        let key = self.key(shader, mode, ShaderDrawVariant::Whole);
        self.request(shader, key);
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
            self.request(shader, key);
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
mod tests {
    use std::sync::atomic::Ordering;

    use cranpose_ui_graphics::{
        GRADIENT_BLUR_WGSL, GRADIENT_CUT_MASK_WGSL, GRADIENT_FADE_DST_OUT_WGSL, LIQUID_GLASS_WGSL,
        ROUNDED_ALPHA_MASK_WGSL, RuntimeShader,
    };
    use web_time::{Duration, Instant};

    use super::{
        RuntimeShaderPipelineMode, ShaderDrawVariant, ShaderPipelineCache, ShaderPipelineFit,
        validate_runtime_shader_source,
    };
    use crate::{
        effect_renderer::EffectRenderer, pipeline::GPU_TEXT_BRUSH_EFFECT_SHADER,
        pipeline_compiler::PipelineCompiler,
    };

    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
    const SETTLE: Duration = Duration::from_secs(30);

    fn cache(device: &wgpu::Device, compiler: PipelineCompiler) -> ShaderPipelineCache {
        let renderer = EffectRenderer::new(
            device,
            PipelineCompiler::inactive(),
            None,
            FORMAT,
            device.adapter_info().backend,
        );
        ShaderPipelineCache::new(
            device,
            compiler,
            None,
            device.adapter_info().backend,
            FORMAT,
            &renderer.effect_texture_bind_group_layout,
            &renderer.effect_uniform_bind_group_layout,
        )
    }

    fn split_shader() -> RuntimeShader {
        let mut shader = RuntimeShader::new(&format!(
            "{}\noverride RED: bool = false;\noverride SPLIT: i32 = 0;",
            valid_shader()
        ));
        shader.set_override("RED", 0.0);
        shader.set_draw_split(Some("SPLIT"));
        shader.set_specialization_exact(true);
        shader
    }

    fn builds(cache: &ShaderPipelineCache) -> (usize, usize) {
        (
            cache.factory.counters.modules.load(Ordering::Relaxed),
            cache.factory.counters.pipelines.load(Ordering::Relaxed),
        )
    }

    fn settle(cache: &mut ShaderPipelineCache, shader: &RuntimeShader) {
        let deadline = Instant::now() + SETTLE;
        loop {
            let pending = [
                ShaderDrawVariant::Whole,
                ShaderDrawVariant::Interior,
                ShaderDrawVariant::Rim,
            ]
            .into_iter()
            .any(|variant| {
                let (_, fit) = cache
                    .get_or_create(shader, RuntimeShaderPipelineMode::Replace, variant)
                    .expect("valid shader");
                fit == ShaderPipelineFit::Fallback
            });
            if !pending {
                return;
            }
            assert!(Instant::now() < deadline, "specializations never landed");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn warm_pipeline_lookups_do_not_rebuild_constants() {
        let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
        let mut cache = cache(&device, PipelineCompiler::inactive());
        let shader = split_shader();
        for forced in [false, true, false] {
            cache.set_forced_flags(forced.then_some("RED").into_iter());
            for _ in 0..12 {
                for variant in [
                    ShaderDrawVariant::Whole,
                    ShaderDrawVariant::Interior,
                    ShaderDrawVariant::Rim,
                ] {
                    let (_, fit) = cache
                        .get_or_create(&shader, RuntimeShaderPipelineMode::Replace, variant)
                        .expect("valid shader");
                    assert_eq!(fit, ShaderPipelineFit::Specialized);
                }
            }
        }
        assert_eq!(cache.pipelines.len(), 6);
        assert_eq!(builds(&cache), (1, 6), "one module, one build per variant");
    }

    #[test]
    fn a_specialization_draws_with_the_general_pipeline_until_it_lands() {
        let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
        let mut cache = cache(&device, PipelineCompiler::spawn());
        let shader = split_shader();
        let mode = RuntimeShaderPipelineMode::Replace;
        let (general, fit) = cache
            .get_or_create(&shader, mode, ShaderDrawVariant::Interior)
            .expect("valid shader");
        assert_eq!(fit, ShaderPipelineFit::Fallback);
        let general = general.clone();
        let (again, fit) = cache
            .get_or_create(&shader, mode, ShaderDrawVariant::Rim)
            .expect("valid shader");
        assert_eq!(fit, ShaderPipelineFit::Fallback);
        assert!(*again == general, "both draws share the general pipeline");
        settle(&mut cache, &shader);
        for variant in [ShaderDrawVariant::Interior, ShaderDrawVariant::Rim] {
            let (specialized, fit) = cache
                .get_or_create(&shader, mode, variant)
                .expect("valid shader");
            assert_eq!(fit, ShaderPipelineFit::Specialized);
            assert!(*specialized != general, "{variant:?} has its own pipeline");
        }
        let (whole, fit) = cache
            .get_or_create(&shader, mode, ShaderDrawVariant::Whole)
            .expect("valid shader");
        assert_eq!(
            fit,
            ShaderPipelineFit::Specialized,
            "an override set is a specialization"
        );
        assert!(*whole != general);
        assert_eq!(
            builds(&cache),
            (1, 4),
            "the general pipeline plus three variants"
        );
    }

    #[test]
    fn an_override_that_is_not_a_fold_compiles_inside_the_frame() {
        let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
        let mut cache = cache(&device, PipelineCompiler::spawn());
        let mut shader = split_shader();
        shader.set_specialization_exact(false);
        let (_, fit) = cache
            .get_or_create(
                &shader,
                RuntimeShaderPipelineMode::Replace,
                ShaderDrawVariant::Interior,
            )
            .expect("valid shader");
        assert_eq!(fit, ShaderPipelineFit::Specialized);
        assert_eq!(
            builds(&cache),
            (1, 1),
            "the requested variant itself was built, and nothing else"
        );
    }

    #[test]
    fn a_warmed_general_pipeline_is_ready_before_its_first_draw() {
        let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
        let mut cache = cache(&device, PipelineCompiler::spawn());
        let shader = RuntimeShader::new(&valid_shader());
        let mode = RuntimeShaderPipelineMode::PremultipliedSrcOver;
        cache.warm(&shader, mode);
        cache.warm(&shader, mode);
        let deadline = Instant::now() + SETTLE;
        while !cache.ready(cache.key(&shader, mode, ShaderDrawVariant::Whole)) {
            assert!(Instant::now() < deadline, "the warm-up never finished");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(builds(&cache), (1, 1), "one warm-up per general pipeline");
        let (_, fit) = cache
            .get_or_create(&shader, mode, ShaderDrawVariant::Whole)
            .expect("valid shader");
        assert_eq!(fit, ShaderPipelineFit::General);
        assert_eq!(
            builds(&cache),
            (1, 1),
            "the draw found the warm-up's pipeline"
        );
    }

    #[test]
    fn an_invalid_shader_disables_every_variant_once() {
        let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
        let mut cache = cache(&device, PipelineCompiler::spawn());
        let mut shader = RuntimeShader::new("this is not wgsl");
        shader.set_override("RED", 1.0);
        let mode = RuntimeShaderPipelineMode::Replace;
        assert!(
            cache
                .get_or_create(&shader, mode, ShaderDrawVariant::Whole)
                .is_none()
        );
        assert!(
            cache
                .get_or_create(&shader, mode, ShaderDrawVariant::Whole)
                .is_none()
        );
        assert_eq!(
            builds(&cache),
            (1, 0),
            "validation ran once and built nothing"
        );
    }

    fn valid_shader() -> String {
        format!(
            "{}\n{}",
            cranpose_ui_graphics::RUNTIME_SHADER_PRELUDE_WGSL,
            r"@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(input_texture, input_sampler, input.uv);
}
"
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
        let missing_effect_fs = r"
@vertex
fn fullscreen_vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(i & 1u) * 2 - 1);
    let y = f32(i32(i >> 1u) * 2 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}
";
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
                result.err().unwrap_or_default()
            );
        }
    }
}

#[cfg(test)]
mod target_tests {
    use cranpose_ui_graphics::ShaderTarget;

    use super::RuntimeShaderPipelineMode;

    #[test]
    fn a_layer_replaces_and_a_page_composites() {
        assert_eq!(
            RuntimeShaderPipelineMode::for_target(ShaderTarget::Layer),
            RuntimeShaderPipelineMode::Replace
        );
        assert_eq!(
            RuntimeShaderPipelineMode::for_target(ShaderTarget::Page),
            RuntimeShaderPipelineMode::PremultipliedSrcOver
        );
    }
}
