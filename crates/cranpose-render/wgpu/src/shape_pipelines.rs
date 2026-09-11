use std::{collections::HashMap, sync::Arc};

use crate::{
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
    pub(crate) fn new(factory: ShapePipelineFactory, _backend: wgpu::Backend) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let compiler = {
            static ASYNC_SHAPE_PIPELINES: crate::debug_toggles::DebugToggle =
                crate::debug_toggles::DebugToggle::new("CRANPOSE_ASYNC_SHAPE_PIPELINES");
            (_backend == wgpu::Backend::Vulkan && !ASYNC_SHAPE_PIPELINES.equals("0"))
                .then(|| {
                    let factory = factory.clone();
                    background::Compiler::new(move |key| factory.create(key))
                        .map_err(|error| {
                            log::error!("could not start the shape pipeline compiler: {error}");
                        })
                        .ok()
                })
                .flatten()
        };
        let mut pipelines = Self {
            factory,
            ready: HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            compiler,
        };
        pipelines.prewarm_general();
        pipelines
    }

    /// Builds every general pipeline before the first frame.
    ///
    /// A pipeline built on demand is built inside the frame that first needs
    /// it, and the driver's compile lands as a stall the user sees while
    /// scrolling. The general space is small and fully known --
    /// `SUPPORTED_BLEND_MODES` over both run tiers, six in all -- so there is
    /// nothing to gain by waiting: a scene that never draws `DstOut` pays for
    /// two pipelines it does not use, and one that does pays for them at
    /// startup instead of mid-scroll.
    ///
    /// Only the general variants. Specialized ones are unbounded in principle
    /// and the background compiler already keeps them off the frame where it
    /// runs; where it does not, `get` falls back to the general pipeline,
    /// which is why every one of these has to exist before any draw.
    fn prewarm_general(&mut self) {
        let started = web_time::Instant::now();
        for blend_mode in crate::render::SUPPORTED_BLEND_MODES {
            for tier in [crate::render::RunTier::Store, crate::render::RunTier::Arena] {
                self.ensure_general(ShapePipelineKey::general_for(blend_mode, tier));
            }
        }
        log::info!(
            "[gpu-init] {} general shape pipelines ready in {:.1} ms",
            self.ready.len(),
            crate::render::instant_ms(started, web_time::Instant::now()),
        );
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
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    };

    use smallvec::SmallVec;

    use crate::render::ShapePipelineKey;

    pub(super) struct Compiler<T> {
        requests: SyncSender<ShapePipelineKey>,
        completed: Receiver<(ShapePipelineKey, T)>,
        pending: SmallVec<[ShapePipelineKey; 2]>,
        stopped: Arc<AtomicBool>,
    }

    impl<T: Send + 'static> Compiler<T> {
        pub(super) fn new(
            mut create: impl FnMut(ShapePipelineKey) -> T + Send + 'static,
        ) -> Result<Self, std::io::Error> {
            let (requests, requested) = mpsc::sync_channel(1);
            let (finished, completed) = mpsc::sync_channel(1);
            let stopped = Arc::new(AtomicBool::new(false));
            let worker_stopped = Arc::clone(&stopped);
            std::thread::Builder::new()
                .name("cranpose-shape-compiler".into())
                .spawn(move || {
                    crate::render::mark_thread_off_frame();
                    while let Ok(key) = requested.recv() {
                        if worker_stopped.load(Ordering::Acquire) {
                            break;
                        }
                        let pipeline = create(key);
                        if finished.send((key, pipeline)).is_err() {
                            break;
                        }
                    }
                })?;
            Ok(Self {
                requests,
                completed,
                pending: SmallVec::new(),
                stopped,
            })
        }

        pub(super) fn request(&mut self, key: ShapePipelineKey) {
            if self.pending.len() == self.pending.inline_size() || self.pending.contains(&key) {
                return;
            }
            match self.requests.try_send(key) {
                Ok(()) => self.pending.push(key),
                Err(TrySendError::Full(_)) => {}
                Err(TrySendError::Disconnected(_)) => {
                    panic!("shape pipeline compiler stopped unexpectedly")
                }
            }
        }

        pub(super) fn collect(&mut self, mut publish: impl FnMut(ShapePipelineKey, T)) {
            loop {
                match self.completed.try_recv() {
                    Ok((key, pipeline)) => {
                        self.pending.retain(|pending| *pending != key);
                        publish(key, pipeline);
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        panic!("shape pipeline compiler stopped unexpectedly")
                    }
                }
            }
        }
    }

    impl<T> Drop for Compiler<T> {
        fn drop(&mut self) {
            self.stopped.store(true, Ordering::Release);
        }
    }

    #[cfg(test)]
    mod tests {
        use cranpose_ui_graphics::BlendMode;
        use web_time::{Duration, Instant};

        use super::*;
        use crate::render::RunTier;

        fn key(blend: BlendMode) -> ShapePipelineKey {
            ShapePipelineKey::general_for(blend, RunTier::Store)
        }

        #[test]
        fn duplicate_and_excess_work_is_bounded_and_drop_cancels_queued_work() {
            let (started, observed) = mpsc::channel();
            let (release, blocked) = mpsc::channel();
            let mut compiler = Compiler::new(move |key| {
                started.send(key).unwrap();
                blocked.recv().unwrap();
                key
            })
            .expect("test compiler");
            let first = key(BlendMode::SrcOver);
            let second = key(BlendMode::DstOut);
            compiler.request(first);
            assert_eq!(
                observed.recv_timeout(Duration::from_secs(2)).unwrap(),
                first
            );
            compiler.request(first);
            compiler.request(second);
            compiler.request(key(BlendMode::Plus));
            assert_eq!(compiler.pending.as_slice(), &[first, second]);
            drop(compiler);
            release.send(()).unwrap();
            assert_eq!(
                observed.recv_timeout(Duration::from_secs(2)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            );
        }

        #[test]
        fn results_keep_their_keys_and_publish_only_when_collected() {
            let mut compiler = Compiler::new(|key| key.blend_mode).expect("test compiler");
            let expected = key(BlendMode::DstOut);
            compiler.request(expected);
            assert_eq!(compiler.pending.as_slice(), &[expected]);
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut published = Vec::new();
            while published.is_empty() {
                compiler.collect(|key, value| published.push((key, value)));
                assert!(Instant::now() < deadline, "compiler did not finish");
                std::thread::yield_now();
            }
            assert_eq!(published, [(expected, BlendMode::DstOut)]);
            assert!(compiler.pending.is_empty());
        }
    }
}

