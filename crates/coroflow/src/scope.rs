use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};

use crate::{
    dispatcher::{ConfinedDispatcher, Dispatcher},
    job::{Job, JobOutcome},
    sync::{OneshotReceiver, lock, oneshot},
    task::{spawn_local, spawn_send},
};

/// The result of a `suspend fun` declared on a trait object: a boxed future
/// that background dispatchers can run.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// Something coroutines of type `F` can be launched into.
///
/// [`CoroutineScope`] accepts only `Send` futures and [`MainScope`] accepts any
/// `'static` future. Operators that launch work, such as
/// [`state_in`](crate::FlowExt::state_in), are generic over this trait, so the
/// compiler infers from the flow itself whether it can run on a background
/// scope — nothing is annotated `Send` by hand.
pub trait Spawn<F: Future<Output = ()>> {
    /// Launches `future` and returns its [`Job`].
    fn spawn(&self, future: F) -> Job;
}

#[derive(Default)]
struct ScopeCore {
    jobs: Mutex<Vec<Job>>,
    cancelled: AtomicBool,
}

impl ScopeCore {
    fn track(&self, job: Job) -> Job {
        {
            let mut jobs = lock(&self.jobs);
            if !self.cancelled.load(Ordering::Acquire) {
                jobs.retain(|tracked| tracked.outcome().is_none());
                jobs.push(job.clone());
                return job;
            }
        }
        job.cancel();
        job
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        let jobs = std::mem::take(&mut *lock(&self.jobs));
        for job in jobs {
            job.cancel();
        }
    }

    fn is_active(&self) -> bool {
        !self.cancelled.load(Ordering::Acquire)
    }
}

/// Owns coroutines launched on a dispatcher of type `D` — Kotlin's
/// `CoroutineScope` with a `SupervisorJob`.
///
/// Use it as [`CoroutineScope`] for background work or [`MainScope`] for work
/// confined to the main thread. Dropping the scope cancels everything it
/// launched, so the object that owns a scope cannot leak work past its own
/// lifetime.
pub struct Scope<D> {
    core: Arc<ScopeCore>,
    dispatcher: D,
}

/// A scope for background work: every launched future must be `Send`.
///
/// ```compile_fail
/// use std::rc::Rc;
/// let scope = coroflow::CoroutineScope::new(coroflow::Dispatchers::default_pool());
/// let local = Rc::new(1);
/// scope.launch(async move { drop(local); });
/// ```
pub type CoroutineScope = Scope<Dispatcher>;

/// A scope confined to the main thread — Kotlin's `viewModelScope`.
///
/// Futures launched here may hold `Rc`, `RefCell` borrows and other
/// thread-bound state because they never leave the thread of the
/// [`ConfinedDispatcher`]; the scope itself cannot leave that thread either.
pub type MainScope = Scope<ConfinedDispatcher>;

impl<D> Scope<D> {
    /// A scope whose coroutines run on `dispatcher`.
    pub fn new(dispatcher: D) -> Self {
        Self {
            core: Arc::default(),
            dispatcher,
        }
    }

    /// Cancels every coroutine launched here and refuses new ones.
    pub fn cancel(&self) {
        self.core.cancel();
    }

    /// Whether the scope still accepts work.
    pub fn is_active(&self) -> bool {
        self.core.is_active()
    }

    /// The dispatcher coroutines launched here run on.
    pub fn dispatcher(&self) -> &D {
        &self.dispatcher
    }

    fn launch_with(&self, spawn: impl FnOnce(&D) -> Job) -> Job {
        if !self.core.is_active() {
            return Job::finished(JobOutcome::Cancelled);
        }
        self.core.track(spawn(&self.dispatcher))
    }
}

impl<D> Drop for Scope<D> {
    fn drop(&mut self) {
        self.core.cancel();
    }
}

impl Scope<Dispatcher> {
    /// Launches `future` — Kotlin's `launch`.
    pub fn launch<F>(&self, future: F) -> Job
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.launch_with(|dispatcher| spawn_send(dispatcher, future))
    }

    /// A weak, cloneable handle for launching from inside the scope's own
    /// coroutines without keeping the scope alive.
    pub fn handle(&self) -> ScopeHandle {
        ScopeHandle {
            core: Arc::downgrade(&self.core),
            dispatcher: self.dispatcher.clone(),
        }
    }
}

impl Scope<ConfinedDispatcher> {
    /// Launches `future` on the main thread — Kotlin's `launch`.
    pub fn launch<F>(&self, future: F) -> Job
    where
        F: Future<Output = ()> + 'static,
    {
        self.launch_with(|dispatcher| spawn_local(dispatcher, future))
    }
}

impl<F: Future<Output = ()> + Send + 'static> Spawn<F> for Scope<Dispatcher> {
    fn spawn(&self, future: F) -> Job {
        self.launch(future)
    }
}

impl<F: Future<Output = ()> + 'static> Spawn<F> for Scope<ConfinedDispatcher> {
    fn spawn(&self, future: F) -> Job {
        self.launch(future)
    }
}

/// A weak handle to a [`CoroutineScope`], see [`Scope::handle`].
#[derive(Clone)]
pub struct ScopeHandle {
    core: Weak<ScopeCore>,
    dispatcher: Dispatcher,
}

impl ScopeHandle {
    /// Launches `future` in the scope; once the scope is gone the returned job
    /// is already cancelled.
    pub fn launch<F>(&self, future: F) -> Job
    where
        F: Future<Output = ()> + Send + 'static,
    {
        match self.core.upgrade() {
            Some(core) if core.is_active() => core.track(spawn_send(&self.dispatcher, future)),
            _ => Job::finished(JobOutcome::Cancelled),
        }
    }
}

/// The coroutine switched to another dispatcher ended without a result
/// because it panicked or its dispatcher dropped it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the coroutine ended without producing a result")]
pub struct TaskFailed;

/// Runs `future` on `dispatcher` and waits for its result — Kotlin's
/// `withContext`.
///
/// Dropping the returned future cancels the work.
pub fn with_context<T, F>(dispatcher: &Dispatcher, future: F) -> WithContext<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let (sender, receiver) = oneshot();
    let job = spawn_send(dispatcher, async move {
        sender.send(future.await);
    });
    WithContext { job, receiver }
}

/// The future returned by [`with_context`].
pub struct WithContext<T> {
    job: Job,
    receiver: OneshotReceiver<T>,
}

impl<T> Future for WithContext<T> {
    type Output = Result<T, TaskFailed>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<T, TaskFailed>> {
        let this = self.get_mut();
        Pin::new(&mut this.receiver)
            .poll(cx)
            .map(|value| value.ok_or(TaskFailed))
    }
}

impl<T> Drop for WithContext<T> {
    fn drop(&mut self) {
        self.job.cancel();
    }
}
