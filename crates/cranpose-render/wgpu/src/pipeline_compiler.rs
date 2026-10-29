use std::collections::VecDeque;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{
    Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{self, Receiver, Sender},
};
#[cfg(target_arch = "wasm32")]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::frame_packet::SceneMotion;

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
/// first frame waiting for them one at a time. Metal compiles in the
/// system's compiler service, side by side, while the thread that asked
/// waits: with no Metal shader cached, the gauntlet's first frame on an
/// Apple M3 Pro (12 cores) waits for 14 pipelines of 40 to 140 ms each, up
/// to five of them for one pass, and on 10 threads each pass starts all of
/// its own at once.
#[cfg(not(target_arch = "wasm32"))]
fn warm_up_threads(backend: wgpu::Backend) -> usize {
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    match backend {
        wgpu::Backend::Metal => cores.saturating_sub(2).clamp(3, 10),
        _ => (cores / 2).clamp(1, 3),
    }
}

#[cfg(not(target_arch = "wasm32"))]
type Job = Box<dyn FnOnce() + Send + 'static>;
#[cfg(target_arch = "wasm32")]
type Job = Box<dyn FnOnce()>;

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
    /// A frame drew without this pipeline, with a stand-in, a placeholder or
    /// fallback triangles, or left its draw out, until it is ready.
    Demanded,
    /// No frame has asked for this pipeline yet; it is built ahead of its
    /// first use.
    WarmUp,
    /// Only a later frame may ask for this pipeline: held until the frame
    /// queuing it ends, then queued behind the warm-ups, so it never takes a
    /// thread from a pipeline the frame draws with.
    Later,
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
    #[cfg(target_arch = "wasm32")]
    discovery: std::rc::Rc<DiscoveryFrame>,
    // On WebGL, the builds that link one a frame (see `paced`), shared by
    // every handle, and whether this handle queues there.
    #[cfg(target_arch = "wasm32")]
    paced: std::rc::Rc<std::cell::RefCell<Queues>>,
    #[cfg(target_arch = "wasm32")]
    paced_landing: std::rc::Rc<PacedLanding>,
    #[cfg(target_arch = "wasm32")]
    pacing: bool,
}

// On WebGL, whether the browser's GPU process is still on the frame that
// ran the last paced build, that build's link among the frame's work,
// whether the next submit is to watch for it, and the moving frames a
// demanded build has waited since the last build.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Default)]
pub(crate) struct PacedLanding {
    in_flight: Arc<AtomicBool>,
    watch: std::cell::Cell<bool>,
    moving_frames: std::cell::Cell<u32>,
}

// The moving frames a demanded paced build waits for a still one at most: a
// scene that never stands still, as an endless animation keeps it, gets its
// pipelines one per this many frames.
#[cfg(any(target_arch = "wasm32", test))]
const MOVING_FRAMES_PER_BUILD: u32 = 60;

#[cfg(any(target_arch = "wasm32", test))]
impl PacedLanding {
    pub(crate) fn may_build(&self) -> bool {
        !self.in_flight.load(Ordering::Acquire)
    }

    pub(crate) fn built(&self) {
        self.in_flight.store(true, Ordering::Release);
        self.watch.set(true);
        self.moving_frames.set(0);
    }

    // Notes the end of a frame whose scene moved: whether `demanded` builds
    // have waited `MOVING_FRAMES_PER_BUILD` such frames, so the next runs.
    pub(crate) fn waited_out(&self, demanded: bool) -> bool {
        let frames = if demanded {
            self.moving_frames.get() + 1
        } else {
            0
        };
        self.moving_frames.set(frames);
        frames >= MOVING_FRAMES_PER_BUILD
    }

    // Called after a submit: once the GPU has done the work submitted so far,
    // a later submit's maintenance lets the next build run.
    pub(crate) fn watch(&self, queue: &wgpu::Queue) {
        if self.watch.replace(false) {
            let in_flight = Arc::clone(&self.in_flight);
            queue.on_submitted_work_done(move || {
                in_flight.store(false, Ordering::Release);
            });
        }
    }
}

