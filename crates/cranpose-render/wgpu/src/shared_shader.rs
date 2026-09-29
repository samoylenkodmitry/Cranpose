use std::borrow::Cow;

use crate::lazy_resource::LazyGpuResource;

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
    source: fn() -> Cow<'static, str>,
    module: LazyGpuResource<wgpu::ShaderModule>,
    layout: wgpu::PipelineLayout,
}

impl SharedShader {
    pub(crate) fn new(
        device: &wgpu::Device,
        backend: wgpu::Backend,
        label: &'static str,
        source: fn() -> Cow<'static, str>,
        bind_group_layouts: &[Option<&wgpu::BindGroupLayout>],
    ) -> Self {
        Self {
            device: device.clone(),
            backend,
            label,
            source,
            module: LazyGpuResource::new(label),
            layout: device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts,
                immediate_size: 0,
            }),
        }
    }

    /// The shader's module, parsed on the first call.
    pub(crate) fn module(&self) -> &wgpu::ShaderModule {
        self.module.get_or_init(self.backend, || {
            self.device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(self.label),
                    source: wgpu::ShaderSource::Wgsl((self.source)()),
                })
        })
    }

    pub(crate) fn layout(&self) -> &wgpu::PipelineLayout {
        &self.layout
    }
}

#[cfg(test)]
#[path = "tests/shared_shader_tests.rs"]
mod tests;
