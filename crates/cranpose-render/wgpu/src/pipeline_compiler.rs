#[cfg(not(target_arch = "wasm32"))]
use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
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
/// Each [`CompileLane`] starts its jobs in the order they were queued. One
/// thread takes only demanded jobs, so a pipeline a frame is waiting for
/// never queues behind warm-ups, which take seconds each on a slow device's
/// driver. While [`Self::spread_demand`] is on, the warm-up threads take a
/// waiting demanded job before their own, so a screen of new materials
/// compiles on all of them. The threads
/// end with the last handle, which skips the jobs they had not started and
/// waits, up to [`LANE_FINISH_WAIT`], for the ones they had.
#[derive(Clone, Default)]
pub(crate) struct PipelineCompiler {
    #[cfg(not(target_arch = "wasm32"))]
    workers: Option<Arc<Workers>>,
}

/// The jobs waiting for a compiler thread, per lane.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct Queues {
    demanded: VecDeque<Job>,
    warm_up: VecDeque<Job>,
    /// Set by the last handle: the threads start nothing more.
    closed: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl Queues {
    /// The next job a thread taking `warm_ups` or only demanded jobs
    /// starts; a warm-up thread takes demanded jobs only when `spread`.
    fn next(&mut self, warm_ups: bool, spread: bool) -> Option<Job> {
        let demanded = if warm_ups && !spread {
            None
        } else {
            self.demanded.pop_front()
        };
        demanded.or_else(|| warm_ups.then(|| self.warm_up.pop_front()).flatten())
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct Pool {
    queues: Mutex<Queues>,
    queued: Condvar,
    /// Whether the warm-up threads take demanded jobs; set outside the
    /// lock, read under it.
    spread: AtomicBool,
}

#[cfg(not(target_arch = "wasm32"))]
impl Pool {
    fn queues(&self) -> MutexGuard<'_, Queues> {
        self.queues.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Waits for the next job a thread taking `warm_ups` or only demanded
    /// jobs starts; `None` once the pool is closed.
    fn take(&self, warm_ups: bool) -> Option<Job> {
        let mut queues = self.queues();
        loop {
            if queues.closed {
                return None;
            }
            if let Some(job) = queues.next(warm_ups, self.spread.load(Ordering::Acquire)) {
                return Some(job);
            }
            queues = self
                .queued
                .wait(queues)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct Workers {
    pool: Arc<Pool>,
    /// Each thread says here that it has ended. Behind a mutex only so the
    /// handle is `Sync`; the last handle reads it without locking.
    finished: Mutex<Receiver<()>>,
    threads: usize,
    landing: Arc<Landing>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Workers {
    fn drop(&mut self) {
        let skipped = {
            let mut queues = self.pool.queues();
            queues.closed = true;
            (
                std::mem::take(&mut queues.demanded),
                std::mem::take(&mut queues.warm_up),
            )
        };
        drop(skipped);
        self.pool.queued.notify_all();
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

/// A compiler thread named `name` taking jobs from `pool`: demanded ones
/// only, or demanded ones first and then `warm_ups`.
#[cfg(not(target_arch = "wasm32"))]
fn spawn_thread(
    name: &str,
    warm_ups: bool,
    pool: &Arc<Pool>,
    finished: &Sender<()>,
    landing: &Arc<Landing>,
) -> std::io::Result<()> {
    let pool = Arc::clone(pool);
    let finished = finished.clone();
    let landing = Arc::clone(landing);
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            crate::render::mark_thread_off_frame();
            while let Some(job) = pool.take(warm_ups) {
                job();
                landing.after_build();
            }
            let _ = finished.send(());
        })
        .map(drop)
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
            let (finished_tx, finished) = mpsc::channel();
            let mut workers = Workers {
                pool: Arc::default(),
                finished: Mutex::new(finished),
                threads: 0,
                landing: Arc::default(),
            };
            let spawned = std::iter::once(("cranpose-pipelines", false))
                .chain(std::iter::repeat_n(
                    ("cranpose-warm-up", true),
                    warm_up_threads(),
                ))
                .try_for_each(|(name, warm_ups)| -> std::io::Result<()> {
                    spawn_thread(
                        name,
                        warm_ups,
                        &workers.pool,
                        &finished_tx,
                        &workers.landing,
                    )?;
                    workers.threads += 1;
                    Ok(())
                });
            match spawned {
                Ok(()) => Self {
                    workers: Some(Arc::new(workers)),
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

    /// Whether the warm-up threads take demanded jobs before their own. A
    /// frame that built a pipeline itself turns it off: on a driver that
    /// serializes much of each compile, compiles beside the frame's own slow
    /// it several times (a Mali blit: 33 ms alone, 180 ms beside three
    /// glass compiles), and the frame shows nothing until it is done.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn spread_demand(&self, spread: bool) {
        if let Some(workers) = self.workers.as_ref()
            && workers.pool.spread.swap(spread, Ordering::AcqRel) != spread
            && spread
        {
            // A thread that read the old value under the lock is waiting
            // by the time the lock is free.
            drop(workers.pool.queues());
            workers.pool.queued.notify_all();
        }
    }

    /// Queues `job` on `lane`'s thread, behind the jobs already waiting
    /// there; an inactive compiler drops it, and the caller compiles at
    /// first use as before.
    pub(crate) fn enqueue(&self, lane: CompileLane, job: impl FnOnce() + CompilerSend + 'static) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(workers) = self.workers.as_ref() {
            workers.landing.queued.fetch_add(1, Ordering::AcqRel);
            let mut queues = workers.pool.queues();
            match lane {
                CompileLane::Demanded => queues.demanded.push_back(Box::new(job)),
                CompileLane::WarmUp => queues.warm_up.push_back(Box::new(job)),
            }
            drop(queues);
            // A warm-up wakes every thread: the one taking only demanded
            // jobs may be the one woken, and it would leave the job queued.
            workers.pool.queued.notify_all();
        }
        #[cfg(target_arch = "wasm32")]
        drop((lane, job));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "tests/pipeline_compiler_tests.rs"]
pub(crate) mod tests;