/// On the web, the frame drawn while it takes its pipelines from a pipeline
/// discovery, and the builds it holds back to its end.
#[cfg(target_arch = "wasm32")]
#[derive(Default)]
struct DiscoveryFrame {
    drawing: std::cell::Cell<bool>,
    held: std::cell::RefCell<Vec<Job>>,
}

/// The jobs waiting for a compiler thread, or on WebGL for a frame's end, per lane.
#[derive(Default)]
struct Queues {
    demanded: VecDeque<Job>,
    warm_up: VecDeque<Job>,
    /// Set by the last handle: the threads start nothing more.
    #[cfg(not(target_arch = "wasm32"))]
    closed: bool,
}

impl Queues {
    fn push(&mut self, lane: CompileLane, job: Job) {
        match lane {
            CompileLane::Demanded => self.demanded.push_back(job),
            CompileLane::WarmUp | CompileLane::Later => self.warm_up.push_back(job),
        }
    }

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
    /// The jobs on [`CompileLane::Later`] the drawing frame holds. Behind a
    /// mutex only so the handle is `Sync`; only the drawing thread takes it.
    later: Mutex<Vec<Job>>,
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
                std::mem::take(self.later.get_mut().unwrap_or_else(PoisonError::into_inner)),
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
    /// On background threads; until its own pipeline is ready, a draw shows
    /// its placeholder, a first screen's shared stand-in or, for a shape,
    /// fallback triangles, and is left out when it has none of them.
    Background,
    /// Where each is first needed, so no draw uses a stand-in.
    Inline,
}

impl PipelineCompiler {
    pub(crate) fn for_compilation(
        compilation: PipelineCompilation,
        backend: wgpu::Backend,
    ) -> Self {
        match compilation {
            PipelineCompilation::Background => Self::spawn(backend),
            PipelineCompilation::Inline => Self::inactive(),
        }
    }

    /// A compiler that runs nothing: every resource compiles where it is
    /// first needed.
    pub(crate) fn inactive() -> Self {
        Self::default()
    }

