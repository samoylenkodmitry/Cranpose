#[cfg(not(target_arch = "wasm32"))]
use std::sync::{
    Arc, Mutex, OnceLock, PoisonError,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{self, Receiver, Sender},
};

/// Tells a renderer that drew a placeholder when a pipeline lands, so it
/// draws again with the pipeline instead of the placeholder.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub(crate) struct Landing {
    queued: AtomicU64,
    built: AtomicU64,
    awaited: AtomicBool,
    landed: AtomicBool,
    wake: OnceLock<Box<dyn Fn() + Send + Sync>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Landing {
    /// The pipelines the background threads have built.
    pub(crate) fn built(&self) -> u64 {
        self.built.load(Ordering::Acquire)
    }

    /// Whether a queued pipeline has not been built yet.
    pub(crate) fn in_flight(&self) -> bool {
        self.queued.load(Ordering::Acquire) != self.built()
    }

    /// Waits for the next pipeline to land: a frame drew a placeholder.
    /// One built since `since`, a count of [`Self::built`] the frame read
    /// when it started, has landed already.
    pub(crate) fn await_since(&self, since: u64) {
        self.awaited.store(true, Ordering::Release);
        if self.built() != since && self.awaited.swap(false, Ordering::AcqRel) {
            self.landed.store(true, Ordering::Release);
        }
    }

    /// Whether a pipeline landed since a frame drew a placeholder; reading
    /// it clears it.
    pub(crate) fn take_landed(&self) -> bool {
        self.landed.swap(false, Ordering::AcqRel)
    }

    /// Calls `wake` when an awaited pipeline lands, from the thread that
    /// built it. Only the first call installs.
    pub(crate) fn wake_with(&self, wake: Box<dyn Fn() + Send + Sync>) {
        let _ = self.wake.set(wake);
    }

    /// Whether a pipeline landed since a frame drew a placeholder.
    pub(crate) fn landed(&self) -> bool {
        self.landed.load(Ordering::Acquire)
    }

    pub(crate) fn has_wake(&self) -> bool {
        self.wake.get().is_some()
    }

    fn after_build(&self) {
        self.built.fetch_add(1, Ordering::AcqRel);
        if self.awaited.swap(false, Ordering::AcqRel) {
            self.landed.store(true, Ordering::Release);
            if let Some(wake) = self.wake.get() {
                wake();
            }
        }
    }
}

/// How long the last handle waits for a lane to finish the pipeline it is
/// building. A process that exits while a thread is inside the driver's
/// pipeline compile can crash in the driver's own teardown (NVIDIA's does,
/// #859), so a dropped compiler lets that compile end; a driver slower than
/// this is left to it rather than holding the dropping thread.
#[cfg(not(target_arch = "wasm32"))]
const LANE_FINISH_WAIT: web_time::Duration = web_time::Duration::from_secs(5);

/// The warm-up lane's threads: a launch after an update compiles the last
/// launch's first screen there, dozens of pipelines of a few hundred
/// milliseconds each on a slow device's driver, and one thread left its
/// first frame waiting for them one at a time.
#[cfg(not(target_arch = "wasm32"))]
fn warm_up_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |cores| (cores.get() / 2).clamp(1, 3))
}

#[cfg(not(target_arch = "wasm32"))]
type Job = Box<dyn FnOnce() + Send + 'static>;

