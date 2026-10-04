//! Render pipelines the renderer builds from its own fixed sources: blurs,
//! blits, images and glyphs. A launch records, by label, the ones it draws
//! with, and later launches queue those on the warm-up lane before their
//! frames ask for them.

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
    /// Whether its build is queued, so it is queued once.
    queued: Cell<bool>,
    /// Whether the loaded driver cache holds it: a draw builds it in its
    /// frame as a cache hit.
    cached: Cell<bool>,
    #[cfg(not(target_arch = "wasm32"))]
    drawn: Cell<bool>,
}

impl FixedPipeline {
    pub(crate) fn new(label: &'static str) -> Self {
        Self {
            resource: LazyGpuResource::new(label),
            queued: Cell::new(false),
            cached: Cell::new(false),
            #[cfg(not(target_arch = "wasm32"))]
            drawn: Cell::new(false),
        }
    }

    /// The pipeline for a draw, built here if nothing has built it yet. The
    /// first draw is noted with `recorder`.
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
        self.resource.for_draw(recorder, backend, create)
    }

    pub(crate) fn get(&self) -> Option<&wgpu::RenderPipeline> {
        self.resource.get()
    }

    /// Queues the build on `lane` unless it is already built or queued.
    pub(crate) fn queue(
        &self,
        compiler: &PipelineCompiler,
        lane: CompileLane,
        backend: wgpu::Backend,
        create: impl FnOnce() -> wgpu::RenderPipeline + CompilerSend + 'static,
    ) {
        if self.resource.get().is_none() && !self.queued.replace(true) {
            self.resource.queue(compiler, lane, backend, create);
        }
    }

    /// Whether the pipeline is built; when it is not, its build, which
    /// `job` makes, is queued on the demand lane.
    pub(crate) fn ready_or_queue<J>(
        &self,
        compiler: &PipelineCompiler,
        backend: wgpu::Backend,
        job: impl FnOnce() -> J,
    ) -> bool
    where
        J: FnOnce() -> wgpu::RenderPipeline + CompilerSend + 'static,
    {
        if self.resource.get().is_some() || self.cached.get() {
            return true;
        }
        if compiler.is_active() {
            self.queue(compiler, CompileLane::Demanded, backend, job());
            return false;
        }
        true
    }

    /// Trusts the loaded driver cache with this pipeline.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn trust_cached(&self) {
        self.cached.set(true);
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn label(&self) -> &'static str {
        self.resource.label()
    }
}
