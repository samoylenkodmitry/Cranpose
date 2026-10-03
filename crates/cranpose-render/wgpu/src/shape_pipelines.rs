use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use cranpose_core::collections::map::HashMap;
use smallvec::SmallVec;

use crate::{
    pipeline_compiler::{CompileLane, CompilerSend, CompilerSync, PipelineCompiler},
    render::{ShapePipelineKey, create_shape_pipeline},
    run_store::RunBufferMode,
    shared_shader::SharedShader,
};

#[derive(Clone)]
pub(crate) struct ShapePipelineFactory {
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) cache: Option<wgpu::PipelineCache>,
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) shader: SharedShader,
    pub(crate) mode: RunBufferMode,
}

/// Builds the value a key names, on whichever thread asks.
pub(crate) trait KeyedBuild: CompilerSend + CompilerSync + 'static {
    type Output: CompilerSend + CompilerSync + 'static;

    fn build(&self, key: ShapePipelineKey) -> Self::Output;
}

impl KeyedBuild for ShapePipelineFactory {
    type Output = wgpu::RenderPipeline;

    fn build(&self, key: ShapePipelineKey) -> wgpu::RenderPipeline {
        create_shape_pipeline(
            &self.device,
            self.cache.as_ref(),
            self.format,
            &self.shader,
            key,
            self.mode,
        )
    }
}

pub(crate) struct ShapePipelines {
    slots: Slots<ShapePipelineFactory>,
    /// Whether a draw takes the general pipeline while its specialized one
    /// builds on the background compiler.
    asynchronous: bool,
    #[cfg(not(target_arch = "wasm32"))]
    first_use: Option<web_time::Instant>,
}

impl ShapePipelines {
    /// Pipelines from `factory`. Those `first_screen` names, the ones the
    /// last launch drew its first screen with, start building on the
    /// compiler's warm-up lane at once, so the first frame finds them ready.
    pub(crate) fn new(
        factory: ShapePipelineFactory,
        backend: wgpu::Backend,
        compiler: &PipelineCompiler,
        first_screen: impl IntoIterator<Item = ShapePipelineKey>,
    ) -> Self {
        static ASYNC_SHAPE_PIPELINES: crate::debug_toggles::DebugToggle =
            crate::debug_toggles::DebugToggle::new("CRANPOSE_ASYNC_SHAPE_PIPELINES");
        let asynchronous = compiler.is_active()
            && backend == wgpu::Backend::Vulkan
            && !ASYNC_SHAPE_PIPELINES.equals("0");
        let mut slots = Slots::new(compiler, factory);
        for key in first_screen {
            slots.warm(key);
        }
        Self {
            slots,
            asynchronous,
            #[cfg(not(target_arch = "wasm32"))]
            first_use: None,
        }
    }

    pub(crate) fn begin_frame(&mut self) {
        self.slots.settle_demanded();
    }

    pub(crate) fn request_wanted(&mut self) {
        self.slots.request_wanted();
    }

