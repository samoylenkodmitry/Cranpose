#[cfg(not(target_arch = "wasm32"))]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Sender},
};

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
/// Each [`CompileLane`] has its own thread and runs its jobs in the order
/// they were queued, so a pipeline a frame is waiting for never queues
/// behind warm-ups, which take seconds each on a slow device's driver. The
/// threads end with the last handle and skip the jobs they had not started.
#[derive(Clone, Default)]
pub(crate) struct PipelineCompiler {
    #[cfg(not(target_arch = "wasm32"))]
    workers: Option<Arc<Workers>>,
}

#[cfg(not(target_arch = "wasm32"))]
struct Workers {
    demanded: Sender<Job>,
    warm_up: Sender<Job>,
    stopped: Arc<AtomicBool>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Workers {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_lane(name: &str, stopped: &Arc<AtomicBool>) -> std::io::Result<Sender<Job>> {
    let (jobs, queued) = mpsc::channel::<Job>();
    let stopped = Arc::clone(stopped);
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            crate::render::mark_thread_off_frame();
            while let Ok(job) = queued.recv() {
                if stopped.load(Ordering::Acquire) {
                    break;
                }
                job();
            }
        })?;
    Ok(jobs)
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
            let stopped = Arc::new(AtomicBool::new(false));
            let lanes = spawn_lane("cranpose-pipelines", &stopped).and_then(|demanded| {
                spawn_lane("cranpose-warm-up", &stopped).map(|warm_up| (demanded, warm_up))
            });
            match lanes {
                Ok((demanded, warm_up)) => Self {
                    workers: Some(Arc::new(Workers {
                        demanded,
                        warm_up,
                        stopped,
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
                CompileLane::Demanded => &workers.demanded,
                CompileLane::WarmUp => &workers.warm_up,
            };
            if jobs.send(Box::new(job)).is_err() {
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
