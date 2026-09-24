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
#[path = "tests/pipeline_compiler_tests.rs"]
mod tests;