#[cfg(test)]
mod prewarm_tests {
    use cranpose_ui_graphics::BlendMode;

    use crate::render::{RunTier, SUPPORTED_BLEND_MODES, ShapePipelineKey, supported_blend_mode};

    fn prewarmed() -> Vec<ShapePipelineKey> {
        let mut keys = Vec::new();
        for blend_mode in SUPPORTED_BLEND_MODES {
            for tier in [RunTier::Store, RunTier::Arena] {
                keys.push(ShapePipelineKey::general_for(blend_mode, tier));
            }
        }
        keys
    }

    #[test]
    fn every_blend_mode_a_scene_can_ask_for_is_prewarmed() {
        let ready = prewarmed();
        for mode in BlendMode::ALL {
            for tier in [RunTier::Store, RunTier::Arena] {
                let key = ShapePipelineKey::general_for(supported_blend_mode(mode), tier);
                assert!(
                    ready.contains(&key),
                    "{mode:?} resolves to a general pipeline no one built at startup, so the \
                     first draw using it compiles inside its own frame"
                );
            }
        }
    }

    #[test]
    fn the_prewarmed_set_is_the_whole_general_space() {
        let ready = prewarmed();
        assert_eq!(ready.len(), SUPPORTED_BLEND_MODES.len() * 2);
        assert!(ready.iter().all(|key| key.is_general()));
        let mut unique = ready.clone();
        unique.dedup();
        assert_eq!(unique.len(), ready.len(), "a pipeline built twice");
    }

    #[test]
    fn an_unsupported_mode_folds_onto_one_that_is_built() {
        assert_eq!(
            supported_blend_mode(BlendMode::Multiply),
            BlendMode::SrcOver
        );
        assert_eq!(supported_blend_mode(BlendMode::Clear), BlendMode::SrcOver);
        assert_eq!(supported_blend_mode(BlendMode::Src), BlendMode::Src);
        assert_eq!(supported_blend_mode(BlendMode::DstOut), BlendMode::DstOut);
    }
}
