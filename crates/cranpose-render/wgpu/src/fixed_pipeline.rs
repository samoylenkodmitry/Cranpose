//! Render pipelines the renderer builds from its own fixed sources: blurs,
//! blits, images and glyphs. A launch records, by label, the ones it draws
//! with, and later launches queue those on the warm-up lane before their
//! frames ask for them.

#[cfg(not(target_arch = "wasm32"))]
use std::cell::Cell;

use crate::{
    lazy_resource::LazyGpuResource,
    pipeline_compiler::{CompileLane, CompilerSend, PipelineCompiler},
    pipeline_recorder::PipelineRecorder,
};

/// One fixed render pipeline, named by a label no other fixed pipeline
/// shares.
pub(crate) struct FixedPipeline {
    resource: LazyGpuResource<wgpu::RenderPipeline>,
    #[cfg(not(target_arch = "wasm32"))]
    drawn: Cell<bool>,
}

impl FixedPipeline {
    pub(crate) fn new(label: &'static str) -> Self {
        Self {
            resource: LazyGpuResource::new(label),
            #[cfg(not(target_arch = "wasm32"))]
            drawn: Cell::new(false),
        }
    }

    /// The pipeline for a draw, built here if nothing has built it yet. The
    /// first draw is noted with `recorder`.
    #[cfg_attr(target_arch = "wasm32", expect(unused_variables))]
    pub(crate) fn for_draw(
        &self,
        recorder: &PipelineRecorder,
        backend: wgpu::Backend,
        create: impl FnOnce() -> wgpu::RenderPipeline,
    ) -> &wgpu::RenderPipeline {
        #[cfg(not(target_arch = "wasm32"))]
        if !self.drawn.replace(true) {
            recorder.note_fixed(self.resource.label(), recorder.in_first_screen());
        }
        self.resource.get_or_init(backend, create)
    }

    pub(crate) fn get(&self) -> Option<&wgpu::RenderPipeline> {
        self.resource.get()
    }

    /// Queues the build on `lane` unless it is already built.
    pub(crate) fn queue(
        &self,
        compiler: &PipelineCompiler,
        lane: CompileLane,
        backend: wgpu::Backend,
        create: impl FnOnce() -> wgpu::RenderPipeline + CompilerSend + 'static,
    ) {
        if self.resource.get().is_none() {
            self.resource.queue(compiler, lane, backend, create);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn label(&self) -> &'static str {
        self.resource.label()
    }
}
