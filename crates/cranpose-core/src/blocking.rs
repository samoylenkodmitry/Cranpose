use std::{
    future::Future,
    num::NonZeroUsize,
    pin::Pin,
    sync::OnceLock,
    task::{Context, Poll},
    time::Duration,
};

use crate::runtime::{TaskHandle, current_runtime_handle};

#[cfg(not(target_arch = "wasm32"))]
mod native;

/// Failure to admit or complete blocking work.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum BlockingError {
    /// An immediate `try_submit` found a full queue. The closure has not run.
    #[error("the blocking executor is at capacity")]
    Saturated,
    /// The executor stopped accepting work and discarded its waiting jobs.
    #[error("the blocking executor is shut down")]
    Shutdown,
    /// No worker could be started. The closure has not run.
    #[error("a blocking worker could not start")]
    WorkerUnavailable,
    /// The closure unwound. With panic=abort, a panic still ends the process.
    #[error("blocking work panicked")]
    Panicked,
    /// This target has no supported executor for blocking closures.
    #[error("blocking work is unsupported on this target")]
    Unsupported,
    /// A UI callback was requested without a live composition runtime.
    #[error("a blocking UI callback needs a live runtime")]
    NoRuntime,
}

/// Bounds for a native blocking executor. Running closures cannot be interrupted.
#[derive(Clone, Copy, Debug)]
pub struct BlockingExecutorConfig {
    /// Maximum concurrent worker threads, created only when work needs them.
    pub max_threads: NonZeroUsize,
    /// Maximum waiting jobs, in addition to jobs already running.
    pub max_queued: NonZeroUsize,
    /// How long an idle worker remains available. Zero retires it immediately.
    pub idle_timeout: Duration,
}

impl Default for BlockingExecutorConfig {
    fn default() -> Self {
        let max_threads = std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
        Self {
            max_threads,
            max_queued: max_threads.saturating_mul(NonZeroUsize::new(4).expect("nonzero")),
            idle_timeout: Duration::from_secs(30),
        }
    }
}

/// A bounded, lazily started executor for synchronous work.
///
/// Clones share one pool. Dropping the last executor or calling `shutdown`
/// rejects new submissions and fails waiting jobs; running jobs may finish.
/// The default limits use the available CPU count and four waiting jobs per CPU.
/// Web submissions return `Unsupported` without executing the closure.
#[derive(Clone)]
pub struct BlockingExecutor {
    #[cfg(not(target_arch = "wasm32"))]
    inner: std::sync::Arc<native::Executor>,
}

impl Default for BlockingExecutor {
    fn default() -> Self {
        Self::new(BlockingExecutorConfig::default())
    }
}

impl BlockingExecutor {
    /// Creates an executor without starting threads or reserving queue storage.
    pub fn new(config: BlockingExecutorConfig) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self {
                inner: std::sync::Arc::new(native::Executor::new(config)),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = config;
            Self {}
        }
    }

    /// Admits work without waiting for capacity, or returns `Saturated`.
    ///
    /// The returned future owns the job: dropping it removes waiting work and
    /// releases its captures. A job already taken by a worker may finish.
    pub fn try_submit<T, F>(&self, work: F) -> Result<BlockingTask<T>, BlockingError>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.inner
                .try_submit(work)
                .map(|inner| BlockingTask { inner })
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = work;
            Err(BlockingError::Unsupported)
        }
    }

    /// Waits asynchronously for queue capacity, then runs work and returns its result.
    ///
    /// A full queue suspends this future without blocking its polling thread or
    /// returning `Saturated`. Dropping the future cancels admission or queued work.
    /// Each suspended caller retains its closure until admission or cancellation;
    /// applications should bound the number of concurrent producer tasks.
    pub async fn submit<T, F>(&self, work: F) -> Result<T, BlockingError>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let inner = self.inner.submit(work).await?;
            BlockingTask { inner }.await
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = work;
            Err(BlockingError::Unsupported)
        }
    }

    /// Stops admission and resolves waiting jobs with `Shutdown`.
    /// Does not wait for or interrupt running closures.
    pub fn shutdown(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.inner.shutdown();
    }
}

/// Completion of an admitted job. Dropping it cancels work still waiting to run.
#[must_use = "dropping the task cancels waiting work"]
pub struct BlockingTask<T> {
    #[cfg(not(target_arch = "wasm32"))]
    inner: native::Pending<T>,
    #[cfg(target_arch = "wasm32")]
    marker: std::marker::PhantomData<fn() -> T>,
}

impl<T> Future for BlockingTask<T> {
    type Output = Result<T, BlockingError>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.get_mut().inner.poll(context)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (self, context);
            Poll::Ready(Err(BlockingError::Unsupported))
        }
    }
}

fn shared_executor() -> &'static BlockingExecutor {
    static EXECUTOR: OnceLock<BlockingExecutor> = OnceLock::new();
    EXECUTOR.get_or_init(BlockingExecutor::default)
}

/// Runs synchronous work on the shared bounded executor when first polled.
///
/// A full queue suspends this future until capacity is available, keeping the UI
/// thread free. Shutdown and unwinding panics are reported as errors. Dropping
/// the future releases its captures and cancels work not yet started. Running
/// closures cannot be interrupted. Bound concurrent callers to bound the memory
/// retained by their suspended futures.
/// On Web this returns `Unsupported` without running the closure.
#[expect(non_snake_case)]
pub async fn withBlocking<T, F>(work: F) -> Result<T, BlockingError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    shared_executor().submit(work).await
}

/// Starts blocking work and delivers its result on the current runtime's UI
/// thread. A full queue waits asynchronously inside the runtime-owned task.
/// Execution errors are delivered to `on_ui`; only a missing runtime is returned
/// immediately without invoking the callback.
///
/// The runtime owns the returned task until completion. Calling its `cancel`
/// method, or dropping the runtime, cancels waiting work and the callback.
/// For screen ownership, prefer `rememberCoroutineScope().launch` with
/// `withBlocking`. Without a live runtime this returns `NoRuntime`.
#[expect(non_snake_case)]
pub fn launchBlocking<T>(
    work: impl FnOnce() -> T + Send + 'static,
    on_ui: impl FnOnce(Result<T, BlockingError>) + 'static,
) -> Result<TaskHandle, BlockingError>
where
    T: Send + 'static,
{
    let runtime = current_runtime_handle().ok_or(BlockingError::NoRuntime)?;
    runtime
        .spawn_ui(async move { on_ui(withBlocking(work).await) })
        .ok_or(BlockingError::NoRuntime)
}