/// What a value crossing to the compiler threads must be: `Send` where
/// those threads exist, nothing on the web, where wgpu handles are single-threaded
/// and the compiler runs nothing.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) trait CompilerSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + ?Sized> CompilerSend for T {}
#[cfg(target_arch = "wasm32")]
pub(crate) trait CompilerSend {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> CompilerSend for T {}

/// What a value shared with the compiler threads must be: `Sync` where
/// those threads exist, nothing on the web.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) trait CompilerSync: Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Sync + ?Sized> CompilerSync for T {}
#[cfg(target_arch = "wasm32")]
pub(crate) trait CompilerSync {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> CompilerSync for T {}

/// Why a pipeline is queued, which decides the thread that builds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompileLane {
    /// A frame is drawing with a stand-in until this pipeline is ready.
    Demanded,
    /// No frame has asked for this pipeline yet; it is built ahead of its
    /// first use.
    WarmUp,
}

/// Runs pipeline creation on background threads, so a draw finds its
/// pipeline ready instead of paying the driver's compile inside the frame.
/// Each [`CompileLane`] has its own threads, one for the demanded lane and
/// a few for warm-ups, and starts its jobs in the order they were queued, so
/// a pipeline a frame is waiting for never queues behind warm-ups, which
/// take seconds each on a slow device's driver. The threads end with the
/// last handle, which skips the jobs they had not started and waits, up to
/// [`LANE_FINISH_WAIT`], for the ones they had.
#[derive(Clone, Default)]
pub(crate) struct PipelineCompiler {
    #[cfg(not(target_arch = "wasm32"))]
    workers: Option<Arc<Workers>>,
}

#[cfg(not(target_arch = "wasm32"))]
struct Workers {
    /// `None` once dropping has closed the lanes.
    demanded: Option<Sender<Job>>,
    warm_up: Option<Sender<Job>>,
    stopped: Arc<AtomicBool>,
    /// Each lane thread says here that it has ended. Behind a mutex only so
    /// the handle is `Sync`; the last handle reads it without locking.
    finished: Mutex<Receiver<()>>,
    threads: usize,
    landing: Arc<Landing>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Workers {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        self.demanded = None;
        self.warm_up = None;
        let finished = self
            .finished
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        let deadline = web_time::Instant::now() + LANE_FINISH_WAIT;
        for _ in 0..self.threads {
            let left = deadline.saturating_duration_since(web_time::Instant::now());
            if finished.recv_timeout(left).is_err() {
                log::warn!(
                    "[gpu-pipeline] a compile outlived its renderer by {LANE_FINISH_WAIT:?}"
                );
                break;
            }
        }
    }
}

/// A lane of `threads` threads taking its jobs in the order queued.
#[cfg(not(target_arch = "wasm32"))]
fn spawn_lane(
    name: &str,
    threads: usize,
    stopped: &Arc<AtomicBool>,
    finished: &Sender<()>,
    landing: &Arc<Landing>,
) -> std::io::Result<Sender<Job>> {
    let (jobs, queued) = mpsc::channel::<Job>();
    let queued = Arc::new(Mutex::new(queued));
    for _ in 0..threads {
        let queued = Arc::clone(&queued);
        let stopped = Arc::clone(stopped);
        let finished = finished.clone();
        let landing = Arc::clone(landing);
        std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                crate::render::mark_thread_off_frame();
                loop {
                    let job = queued.lock().unwrap_or_else(PoisonError::into_inner).recv();
                    let Ok(job) = job else {
                        break;
                    };
                    if stopped.load(Ordering::Acquire) {
                        break;
                    }
                    job();
                    landing.after_build();
                }
                let _ = finished.send(());
            })?;
    }
    Ok(jobs)
}

/// Where a renderer compiles its pipelines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PipelineCompilation {
    /// On background threads; a draw shows its placeholder, or a first
    /// screen's shared stand-in, until its own pipeline is ready.
    Background,
    /// Where each is first needed, so no draw uses a stand-in.
    Inline,
}

impl PipelineCompiler {
    pub(crate) fn for_compilation(compilation: PipelineCompilation) -> Self {
        match compilation {
            PipelineCompilation::Background => Self::spawn(),
            PipelineCompilation::Inline => Self::inactive(),
        }
    }

    /// A compiler that runs nothing: every resource compiles where it is
    /// first needed.
    pub(crate) fn inactive() -> Self {
        Self::default()
    }

    pub(crate) fn spawn() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            static BACKGROUND_PIPELINES: crate::debug_toggles::DebugToggle =
                crate::debug_toggles::DebugToggle::new("CRANPOSE_BACKGROUND_PIPELINES");
            if BACKGROUND_PIPELINES.equals("0") {
                return Self::inactive();
            }
            let stopped = Arc::new(AtomicBool::new(false));
            let (finished_tx, finished) = mpsc::channel();
            let warm_up_threads = warm_up_threads();
            let landing = Arc::new(Landing::default());
            let lanes = spawn_lane("cranpose-pipelines", 1, &stopped, &finished_tx, &landing)
                .and_then(|demanded| {
                    spawn_lane(
                        "cranpose-warm-up",
                        warm_up_threads,
                        &stopped,
                        &finished_tx,
                        &landing,
                    )
                    .map(|warm_up| (demanded, warm_up))
                });
            match lanes {
                Ok((demanded, warm_up)) => Self {
                    workers: Some(Arc::new(Workers {
                        demanded: Some(demanded),
                        warm_up: Some(warm_up),
                        stopped,
                        finished: Mutex::new(finished),
                        threads: 1 + warm_up_threads,
                        landing,
                    })),
                },
                Err(error) => {
                    log::error!("[gpu-pipeline] background compiler failed to spawn: {error}");
                    Self::inactive()
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self::inactive()
        }
    }

    /// Where the background threads say a pipeline landed; none for an
    /// inactive compiler, whose pipelines build where they are needed.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn landing(&self) -> Option<Arc<Landing>> {
        self.workers
            .as_ref()
            .map(|workers| Arc::clone(&workers.landing))
    }

    pub(crate) fn is_active(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.workers.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    /// Queues `job` on `lane`'s thread, behind the jobs already waiting
    /// there; an inactive compiler drops it, and the caller compiles at
    /// first use as before.
    pub(crate) fn enqueue(&self, lane: CompileLane, job: impl FnOnce() + CompilerSend + 'static) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(workers) = self.workers.as_ref() {
            let jobs = match lane {
                CompileLane::Demanded => workers.demanded.as_ref(),
                CompileLane::WarmUp => workers.warm_up.as_ref(),
            };
            workers.landing.queued.fetch_add(1, Ordering::AcqRel);
            if jobs.is_none_or(|jobs| jobs.send(Box::new(job)).is_err()) {
                log::error!("[gpu-pipeline] background compiler stopped unexpectedly");
            }
        }
        #[cfg(target_arch = "wasm32")]
        drop((lane, job));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "tests/pipeline_compiler_tests.rs"]
mod tests;
