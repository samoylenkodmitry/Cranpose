use std::borrow::Cow;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Arc, OnceLock};

use crate::lazy_resource::LazyGpuResource;

#[cfg(not(target_arch = "wasm32"))]
static NO_SHADER_FOLD: crate::debug_toggles::DebugToggle =
    crate::debug_toggles::DebugToggle::new("CRANPOSE_NO_SHADER_FOLD");

#[cfg(not(target_arch = "wasm32"))]
static STAGES_FOLDED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The pipeline stages this process built specialized and folded, each
/// carrying only the code its override constants keep.
pub fn shader_stages_folded() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        STAGES_FOLDED.load(std::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

/// A pipeline's two stages and the override constants it is built with.
pub(crate) struct Stages<'c> {
    pub(crate) vertex: wgpu::ShaderModule,
    pub(crate) fragment: wgpu::ShaderModule,
    pub(crate) constants: &'c [(&'c str, f64)],
}

/// A shader's naga module and what validating it found, which each stage
/// is specialized from.
#[cfg(not(target_arch = "wasm32"))]
type Parsed = Option<(naga::Module, naga::valid::ModuleInfo)>;

/// A shader module's code: its WGSL, and on the web the GLSL templates of
/// its entry points, which a WebGL device builds its pipelines from without
/// running naga on the page.
#[derive(Clone, Copy)]
pub(crate) struct ShaderCode {
    wgsl: fn() -> Cow<'static, str>,
    #[cfg(all(target_arch = "wasm32", feature = "gl-templates"))]
    glsl: &'static [wgpu::hal::GlslTemplate],
}

impl ShaderCode {
    /// WGSL alone: written at run time, so no template covers it.
    pub(crate) const fn wgsl(wgsl: fn() -> Cow<'static, str>) -> Self {
        Self {
            wgsl,
            #[cfg(all(target_arch = "wasm32", feature = "gl-templates"))]
            glsl: &[],
        }
    }

    /// A built-in module: its WGSL, and the templates the build script
    /// wrote of it (`crate::gl_templates`).
    #[cfg(all(target_arch = "wasm32", feature = "gl-templates"))]
    pub(crate) const fn built_in(
        wgsl: fn() -> Cow<'static, str>,
        glsl: &'static [wgpu::hal::GlslTemplate],
    ) -> Self {
        Self { wgsl, glsl }
    }

    /// The module of the templates, on a WebGL device.
    #[cfg(all(target_arch = "wasm32", feature = "gl-templates"))]
    fn template_module(
        &self,
        device: &wgpu::Device,
        backend: wgpu::Backend,
        label: &str,
    ) -> Option<wgpu::ShaderModule> {
        if backend != wgpu::Backend::Gl || self.glsl.is_empty() {
            return None;
        }
        device.create_shader_module_glsl_templates(Some(label), self.glsl)
    }

    fn wgsl_module(&self, device: &wgpu::Device, label: &str) -> wgpu::ShaderModule {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl((self.wgsl)()),
        })
    }
}

/// One WGSL shader and the pipeline layout of every pipeline built from it.
/// The module is parsed once, by the first build that needs it, on whichever
/// thread: each pipeline keeps its module alive, and a module with override
/// constants keeps its parsed shader, so a module per pipeline would hold a
/// parse per variant. On a native device a pipeline with constants instead
/// takes its two stages specialized and folded from one naga parse (see
/// [`Self::stages`]): each holds only the code its constants keep.
#[derive(Clone)]
pub(crate) struct SharedShader {
    device: wgpu::Device,
    backend: wgpu::Backend,
    label: &'static str,
    code: ShaderCode,
    module: LazyGpuResource<wgpu::ShaderModule>,
    #[cfg(not(target_arch = "wasm32"))]
    parsed: Arc<OnceLock<Parsed>>,
    layout: wgpu::PipelineLayout,
}

