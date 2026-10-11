use std::borrow::Cow;

use crate::lazy_resource::LazyGpuResource;

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
/// parse per variant.
#[derive(Clone)]
pub(crate) struct SharedShader {
    device: wgpu::Device,
    backend: wgpu::Backend,
    label: &'static str,
    code: ShaderCode,
    module: LazyGpuResource<wgpu::ShaderModule>,
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
}

#[cfg(test)]
#[path = "tests/shared_shader_tests.rs"]
mod tests;
