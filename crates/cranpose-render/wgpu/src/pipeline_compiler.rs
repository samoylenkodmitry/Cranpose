#[cfg(not(target_arch = "wasm32"))]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Sender},
};

#[cfg(not(target_arch = "wasm32"))]
type Job = Box<dyn FnOnce() + Send + 'static>;

/// What a value crossing to the compiler thread must be: `Send` where that
/// thread exists, nothing on the web, where wgpu handles are single-threaded
/// and the compiler runs nothing.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) trait CompilerSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + ?Sized> CompilerSend for T {}
#[cfg(target_arch = "wasm32")]
pub(crate) trait CompilerSend {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> CompilerSend for T {}

/// What a value shared with the compiler thread must be: `Sync` where that
/// thread exists, nothing on the web.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) trait CompilerSync: Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Sync + ?Sized> CompilerSync for T {}
#[cfg(target_arch = "wasm32")]
pub(crate) trait CompilerSync {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> CompilerSync for T {}

/// Runs pipeline creation on one background thread, in the order it was
/// queued, so a draw finds its pipeline ready instead of paying the driver's
/// compile inside the frame. The thread ends with the last handle and skips
/// the jobs it had not started.
#[derive(Clone, Default)]
pub(crate) struct PipelineCompiler {
    #[cfg(not(target_arch = "wasm32"))]
    worker: Option<Arc<Worker>>,
}

#[cfg(not(target_arch = "wasm32"))]
struct Worker {
    jobs: Sender<Job>,
    stopped: Arc<AtomicBool>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Worker {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}

impl PipelineCompiler {
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
            let (jobs, queued) = mpsc::channel::<Job>();
            let stopped = Arc::new(AtomicBool::new(false));
            let worker_stopped = Arc::clone(&stopped);
            let spawned = std::thread::Builder::new()
                .name("cranpose-pipelines".into())
                .spawn(move || {
                    crate::render::mark_thread_off_frame();
                    while let Ok(job) = queued.recv() {
                        if worker_stopped.load(Ordering::Acquire) {
                            break;
                        }
                        job();
                    }
                });
            match spawned {
                Ok(_) => Self {
                    worker: Some(Arc::new(Worker { jobs, stopped })),
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

    pub(crate) fn is_active(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.worker.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    /// Queues `job` behind the jobs already waiting; an inactive compiler
    /// drops it, and the caller compiles at first use as before.
    pub(crate) fn enqueue(&self, job: impl FnOnce() + CompilerSend + 'static) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(worker) = self.worker.as_ref()
            && worker.jobs.send(Box::new(job)).is_err()
        {
            log::error!("[gpu-pipeline] background compiler stopped unexpectedly");
        }
        #[cfg(target_arch = "wasm32")]
        drop(job);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    use super::PipelineCompiler;

    #[test]
    fn jobs_run_in_queue_order_off_the_calling_thread() {
        let compiler = PipelineCompiler::spawn();
        assert!(compiler.is_active());
        let (finished, observed) = mpsc::channel();
        let caller = std::thread::current().id();
        for index in 0..3 {
            let finished = finished.clone();
            compiler.enqueue(move || {
                finished.send((index, std::thread::current().id())).unwrap();
            });
        }
        for expected in 0..3 {
            let (index, thread) = observed.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(index, expected);
            assert_ne!(thread, caller);
        }
    }

    #[test]
    fn dropping_every_handle_skips_the_jobs_not_started() {
        let compiler = PipelineCompiler::spawn();
        let handle = compiler.clone();
        let (started, observed) = mpsc::channel();
        let (release, blocked) = mpsc::channel::<()>();
        compiler.enqueue(move || {
            started.send("first").unwrap();
            blocked.recv().unwrap();
        });
        let (ran, second_ran) = mpsc::channel();
        compiler.enqueue(move || ran.send("second").unwrap());
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(5)).unwrap(),
            "first"
        );
        drop(compiler);
        assert!(handle.is_active(), "a surviving handle keeps the worker");
        drop(handle);
        release.send(()).unwrap();
        assert_eq!(
            second_ran.recv_timeout(Duration::from_millis(500)),
            Err(mpsc::RecvTimeoutError::Disconnected),
            "the queued job must be dropped, not run"
        );
    }

    #[test]
    fn an_inactive_compiler_drops_jobs() {
        let compiler = PipelineCompiler::inactive();
        assert!(!compiler.is_active());
        let (ran, observed) = mpsc::channel();
        compiler.enqueue(move || ran.send(()).unwrap());
        let deadline = Instant::now() + Duration::from_millis(100);
        assert!(observed.recv_timeout(deadline - Instant::now()).is_err());
    }
}
