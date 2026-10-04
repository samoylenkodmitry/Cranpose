use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    sync::Arc,
};

#[cfg(not(target_arch = "wasm32"))]
use cranpose_ui_graphics::runtime_shader_source_hash;
use cranpose_ui_graphics::{DrawSpecialization, FxBuildHasher, RuntimeShader, ShaderTarget};
use naga::ShaderStage;

#[cfg(not(target_arch = "wasm32"))]
use crate::pipeline_records::ShaderPipelineRecord;
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

    #[cfg(not(target_arch = "wasm32"))]
    fn disk_byte(self) -> u8 {
        match self {
            Self::Replace => 0,
            Self::PremultipliedSrcOver => 1,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn from_disk_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Replace),
            1 => Some(Self::PremultipliedSrcOver),
            _ => None,
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

    #[cfg(not(target_arch = "wasm32"))]
    fn disk_byte(self) -> u8 {
        match self {
            Self::Whole => 0,
            Self::Interior => 1,
            Self::Rim => 2,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn from_disk_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Whole),
            1 => Some(Self::Interior),
            2 => Some(Self::Rim),
            _ => None,
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
    module: LazyGpuResource<Option<RuntimeShaderModule>>,
}

impl ShaderSource {
    fn new(text: &str) -> Self {
        Self {
            text: Arc::from(text),
            module: LazyGpuResource::new("runtime-shader module"),
        }
    }
}

struct RuntimeShaderModule {
    module: wgpu::ShaderModule,
    position_independent: bool,
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

impl PipelineFactory {
    fn module(&self, source: &str, source_hash: u64) -> Option<RuntimeShaderModule> {
        #[cfg(test)]
        self.counters
            .modules
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        create_runtime_shader_module(&self.device, source, source_hash, self.backend)
    }
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
    module: LazyGpuResource<Option<RuntimeShaderModule>>,
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
            .get_or_init(factory.backend, || factory.module(&source, key.source))
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
            &module.module,
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
    #[cfg(not(target_arch = "wasm32"))]
    first_screen: FirstScreen,
}

/// The pipelines draws asked for while the first screen came up, noted for
/// the next launch to build before its first frame.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct FirstScreen {
    /// When the first draw asked, moved later by every pipeline a draw then
    /// waited to build, so a slow compile does not end the first screen.
    started: Option<web_time::Instant>,
    over: bool,
    noted: HashSet<PipelineKey, FxBuildHasher>,
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
            #[cfg(not(target_arch = "wasm32"))]
            first_screen: FirstScreen::default(),
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
        specialization: DrawSpecialization<'_>,
        mode: RuntimeShaderPipelineMode,
        variant: ShaderDrawVariant,
    ) -> PipelineKey {
        let specialize = shader_specialization_enabled();
        PipelineKey {
            source: shader.source_hash(),
            overrides: if specialize {
                specialization.overrides_hash()
            } else {
                0
            },
            forced: self.forced_hash,
            mode,
            split: specialization
                .draw_split()
                .filter(|_| specialize)
                .zip(variant.constant())
                .map(|(name, _)| (name, variant)),
        }
    }

    fn job(
        &mut self,
        shader: &RuntimeShader,
        specialization: DrawSpecialization<'_>,
        key: PipelineKey,
    ) -> PipelineJob {
        self.sources
            .entry(key.source)
            .or_insert_with(|| ShaderSource::new(shader.source()));
        let constants = if key.overrides == 0 {
            Vec::new()
        } else {
            specialization.overrides().to_vec()
        };
        self.source_job(key, constants)
    }

    /// The job building `key` from its registered source with `constants`,
    /// the overrides the key's hash names.
    fn source_job(&self, key: PipelineKey, mut constants: Vec<(&'static str, f64)>) -> PipelineJob {
        let source = &self.sources[&key.source];
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

    pub(crate) fn position_independent(&mut self, shader: &RuntimeShader) -> bool {
        if shader.position_independent() {
            return true;
        }
        let hash = shader.source_hash();
        let source = self
            .sources
            .entry(hash)
            .or_insert_with(|| ShaderSource::new(shader.source()));
        source
            .module
            .get_or_init(self.factory.backend, || {
                self.factory.module(&source.text, hash)
            })
            .as_ref()
            .is_some_and(|module| module.position_independent)
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

    fn request(
        &mut self,
        shader: &RuntimeShader,
        specialization: DrawSpecialization<'_>,
        key: PipelineKey,
        lane: CompileLane,
    ) {
        let queued = self.pipelines.contains_key(&key);
        let first_demand = lane == CompileLane::Demanded && self.demanded.insert(key);
        if queued && !first_demand {
            return;
        }
        let job = self.job(shader, specialization, key);
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
        let specialization = shader.draw_specialization(0);
        let key = self.key(shader, specialization, mode, ShaderDrawVariant::Whole);
        self.request(shader, specialization, key, CompileLane::WarmUp);
    }

    /// The pipeline drawing `shader` as `variant` with `specialization`, or
    /// `None` when the shader failed validation. The fit says whether the
    /// draw got the specialization it asked for or the general pipeline
    /// standing in.
    pub fn get_or_create(
        &mut self,
        shader: &RuntimeShader,
        specialization: DrawSpecialization<'_>,
        mode: RuntimeShaderPipelineMode,
        variant: ShaderDrawVariant,
    ) -> Option<(&wgpu::RenderPipeline, ShaderPipelineFit)> {
        let key = self.key(shader, specialization, mode, variant);
        #[cfg(not(target_arch = "wasm32"))]
        self.note_first_screen(specialization, key);
        let general = key.general();
        let (build, fit) = if self.ready(key)
            || key == general
            || !self.compiler.is_active()
            || !specialization.exact()
            || (self.pipelines.contains_key(&key) && !self.ready(general))
        {
            let fit = if key.is_general() {
                ShaderPipelineFit::General
            } else {
                ShaderPipelineFit::Specialized
            };
            (key, fit)
        } else {
            self.request(shader, specialization, key, CompileLane::Demanded);
            (general, ShaderPipelineFit::Fallback)
        };
        if !self.ready(build) {
            let job = self.job(shader, specialization, build);
            let backend = self.factory.backend;
            #[cfg(not(target_arch = "wasm32"))]
            let waited = web_time::Instant::now();
            self.slot(build).get_or_init(backend, || job.build());
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(started) = &mut self.first_screen.started {
                *started += waited.elapsed();
            }
        }
        self.pipelines[&build]
            .get()
            .and_then(Option::as_ref)
            .map(|pipeline| (pipeline, fit))
    }

    /// Notes `key` for the next launch while the first screen comes up.
    #[cfg(not(target_arch = "wasm32"))]
    fn note_first_screen(&mut self, specialization: DrawSpecialization<'_>, key: PipelineKey) {
        let first_screen = &mut self.first_screen;
        if first_screen.over || key.forced != 0 {
            return;
        }
        let started = *first_screen
            .started
            .get_or_insert_with(web_time::Instant::now);
        if started.elapsed() > crate::pipeline_disk_cache::FIRST_SCREEN_SPAN {
            first_screen.over = true;
            return;
        }
        if !first_screen.noted.insert(key) {
            return;
        }
        crate::pipeline_disk_cache::note_first_screen_shader(ShaderPipelineRecord {
            source: key.source,
            overrides: key.overrides,
            mode: key.mode.disk_byte(),
            variant: key
                .split
                .map_or(ShaderDrawVariant::Whole, |(_, variant)| variant)
                .disk_byte(),
            split: key.split.map(|(name, _)| name.to_owned()),
            constants: if key.overrides == 0 {
                Vec::new()
            } else {
                specialization
                    .overrides()
                    .iter()
                    .map(|&(name, value)| (name.to_owned(), value))
                    .collect()
            },
        });
    }

    /// Queues on the warm-up lane every pipeline `records` names whose shader
    /// source is one of `sources`, built as the draw that recorded it built
    /// it, so a launch's first frame finds the last launch's pipelines ready.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn warm_recorded(
        &mut self,
        records: &[ShaderPipelineRecord],
        sources: impl IntoIterator<Item = &'static str>,
    ) {
        if !self.compiler.is_active() || self.forced_hash != 0 || records.is_empty() {
            return;
        }
        let sources: smallvec::SmallVec<[(u64, &'static str); 8]> = sources
            .into_iter()
            .map(|text| (runtime_shader_source_hash(text), text))
            .collect();
        for record in records {
            let Some(&(_, text)) = sources.iter().find(|(hash, _)| *hash == record.source) else {
                continue;
            };
            let Some((key, constants)) = recorded_key(record, text) else {
                continue;
            };
            if self.pipelines.contains_key(&key) {
                continue;
            }
            self.sources
                .entry(key.source)
                .or_insert_with(|| ShaderSource::new(text));
            let job = self.source_job(key, constants);
            self.slot(key).queue(
                &self.compiler,
                CompileLane::WarmUp,
                self.factory.backend,
                || job.build(),
            );
        }
    }
}

/// The key `record` names and the overrides it compiled with, spelled as
/// `text` declares them, or `None` when `text` no longer declares one.
#[cfg(not(target_arch = "wasm32"))]
fn recorded_key(
    record: &ShaderPipelineRecord,
    text: &'static str,
) -> Option<(PipelineKey, Vec<(&'static str, f64)>)> {
    let variant = ShaderDrawVariant::from_disk_byte(record.variant)?;
    let split = match &record.split {
        Some(name) => Some((declared_override(text, name)?, variant)),
        None => None,
    };
    let constants = record
        .constants
        .iter()
        .map(|(name, value)| Some((declared_override(text, name)?, *value)))
        .collect::<Option<Vec<_>>>()?;
    let key = PipelineKey {
        source: record.source,
        overrides: record.overrides,
        forced: 0,
        mode: RuntimeShaderPipelineMode::from_disk_byte(record.mode)?,
        split,
    };
    Some((key, constants))
}

/// `name` as the `override` declaration in `text` spells it.
#[cfg(not(target_arch = "wasm32"))]
fn declared_override(text: &'static str, name: &str) -> Option<&'static str> {
    const OVERRIDE: &str = "override ";
    text.match_indices(OVERRIDE).find_map(|(at, _)| {
        let declared = &text[at + OVERRIDE.len()..];
        declared
            .strip_prefix(name)?
            .starts_with(':')
            .then(|| &declared[..name.len()])
    })
}

fn create_runtime_shader_module(
    device: &wgpu::Device,
    source: &str,
    source_hash: u64,
    backend: wgpu::Backend,
) -> Option<RuntimeShaderModule> {
    let position_independent = match validate_runtime_shader_source(source, backend) {
        Ok(independent) => independent,
        Err(err) => {
            log::warn!(
                "Disabling RuntimeShader (hash={source_hash}): {err}. Falling back to pass-through."
            );
            return None;
        }
    };
    Some(RuntimeShaderModule {
        module: device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("RuntimeShader Effect"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        }),
        position_independent,
    })
}

fn validate_runtime_shader_source(source: &str, backend: wgpu::Backend) -> Result<bool, String> {
    let module =
        naga::front::wgsl::parse_str(source).map_err(|err| format!("WGSL parse error: {err}"))?;

    let has_fullscreen_vs = module
        .entry_points
        .iter()
        .any(|ep| ep.stage == ShaderStage::Vertex && ep.name == "fullscreen_vs");
    if !has_fullscreen_vs {
        return Err("missing required vertex entry point `fullscreen_vs`".to_string());
    }

    let fragment = module
        .entry_points
        .iter()
        .position(|ep| ep.stage == ShaderStage::Fragment && ep.name == "effect_fs");
    let Some(fragment) = fragment else {
        return Err("missing required fragment entry point `effect_fs`".to_string());
    };

    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    let module_info = validator
        .validate(&module)
        .map_err(|err| format!("WGSL validation error: {err}"))?;

    validate_runtime_shader_backend_support(&module, &module_info, backend)?;

    Ok(fragment_position_independent(
        &module,
        &module_info,
        fragment,
    ))
}

fn is_position(binding: &Option<naga::Binding>) -> bool {
    matches!(
        binding,
        Some(naga::Binding::BuiltIn(naga::BuiltIn::Position { .. }))
    )
}

fn fragment_position_independent(
    module: &naga::Module,
    info: &naga::valid::ModuleInfo,
    fragment: usize,
) -> bool {
    let function = &module.entry_points[fragment].function;
    let info = info.get_entry_point(fragment);
    function.expressions.iter().all(|(handle, expression)| {
        let naga::Expression::FunctionArgument(index) = expression else {
            return true;
        };
        let argument = &function.arguments[*index as usize];
        if is_position(&argument.binding) {
            return info[handle].ref_count == 0;
        }
        let naga::TypeInner::Struct { members, .. } = &module.types[argument.ty].inner else {
            return true;
        };
        if !members.iter().any(|member| is_position(&member.binding)) {
            return true;
        }
        let safe_uses = function
            .expressions
            .iter()
            .filter(|(_, expression)| {
                matches!(expression,
                naga::Expression::AccessIndex { base, index }
                    if *base == handle && !is_position(&members[*index as usize].binding))
            })
            .count();
        safe_uses == info[handle].ref_count
    })
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