    pub(crate) fn spawn(
        #[cfg_attr(
            target_arch = "wasm32",
            expect(unused_variables, reason = "the web build compiles on its one thread")
        )]
        backend: wgpu::Backend,
    ) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if cranpose_core::DIAGNOSTICS.background_pipelines.equals("0") {
                return Self::inactive();
            }
            let (finished_tx, finished) = mpsc::channel();
            let mut workers = Workers {
                pool: Arc::default(),
                later: Mutex::default(),
                finished: Mutex::new(finished),
                threads: 0,
                landing: Arc::default(),
            };
            let spawned = std::iter::once(("cranpose-pipelines", false))
                .chain(std::iter::repeat_n(
                    ("cranpose-warm-up", true),
                    warm_up_threads(backend),
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

    /// How many threads take warm-ups.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn warm_up_threads(&self) -> usize {
        self.workers
            .as_ref()
            .map_or(0, |workers| workers.threads.saturating_sub(1))
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
            self.pacing
        }
    }

    // A handle whose builds link one a frame, at the frame's end, where a
    // link holds the browser's GPU process and every frame behind it: WebGL
    // without parallel compile, as Chrome on a Huawei Mate 20 X runs it,
    // where the first open of the demo's Liquid UI tab linked 16 glass
    // programs in a row, ~250 ms each, and the page drew nothing for 4.4 s.
    // A link runs only after a frame whose scene stood still, the app
    // changing nothing it shows, so no input or animation waits behind it;
    // a draw asking for a pipeline that is not built draws its placeholder
    // meanwhile. A scene that never stands still still gets its demanded
    // builds, one per `MOVING_FRAMES_PER_BUILD` frames, and a held pointer
    // none. Elsewhere it is this compiler.
    #[cfg_attr(
        not(target_arch = "wasm32"),
        expect(unused_variables, reason = "native builds compile on threads")
    )]
    pub(crate) fn paced(&self, backend: wgpu::Backend) -> Self {
        #[cfg(target_arch = "wasm32")]
        if backend == wgpu::Backend::Gl {
            return Self {
                pacing: true,
                ..self.clone()
            };
        }
        self.clone()
    }

    /// Whether a pipeline queued now is built ahead of the draw that needs it: on the
    /// compiler's threads where they run, on WebGL at a frame's end, and elsewhere on the
    /// web at the end of a frame that takes its pipelines from a pipeline discovery.
    pub(crate) fn builds_ahead(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.is_active()
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.pacing || self.discovery.drawing.get()
        }
    }

    /// Starts a frame that takes its pipelines from a pipeline discovery:
    /// the jobs queued until [`Self::end_frame`] run there, after the frame's
    /// own builds, so the browser compiles those first.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn begin_discovery_frame(&self) {
        self.discovery.drawing.set(true);
    }

    /// Ends the drawing frame: queues the jobs it held on [`CompileLane::Later`] behind the
    /// warm-ups, or on the web runs the jobs a discovery frame held, and on WebGL the next one
    /// waiting as the frame's `scene_motion` allows (see [`Self::paced`]).
    #[cfg_attr(
        not(target_arch = "wasm32"),
        expect(unused_variables, reason = "threads build ahead whatever the frame")
    )]
    pub(crate) fn end_frame(&self, scene_motion: SceneMotion) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(workers) = self.workers.as_ref() {
            let mut later = workers.later.lock().unwrap_or_else(PoisonError::into_inner);
            if later.is_empty() {
                return;
            }
            workers
                .landing
                .queued
                .fetch_add(later.len() as u64, Ordering::AcqRel);
            workers.pool.queues().warm_up.extend(later.drain(..));
            drop(later);
            workers.pool.queued.notify_all();
        }
        #[cfg(target_arch = "wasm32")]
        {
            if self.discovery.drawing.replace(false) {
                let held = std::mem::take(&mut *self.discovery.held.borrow_mut());
                for job in held {
                    job();
                }
            }
            // A link is read at the first submit that draws with it, so the
            // page goes on and runs the next paced build a frame later while
            // the GPU process still links the last: in Chrome on a Mate 20 X
            // six glass and blur links piled up there, and the first frame
            // drawing with one waited 790 ms for all six. The next build waits
            // until the GPU process has done the frame that ran the last.
            let warm_ups = match scene_motion {
                SceneMotion::Still => Some(true),
                SceneMotion::Moving => {
                    let demanded = !self.paced.borrow().demanded.is_empty();
                    self.paced_landing.waited_out(demanded).then_some(false)
                }
                SceneMotion::Held => None,
            };
            let paced = warm_ups
                .filter(|_| self.paced_landing.may_build())
                .and_then(|warm_ups| self.paced.borrow_mut().next(warm_ups, true));
            if let Some(job) = paced {
                job();
                self.paced_landing.built();
            }
        }
    }

    // After a frame's submit: when a frame ran a paced build, the next one
    // waits until the browser's GPU process has done the work submitted so
    // far, the build's link among it (see `end_frame`).
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn watch_paced_landing(&self, queue: &wgpu::Queue) {
        self.paced_landing.watch(queue);
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

    /// Queues `job` on `lane`'s thread, behind the jobs already waiting there, or holds
    /// it to the frame's end on [`CompileLane::Later`]; on WebGL it waits for its turn at
    /// a frame's end, and elsewhere on the web for the end of a frame that takes its
    /// pipelines from a pipeline discovery. An inactive compiler drops it, and the
    /// caller compiles at first use as before.
    pub(crate) fn enqueue(&self, lane: CompileLane, job: impl FnOnce() + CompilerSend + 'static) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(workers) = self.workers.as_ref() {
            if lane == CompileLane::Later {
                workers
                    .later
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(Box::new(job));
                return;
            }
            workers.landing.queued.fetch_add(1, Ordering::AcqRel);
            workers.pool.queues().push(lane, Box::new(job));
            // A warm-up wakes every thread: the one taking only demanded
            // jobs may be the one woken, and it would leave the job queued.
            workers.pool.queued.notify_all();
        }
        #[cfg(target_arch = "wasm32")]
        if self.pacing {
            self.paced.borrow_mut().push(lane, Box::new(job));
        } else if self.discovery.drawing.get() {
            self.discovery.held.borrow_mut().push(Box::new(job));
        } else {
            drop((lane, job));
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "tests/pipeline_compiler_tests.rs"]
pub(crate) mod tests;