impl SharedShader {
    pub(crate) fn new(
        device: &wgpu::Device,
        backend: wgpu::Backend,
        label: &'static str,
        code: ShaderCode,
        bind_group_layouts: &[Option<&wgpu::BindGroupLayout>],
    ) -> Self {
        Self {
            device: device.clone(),
            backend,
            label,
            code,
            module: LazyGpuResource::new(label),
            #[cfg(not(target_arch = "wasm32"))]
            parsed: Arc::default(),
            layout: device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts,
                immediate_size: 0,
            }),
        }
    }

    /// The shader's module, made on the first call.
    pub(crate) fn module(&self) -> &wgpu::ShaderModule {
        self.module.get_or_init(self.backend, || {
            #[cfg(all(target_arch = "wasm32", feature = "gl-templates"))]
            if let Some(module) = self
                .code
                .template_module(&self.device, self.backend, self.label)
            {
                return module;
            }
            self.code.wgsl_module(&self.device, self.label)
        })
    }

    pub(crate) fn layout(&self) -> &wgpu::PipelineLayout {
        &self.layout
    }

    /// The two stages of a pipeline with `constants`, whose entry points are
    /// `vertex` and `fragment`. On a native device each is specialized by
    /// the constants and folded on its own (see [`crate::shader_fold`]), so
    /// the driver compiles only the code they keep; on the web, or where
    /// naga refuses the shader, both share the module, which the device
    /// specializes.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(unused_variables, reason = "the web specializes the shared module")
    )]
    pub(crate) fn stages<'c>(
        &self,
        (vertex, fragment): (&str, &str),
        constants: &'c [(&'c str, f64)],
    ) -> Stages<'c> {
        #[cfg(not(target_arch = "wasm32"))]
        if !constants.is_empty()
            && !NO_SHADER_FOLD.equals("1")
            && let Some((vertex, fragment)) = self
                .folded(naga::ShaderStage::Vertex, vertex, constants)
                .zip(self.folded(naga::ShaderStage::Fragment, fragment, constants))
        {
            return Stages {
                vertex,
                fragment,
                constants: &[],
            };
        }
        let module = self.module().clone();
        Stages {
            vertex: module.clone(),
            fragment: module,
            constants,
        }
    }

    /// The shader parsed by naga once, by the first build that needs it.
    #[cfg(not(target_arch = "wasm32"))]
    fn parsed(&self) -> Option<&(naga::Module, naga::valid::ModuleInfo)> {
        self.parsed
            .get_or_init(|| {
                let source = (self.code.wgsl)();
                let module = naga::front::wgsl::parse_str(&source)
                    .inspect_err(|error| {
                        log::error!("[shader-fold] {} does not parse: {error}", self.label);
                    })
                    .ok()?;
                let info = naga::valid::Validator::new(
                    naga::valid::ValidationFlags::all(),
                    naga::valid::Capabilities::all(),
                )
                .validate(&module)
                .inspect_err(|error| {
                    log::error!("[shader-fold] {} does not validate: {error:?}", self.label);
                })
                .ok()?;
                Some((module, info))
            })
            .as_ref()
    }

    /// The `stage` of entry point `entry` with `constants`, specialized and
    /// folded into a module of its own.
    #[cfg(not(target_arch = "wasm32"))]
    fn folded(
        &self,
        stage: naga::ShaderStage,
        entry: &str,
        constants: &[(&str, f64)],
    ) -> Option<wgpu::ShaderModule> {
        let (module, info) = self.parsed()?;
        let constants: naga::back::PipelineConstants = constants
            .iter()
            .map(|&(name, value)| (name.to_string(), value))
            .collect();
        let (specialized, _) = naga::back::pipeline_constants::process_overrides(
            module,
            info,
            Some((stage, entry)),
            &constants,
        )
        .inspect_err(|error| {
            log::error!(
                "[shader-fold] {} {entry} does not specialize: {error:?}",
                self.label
            );
        })
        .ok()?;
        let mut specialized = specialized.into_owned();
        crate::shader_fold::fold(&mut specialized);
        STAGES_FOLDED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Some(
            self.device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(self.label),
                    source: wgpu::ShaderSource::Naga(Cow::Owned(specialized)),
                }),
        )
    }
}

#[cfg(test)]
#[path = "tests/shared_shader_tests.rs"]
mod tests;
