// The pipelines of the app shaders a scene draws with: volumes' and shaded
// surfaces'.

use std::{
    cell::Cell,
    sync::{Arc, Weak},
};

use cranpose_ui_graphics::space::Multisample;

use super::sample_slot;
use crate::{
    lazy_resource::LazyGpuResource,
    pipeline_compiler::{CompileLane, CompilerSend, PipelineCompiler},
    pipeline_recorder::PipelineRecorder,
};

// The pipelines of one app shader, kept while anyone holds the shader, one
// for each of its `N` variants.
pub(crate) struct AppShader<const N: usize> {
    pub(super) source: Weak<str>,
    pub(super) module: LazyGpuResource<Option<wgpu::ShaderModule>>,
    // The variants' pipelines for each sample count, in the order of
    // `super::SAMPLES`.
    pipelines: [[ShaderPipeline; N]; 2],
}

// A volume shader's pipelines, one for each [`super::VolumeVariant`].
pub(crate) type VolumeShader = AppShader<4>;
// A shaded surface's shader's pipelines, one for each [`super::ShadedVariant`].
pub(crate) type ShadedShader = AppShader<3>;

impl<const N: usize> AppShader<N> {
    pub(super) fn new(
        shader: &Arc<str>,
        module: &'static str,
        labels: [[&'static str; N]; 2],
    ) -> Self {
        Self {
            source: Arc::downgrade(shader),
            module: LazyGpuResource::new(module),
            pipelines: labels.map(|labels| labels.map(ShaderPipeline::new)),
        }
    }

    // The pipeline of the variant numbered `variant`.
    pub(crate) fn pipeline(&self, variant: usize, samples: Multisample) -> &ShaderPipeline {
        &self.pipelines[sample_slot(samples)][variant]
    }
}

// The kept pipelines of `shader` among `kept`.
pub(super) fn kept_shader<'k, const N: usize>(
    kept: &'k [AppShader<N>],
    shader: &Arc<str>,
) -> Option<&'k AppShader<N>> {
    // A weak handle keeps the shader's memory, so no other shader takes its
    // address while the pipelines are kept.
    kept.iter()
        .find(|kept| std::ptr::addr_eq(kept.source.as_ptr(), Arc::as_ptr(shader)))
}

// One of an app shader's pipelines; `None` once built when the shader
// does not build.
pub(crate) struct ShaderPipeline {
    resource: LazyGpuResource<Option<wgpu::RenderPipeline>>,
    // Whether its build is queued, so it is queued once.
    queued: Cell<bool>,
}

impl ShaderPipeline {
    fn new(label: &'static str) -> Self {
        Self {
            resource: LazyGpuResource::new(label),
            queued: Cell::new(false),
        }
    }

    // The pipeline once built, or `None` while it compiles: a missing one
    // is queued on the demand lane, and built here when nothing else can
    // build it.
    pub(crate) fn when_ready<J>(
        &self,
        (recorder, compiler, backend): (&PipelineRecorder, &PipelineCompiler, wgpu::Backend),
        job: impl FnOnce() -> J,
    ) -> Option<&Option<wgpu::RenderPipeline>>
    where
        J: FnOnce() -> Option<wgpu::RenderPipeline> + CompilerSend + 'static,
    {
        if let Some(built) = self.resource.get() {
            return Some(built);
        }
        if !compiler.is_active() {
            return Some(self.resource.for_draw(recorder, backend, job()));
        }
        if !self.queued.replace(true) {
            self.resource
                .queue(compiler, CompileLane::Demanded, backend, job());
        }
        None
    }
}
