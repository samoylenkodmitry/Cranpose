use std::{collections::HashMap, sync::Arc};

use crate::{
    pipeline_compiler::PipelineCompiler,
    render::{ShapePipelineKey, create_shape_pipeline},
    run_store::RunBufferMode,
};

#[derive(Clone)]
pub(crate) struct ShapePipelineFactory {
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) cache: Option<wgpu::PipelineCache>,
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) uniform_layout: wgpu::BindGroupLayout,
    pub(crate) run_layout: wgpu::BindGroupLayout,
    pub(crate) mode: RunBufferMode,
}

impl ShapePipelineFactory {
    fn create(&self, key: ShapePipelineKey) -> wgpu::RenderPipeline {
        create_shape_pipeline(
            &self.device,
            self.cache.as_ref(),
            self.format,
            &self.uniform_layout,
            &self.run_layout,
            key,
            self.mode,
        )
    }
}

pub(crate) struct ShapePipelines {
    factory: ShapePipelineFactory,
    ready: HashMap<ShapePipelineKey, wgpu::RenderPipeline>,
    #[cfg(not(target_arch = "wasm32"))]
    compiler: Option<background::Compiler<wgpu::RenderPipeline>>,
}

impl ShapePipelines {
    pub(crate) fn new(
        factory: ShapePipelineFactory,
        _backend: wgpu::Backend,
        _compiler: &PipelineCompiler,
    ) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let compiler = {
            static ASYNC_SHAPE_PIPELINES: crate::debug_toggles::DebugToggle =
                crate::debug_toggles::DebugToggle::new("CRANPOSE_ASYNC_SHAPE_PIPELINES");
            (_backend == wgpu::Backend::Vulkan && !ASYNC_SHAPE_PIPELINES.equals("0"))
                .then(|| {
                    let factory = factory.clone();
                    background::Compiler::new(_compiler, move |key| factory.create(key))
                })
                .flatten()
        };
        Self {
            factory,
            ready: HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            compiler,
        }
    }

    fn asynchronous(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.compiler.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    fn ensure_general(&mut self, key: ShapePipelineKey) {
        self.ready
            .entry(key.general())
            .or_insert_with(|| self.factory.create(key.general()));
    }

    pub(crate) fn begin_frame(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(compiler) = self.compiler.as_mut() {
            compiler.collect(|key, pipeline| {
                self.ready.insert(key, pipeline);
            });
        }
    }

    pub(crate) fn ensure(&mut self, key: ShapePipelineKey) {
        if self.ready.contains_key(&key) {
            return;
        }
        if self.asynchronous() && !key.is_general() {
            self.ensure_general(key);
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(compiler) = self.compiler.as_mut() {
                compiler.request(key);
            }
        } else {
            self.ready.insert(key, self.factory.create(key));
        }
    }

    pub(crate) fn get(&self, key: ShapePipelineKey) -> Option<(&wgpu::RenderPipeline, bool)> {
        self.ready
            .get(&key)
            .map(|pipeline| (pipeline, false))
            .or_else(|| {
                self.ready
                    .get(&key.general())
                    .map(|pipeline| (pipeline, true))
            })
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod background {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    };

    use smallvec::SmallVec;

    use crate::{
        pipeline_compiler::{CompileLane, PipelineCompiler},
        render::ShapePipelineKey,
    };

    pub(super) struct Compiler<T> {
        compiler: PipelineCompiler,
        create: Arc<dyn Fn(ShapePipelineKey) -> T + Send + Sync>,
        finished: Sender<(ShapePipelineKey, T)>,
        completed: Receiver<(ShapePipelineKey, T)>,
        pending: SmallVec<[ShapePipelineKey; 2]>,
        stopped: Arc<AtomicBool>,
    }

    impl<T: Send + 'static> Compiler<T> {
        pub(super) fn new(
            compiler: &PipelineCompiler,
            create: impl Fn(ShapePipelineKey) -> T + Send + Sync + 'static,
        ) -> Option<Self> {
            compiler.is_active().then(|| {
                let (finished, completed) = mpsc::channel();
                Self {
                    compiler: compiler.clone(),
                    create: Arc::new(create),
                    finished,
                    completed,
                    pending: SmallVec::new(),
                    stopped: Arc::new(AtomicBool::new(false)),
                }
            })
        }

        pub(super) fn request(&mut self, key: ShapePipelineKey) {
            if self.pending.len() == self.pending.inline_size() || self.pending.contains(&key) {
                return;
            }
            self.pending.push(key);
            let create = Arc::clone(&self.create);
            let finished = self.finished.clone();
            let stopped = Arc::clone(&self.stopped);
            self.compiler.enqueue(CompileLane::Demanded, move || {
                if stopped.load(Ordering::Acquire) {
                    return;
                }
                finished.send((key, create(key))).ok();
            });
        }

        pub(super) fn collect(&mut self, mut publish: impl FnMut(ShapePipelineKey, T)) {
            while let Ok((key, pipeline)) = self.completed.try_recv() {
                self.pending.retain(|pending| *pending != key);
                publish(key, pipeline);
            }
        }
    }

    impl<T> Drop for Compiler<T> {
        fn drop(&mut self) {
            self.stopped.store(true, Ordering::Release);
        }
    }

    #[cfg(test)]
    #[path = "tests/shape_pipelines_background_tests.rs"]
    mod tests;
}

#[cfg(test)]
#[path = "tests/shape_pipelines_tests.rs"]
mod tests;
