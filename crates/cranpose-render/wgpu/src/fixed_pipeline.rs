//! Render pipelines the renderer builds from its own fixed sources: blurs,
//! blits, images and glyphs. A launch notes, by label, the ones its first
//! screen draws with, and the next launch queues those on the warm-up lane
//! before its first frame asks for them.

#[cfg(not(target_arch = "wasm32"))]
use std::cell::Cell;

#[cfg(not(target_arch = "wasm32"))]
use web_time::Instant;

use crate::{
    lazy_resource::LazyGpuResource,
    pipeline_compiler::{CompileLane, CompilerSend, PipelineCompiler},
};

/// A renderer's first screen: its fixed-pipeline draws within
/// [`crate::pipeline_disk_cache::FIRST_SCREEN_SPAN`] of the first.
#[derive(Default)]
pub(crate) struct FirstScreenSpan {
    #[cfg(not(target_arch = "wasm32"))]
    started: Cell<Option<Instant>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl FirstScreenSpan {
    fn contains_now(&self) -> bool {
        let started = self.started.get().unwrap_or_else(|| {
            let now = Instant::now();
            self.started.set(Some(now));
            now
        });
        started.elapsed() <= crate::pipeline_disk_cache::FIRST_SCREEN_SPAN
    }
}

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
    /// first draw within `first_screen` is noted for the next launch.
    #[cfg_attr(target_arch = "wasm32", expect(unused_variables))]
    pub(crate) fn for_draw(
        &self,
        first_screen: &FirstScreenSpan,
        backend: wgpu::Backend,
        create: impl FnOnce() -> wgpu::RenderPipeline,
    ) -> &wgpu::RenderPipeline {
        #[cfg(not(target_arch = "wasm32"))]
        if !self.drawn.replace(true) && first_screen.contains_now() {
            crate::pipeline_disk_cache::note_first_screen_fixed(self.resource.label());
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

    /// Whether `recorded` names this pipeline.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn recorded_in(&self, recorded: &[String]) -> bool {
        recorded.iter().any(|label| label == self.resource.label())
    }
}
