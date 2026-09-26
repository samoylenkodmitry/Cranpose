use std::sync::{Arc, OnceLock};

use web_time::Instant;

use crate::pipeline_compiler::{CompileLane, CompilerSend, CompilerSync, PipelineCompiler};

/// A GPU resource created once, by whichever thread asks first: a job
/// queued on the background compiler, or the frame that needs it. A frame
/// arriving while that job is under way waits for that one creation rather
/// than starting a second.
pub(crate) struct LazyGpuResource<T> {
    label: &'static str,
    value: Arc<OnceLock<T>>,
}

impl<T> Clone for LazyGpuResource<T> {
    fn clone(&self) -> Self {
        Self {
            label: self.label,
            value: Arc::clone(&self.value),
        }
    }
}

impl<T> LazyGpuResource<T> {
    pub(crate) fn new(label: &'static str) -> Self {
        Self {
            label,
            value: Arc::new(OnceLock::new()),
        }
    }

    pub(crate) fn get_or_init(&self, backend: wgpu::Backend, create: impl FnOnce() -> T) -> &T {
        self.value.get_or_init(|| {
            let started = Instant::now();
            let value = create();
            log::info!(
                "[gpu-pipeline] {:?} {} ready in {:.1} ms",
                backend,
                self.label,
                crate::render::instant_ms(started, Instant::now()),
            );
            value
        })
    }

    pub(crate) fn get(&self) -> Option<&T> {
        self.value.get()
    }
}

impl<T: CompilerSend + CompilerSync + 'static> LazyGpuResource<T> {
    /// Queues the creation on `lane` of the background compiler.
    pub(crate) fn queue(
        &self,
        compiler: &PipelineCompiler,
        lane: CompileLane,
        backend: wgpu::Backend,
        create: impl FnOnce() -> T + CompilerSend + 'static,
    ) {
        let resource = self.clone();
        compiler.enqueue(lane, move || {
            resource.get_or_init(backend, create);
        });
    }
}

#[cfg(test)]
#[path = "tests/lazy_resource_tests.rs"]
mod tests;