    pub(crate) fn ensure(&mut self, key: ShapePipelineKey, vertices: u64) {
        let need = self.slots.need(key);
        #[cfg(not(target_arch = "wasm32"))]
        if need.first
            && self
                .first_use
                .get_or_insert_with(web_time::Instant::now)
                .elapsed()
                <= crate::pipeline_disk_cache::FIRST_SCREEN_SPAN
        {
            crate::pipeline_disk_cache::note_first_screen_pipeline(key.to_bits());
        }
        if need.ready {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let compile_started = web_time::Instant::now();
        let general = key.general();
        if self.asynchronous
            && key != general
            && (!need.queued || self.slots.get(general).is_some())
        {
            self.slots.build(general);
            self.slots.want(key, vertices);
        } else {
            self.slots.build(key);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(first_use) = &mut self.first_use {
            *first_use += compile_started.elapsed();
        }
    }

    pub(crate) fn get(&self, key: ShapePipelineKey) -> Option<(&wgpu::RenderPipeline, bool)> {
        self.slots
            .get(key)
            .map(|pipeline| (pipeline, false))
            .or_else(|| {
                self.slots
                    .get(key.general())
                    .map(|pipeline| (pipeline, true))
            })
    }
}

/// What a draw's first look at a key found.
pub(crate) struct Need {
    /// Its value is built.
    pub(crate) ready: bool,
    /// No draw needed the key before.
    #[cfg_attr(target_arch = "wasm32", expect(dead_code))]
    pub(crate) first: bool,
    pub(crate) queued: bool,
}

struct Entry<T> {
    value: Arc<OnceLock<T>>,
    needed: bool,
    queued: Option<CompileLane>,
}

/// Values by key, each built once by whichever thread asks first: a job on
/// the background compiler, or the frame that needs it. A frame asking for
/// a value a job is building waits for that build rather than starting a
/// second.
pub(crate) struct Slots<B: KeyedBuild> {
    entries: HashMap<ShapePipelineKey, Entry<B::Output>>,
    builder: Arc<B>,
    compiler: PipelineCompiler,
    /// Keys a frame queued, not yet built: at most two at a time, so a burst
    /// of new keys queues behind nothing a frame waits for.
    demanded: SmallVec<[ShapePipelineKey; 2]>,
    wanted: SmallVec<[(ShapePipelineKey, u64); 4]>,
    stopped: Arc<AtomicBool>,
}

impl<B: KeyedBuild> Slots<B> {
    pub(crate) fn new(compiler: &PipelineCompiler, builder: B) -> Self {
        Self {
            entries: HashMap::default(),
            builder: Arc::new(builder),
            compiler: compiler.clone(),
            demanded: SmallVec::new(),
            wanted: SmallVec::new(),
            stopped: Arc::new(AtomicBool::new(false)),
        }
    }

    fn entry(
        entries: &mut HashMap<ShapePipelineKey, Entry<B::Output>>,
        key: ShapePipelineKey,
    ) -> &mut Entry<B::Output> {
        entries.entry(key).or_insert_with(|| Entry {
            value: Arc::new(OnceLock::new()),
            needed: false,
            queued: None,
        })
    }

    /// Records that a draw needs `key`.
    pub(crate) fn need(&mut self, key: ShapePipelineKey) -> Need {
        let entry = Self::entry(&mut self.entries, key);
        let first = !entry.needed;
        entry.needed = true;
        Need {
            ready: entry.value.get().is_some(),
            first,
            queued: entry.queued.is_some(),
        }
    }

    /// Queues `key`'s build on the warm-up lane, ahead of any draw.
    pub(crate) fn warm(&mut self, key: ShapePipelineKey) {
        if self.compiler.is_active() {
            self.queue(key, CompileLane::WarmUp);
        }
    }

    /// Queues `key`'s build for a draw standing in with another value until
    /// it is ready.
    pub(crate) fn request(&mut self, key: ShapePipelineKey) {
        if self.demanded.len() == self.demanded.inline_size()
            || self.entries.get(&key).is_some_and(|entry| {
                entry.queued == Some(CompileLane::Demanded) || entry.value.get().is_some()
            })
        {
            return;
        }
        self.demanded.push(key);
        self.queue(key, CompileLane::Demanded);
    }

    fn queue(&mut self, key: ShapePipelineKey, lane: CompileLane) {
        let builder = Arc::clone(&self.builder);
        let stopped = Arc::clone(&self.stopped);
        let entry = Self::entry(&mut self.entries, key);
        entry.queued = Some(lane);
        let value = Arc::clone(&entry.value);
        self.compiler.enqueue(lane, move || {
            if !stopped.load(Ordering::Acquire) {
                value.get_or_init(|| builder.build(key));
            }
        });
    }

    /// Builds `key`'s value here unless a job already built it, or waits
    /// for the job building it.
    pub(crate) fn build(&mut self, key: ShapePipelineKey) {
        Self::entry(&mut self.entries, key)
            .value
            .get_or_init(|| self.builder.build(key));
    }

    pub(crate) fn get(&self, key: ShapePipelineKey) -> Option<&B::Output> {
        self.entries.get(&key)?.value.get()
    }

    pub(crate) fn want(&mut self, key: ShapePipelineKey, vertices: u64) {
        match self.wanted.iter_mut().find(|(wanted, _)| *wanted == key) {
            Some((_, total)) => *total += vertices,
            None => self.wanted.push((key, vertices)),
        }
    }

    /// Forgets the demanded keys whose values are built.
    pub(crate) fn settle_demanded(&mut self) {
        let entries = &self.entries;
        self.demanded.retain(|key| {
            entries
                .get(key)
                .is_none_or(|entry| entry.value.get().is_none())
        });
    }

    pub(crate) fn request_wanted(&mut self) {
        if self.wanted.is_empty() {
            return;
        }
        let mut wanted = std::mem::take(&mut self.wanted);
        wanted.sort_unstable_by_key(|&(_, vertices)| std::cmp::Reverse(vertices));
        for (key, _) in wanted.drain(..) {
            self.request(key);
        }
        self.wanted = wanted;
    }

    #[cfg(test)]
    fn demanded(&self) -> &[ShapePipelineKey] {
        &self.demanded
    }
}

impl<B: KeyedBuild> Drop for Slots<B> {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "tests/shape_pipelines_slots_tests.rs"]
mod slots_tests;

#[cfg(test)]
#[path = "tests/shape_pipelines_tests.rs"]
mod tests;
