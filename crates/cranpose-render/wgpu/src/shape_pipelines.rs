use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use cranpose_core::collections::map::HashMap;
use smallvec::SmallVec;

use crate::{
    lazy_resource::Pending,
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

    /// Whether `output` draws: a pipeline the browser still compiles in the
    /// background does not yet.
    fn landed(_output: &Self::Output) -> bool {
        true
    }
}

impl KeyedBuild for ShapePipelineFactory {
    type Output = wgpu::RenderPipeline;

    fn landed(pipeline: &wgpu::RenderPipeline) -> bool {
        pipeline.is_ready()
    }

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

pub(crate) struct ShapePipelines<B: KeyedBuild = ShapePipelineFactory> {
    slots: Slots<B>,
    /// Whether draws take the universal pipeline until their specialized one
    /// compiled in the background, which they build only once
    /// [`Self::build_in_background`] says the browser compiles there: see
    /// [`Self::ensure`].
    stands_in: bool,
    /// Whether pipelines built now compile in the browser's background.
    background_builds: bool,
    /// Whether the compiler's threads build every missing pipeline while
    /// its draws take fallback triangles, instead of the frame building it.
    draws_when_ready: bool,
    recorder: crate::pipeline_recorder::PipelineRecorder,
    // The specialized pipelines built to compile in the background that had
    // not landed at the last look.
    compiling: SmallVec<[ShapePipelineKey; 8]>,
    // How many looks found such pipelines landed.
    landings: u64,
}

impl<B: KeyedBuild> ShapePipelines<B> {
    /// Pipelines from `factory`. Those `first_screen` names, the ones the
    /// last launch drew its first screen with, start building on the
    /// compiler's warm-up lane at once, so the first frame finds them ready.
    pub(crate) fn new(
        factory: B,
        backend: wgpu::Backend,
        compiler: &PipelineCompiler,
        first_screen: impl IntoIterator<Item = ShapePipelineKey>,
        recorder: crate::pipeline_recorder::PipelineRecorder,
    ) -> Self {
        let mut slots = Slots::new(compiler, factory);
        for key in first_screen {
            slots.warm(key);
        }
        Self {
            slots,
            stands_in: backend == wgpu::Backend::BrowserWebGpu,
            background_builds: false,
            draws_when_ready: compiler.is_active(),
            recorder,
            compiling: SmallVec::new(),
            landings: 0,
        }
    }

    /// Builds the specialized pipelines draws want from now on: the browser
    /// compiles what the device creates in the background
    /// (`wgpu::Device::compile_pipelines_in_background`).
    pub(crate) fn build_in_background(&mut self) {
        if !self.background_builds {
            self.background_builds = true;
            // The frames before stood in for these. A frame that keeps their
            // stand-ins draws none of them again to want them.
            self.build_wanted();
        }
    }

    // Counts a landing when a specialized pipeline built to compile in the
    // background landed since the last look. No draw waits for one: the
    // universal pipeline stands in, so no pass leaves a draw out for the
    // browser to report its landing.
    pub(crate) fn count_landings(&mut self) {
        let compiling = self.compiling.len();
        let slots = &self.slots;
        self.compiling
            .retain(|key| !slots.get(*key).is_some_and(B::landed));
        if self.compiling.len() != compiling {
            self.landings += 1;
        }
    }

    pub(crate) fn landings(&self) -> u64 {
        self.landings
    }

    pub(crate) fn begin_frame(&mut self) {
        // Until the browser compiles in the background, the wants wait for
        // it with the stand-ins drawn on screen.
        if self.background_builds {
            self.slots.begin_frame();
        }
    }

    /// Queues `key`'s build on the warm-up lane unless it is already queued
    /// or built.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn warm(&mut self, key: ShapePipelineKey) {
        self.slots.warm(key);
    }

    /// Queues the build of `key` and of the interior it lays down on the
    /// demanded lane, handing `awaited` each still to land.
    pub(crate) fn demand_with_interior(
        &mut self,
        key: ShapePipelineKey,
        awaited: &mut impl FnMut(Pending<B::Output>),
    ) {
        for key in std::iter::once(key).chain(key.interior()) {
            if let Some(pending) = self.slots.warm_on(key, CompileLane::Demanded) {
                awaited(pending);
            }
        }
    }

    /// Builds the specialized pipelines the frame's draws stood in for, once
    /// the browser compiles them in the background.
    pub(crate) fn build_wanted(&mut self) {
        if self.background_builds {
            let compiling = &mut self.compiling;
            self.slots.build_wanted(|key| compiling.push(key));
        }
    }

    /// Builds `key`'s pipeline here unless it is built, ahead of any draw;
    /// where draws stand in, the general pipeline that stands in for it.
    pub(crate) fn build_ahead(&mut self, key: ShapePipelineKey) {
        let key = if self.stands_in { key.universal() } else { key };
        self.slots.build(key);
    }

    /// Readies `key`'s pipeline for a draw. Where the compiler's threads
    /// run, a missing one is queued and its draws take fallback triangles
    /// until it lands; elsewhere the frame builds it here.
    pub(crate) fn ensure(&mut self, key: ShapePipelineKey) {
        let need = self.slots.need(key);
        if need.first {
            self.recorder
                .note_shape(key.to_bits(), self.recorder.in_first_screen());
        }
        if need.ready && (!self.stands_in || self.slots.get(key).is_some_and(B::landed)) {
            return;
        }
        if self.draws_when_ready {
            // Behind the frame's image, glyph, shadow and composite
            // pipelines, which have no fallback.
            self.slots.warm(key);
            return;
        }
        self.recorder.during_demand(|| {
            // In the browser every specialized pipeline the first screen
            // draws with compiles beside the others before it shows: on an
            // Apple M5 with no Metal shader cached, the gauntlet's five
            // shape pipelines and seven others ended their compiles 332 ms
            // after they began; one universal pipeline in place of the five
            // ended them after 227. Draws take it until their own compiled
            // in the background.
            if self.stands_in && key != key.universal() {
                self.slots.build(key.universal());
                self.slots.want(key);
                return;
            }
            self.slots.build(key);
        });
    }

    pub(crate) fn get(&self, key: ShapePipelineKey) -> Option<(&B::Output, bool)> {
        let stand_in = if self.stands_in {
            key.universal()
        } else {
            key.general()
        };
        let general = || self.slots.get(stand_in).map(|pipeline| (pipeline, true));
        match self.slots.get(key) {
            Some(pipeline) if !self.stands_in || B::landed(pipeline) => Some((pipeline, false)),
            Some(pipeline) => general().or(Some((pipeline, false))),
            None => general(),
        }
    }
}

/// The shape pipelines a stretch of draws needs: a pass gathers them as it
/// batches its runs and prepares each once before it records its draws,
/// instead of looking every run's keys up as it goes.
#[derive(Default)]
pub(crate) struct PipelineDemand(SmallVec<[ShapePipelineKey; 8]>);

impl PipelineDemand {
    /// Adds `key` unless it is already there.
    pub(crate) fn add(&mut self, key: ShapePipelineKey) {
        if !self.0.contains(&key) {
            self.0.push(key);
        }
    }

    /// Each key, in the order it was added.
    pub(crate) fn keys(&self) -> impl Iterator<Item = ShapePipelineKey> + '_ {
        self.0.iter().copied()
    }

    /// Each key, leaving the demand empty.
    pub(crate) fn drain(&mut self) -> impl Iterator<Item = ShapePipelineKey> + '_ {
        self.0.drain(..)
    }
}

/// What a draw's first look at a key found.
pub(crate) struct Need {
    /// Its value is built.
    pub(crate) ready: bool,
    /// No draw needed the key before.
    pub(crate) first: bool,
}

struct Entry<T> {
    value: Arc<OnceLock<T>>,
    needed: bool,
    queued: bool,
}

/// Values by key, each built once by whichever thread asks first: a job on
/// the background compiler, or the frame that needs it. A frame asking for
/// a value a job is building waits for that build rather than starting a
/// second.
pub(crate) struct Slots<B: KeyedBuild> {
    entries: HashMap<ShapePipelineKey, Entry<B::Output>>,
    builder: Arc<B>,
    compiler: PipelineCompiler,
    wanted: PipelineDemand,
    stopped: Arc<AtomicBool>,
}

impl<B: KeyedBuild> Slots<B> {
    pub(crate) fn new(compiler: &PipelineCompiler, builder: B) -> Self {
        Self {
            entries: HashMap::default(),
            builder: Arc::new(builder),
            compiler: compiler.clone(),
            wanted: PipelineDemand::default(),
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
            queued: false,
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
        }
    }

    /// Queues `key`'s build on the warm-up lane, ahead of any draw, unless it
    /// is already queued or built.
    pub(crate) fn warm(&mut self, key: ShapePipelineKey) {
        self.warm_on(key, CompileLane::WarmUp);
    }

    /// Queues `key`'s build on `lane` unless it is already queued or built,
    /// or no compiler runs; the value while it is still to land.
    pub(crate) fn warm_on(
        &mut self,
        key: ShapePipelineKey,
        lane: CompileLane,
    ) -> Option<Pending<B::Output>> {
        if !self.compiler.is_active() {
            return None;
        }
        let builder = Arc::clone(&self.builder);
        let stopped = Arc::clone(&self.stopped);
        let entry = Self::entry(&mut self.entries, key);
        if entry.value.get().is_some() {
            return None;
        }
        let value = Arc::clone(&entry.value);
        if !std::mem::replace(&mut entry.queued, true) {
            let building = Arc::clone(&value);
            self.compiler.enqueue(lane, move || {
                if !stopped.load(Ordering::Acquire) {
                    building.get_or_init(|| builder.build(key));
                }
            });
        }
        Some(Pending(value))
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

    pub(crate) fn want(&mut self, key: ShapePipelineKey) {
        self.wanted.add(key);
    }

    // Builds here every pipeline draws wanted, handing `built` each key it
    // built now.
    pub(crate) fn build_wanted(&mut self, mut built: impl FnMut(ShapePipelineKey)) {
        for key in self.wanted.drain() {
            let value = &Self::entry(&mut self.entries, key).value;
            if value.get().is_none() {
                value.get_or_init(|| self.builder.build(key));
                built(key);
            }
        }
    }

    /// Forgets the wants of a frame that ended before queuing them.
    pub(crate) fn begin_frame(&mut self) {
        self.wanted.0.clear();
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

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "tests/shape_pipelines_stand_in_tests.rs"]
mod stand_in_tests;
