use std::{
    future::Future,
    ops::Deref,
    pin::Pin,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};

use crate::{
    dispatcher::{ConfinedDispatcher, Dispatcher, Dispatchers, current_dispatcher},
    job::{Job, JobOutcome, JobParent, Launch},
    select::{Either, select},
    sync::{OneshotReceiver, OneshotSender, lock, oneshot},
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

type FailureHandler = Arc<dyn Fn(&str) + Send + Sync>;

struct ScopeCore {
    jobs: Mutex<Vec<Job>>,
    cancelled: AtomicBool,
    supervisor: bool,
    failed: AtomicBool,
    waiters: Mutex<Vec<Waker>>,
    handler: Mutex<Option<FailureHandler>>,
}

impl ScopeCore {
    fn new(supervisor: bool) -> Arc<Self> {
        Arc::new(Self {
            jobs: Mutex::default(),
            cancelled: AtomicBool::new(false),
            supervisor,
            failed: AtomicBool::new(false),
            waiters: Mutex::default(),
            handler: Mutex::default(),
        })
    }

    fn children(&self) -> Vec<Job> {
        let mut jobs = lock(&self.jobs);
        jobs.retain(|job| job.outcome().is_none());
        jobs.clone()
    }

    fn track(core: &Arc<Self>, job: Job) -> Job {
        {
            let mut jobs = lock(&core.jobs);
            if !core.cancelled.load(Ordering::Acquire) {
                jobs.retain(|tracked| tracked.outcome().is_none());
                jobs.push(job.clone());
                drop(jobs);
                job.set_parent(Arc::downgrade(core) as Weak<dyn JobParent>);
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

    fn has_failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    fn watch(&self, cx: &mut Context<'_>) {
        let mut waiters = lock(&self.waiters);
        if !waiters.iter().any(|waiter| waiter.will_wake(cx.waker())) {
            waiters.push(cx.waker().clone());
        }
    }

    fn poll_failed(&self, cx: &mut Context<'_>) -> Poll<()> {
        self.watch(cx);
        if self.has_failed() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    fn poll_idle(&self, cx: &mut Context<'_>) -> Poll<()> {
        self.watch(cx);
        let mut jobs = lock(&self.jobs);
        jobs.retain(|job| job.outcome().is_none());
        if jobs.is_empty() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

impl JobParent for ScopeCore {
    fn child_finished(&self, outcome: JobOutcome, failure: Option<&str>) {
        if let Some(message) = failure {
            let handler = lock(&self.handler).clone();
            if let Some(handler) = handler {
                handler(message);
            }
        }
        if outcome == JobOutcome::Panicked
            && !self.supervisor
            && !self.failed.swap(true, Ordering::AcqRel)
        {
            self.cancel();
        }
        let waiters = std::mem::take(&mut *lock(&self.waiters));
        for waiter in waiters {
            waiter.wake();
        }
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
    /// A scope whose coroutines run on `dispatcher`. A failing coroutine does
    /// not affect its siblings.
    pub fn new(dispatcher: D) -> Self {
        Self::with_policy(dispatcher, true)
    }

    fn with_policy(dispatcher: D, supervisor: bool) -> Self {
        Self {
            core: ScopeCore::new(supervisor),
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

    /// The coroutines launched here that are still running — Kotlin's
    /// `coroutineContext.job.children`.
    pub fn children(&self) -> Vec<Job> {
        self.core.children()
    }

    /// Calls `handler` with the panic message of every coroutine launched here
    /// that panics — Kotlin's `CoroutineExceptionHandler`. A coroutine started
    /// with `async_` reports through its [`Deferred`] instead.
    #[must_use]
    pub fn with_exception_handler(self, handler: impl Fn(&str) + Send + Sync + 'static) -> Self {
        *lock(&self.core.handler) = Some(Arc::new(handler));
        self
    }

    fn launch_with(&self, spawn: impl FnOnce(&D) -> Job) -> Job {
        if !self.core.is_active() {
            return Job::finished(JobOutcome::Cancelled);
        }
        ScopeCore::track(&self.core, spawn(&self.dispatcher))
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
        self.start(future, Launch::EAGER)
    }

    /// Launches `future` without running it until [`Job::start`] or
    /// [`Job::join`] — Kotlin's `launch(start = CoroutineStart.LAZY)`.
    pub fn launch_lazy<F>(&self, future: F) -> Job
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.start(future, Launch::LAZY)
    }

    fn start<F>(&self, future: F, launch: Launch) -> Job
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.launch_with(|dispatcher| spawn_send(dispatcher, future, launch))
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
        self.start(future, Launch::EAGER)
    }

    /// Launches `future` on the main thread without running it until
    /// [`Job::start`] or [`Job::join`] — Kotlin's
    /// `launch(start = CoroutineStart.LAZY)`.
    pub fn launch_lazy<F>(&self, future: F) -> Job
    where
        F: Future<Output = ()> + 'static,
    {
        self.start(future, Launch::LAZY)
    }

    fn start<F>(&self, future: F, launch: Launch) -> Job
    where
        F: Future<Output = ()> + 'static,
    {
        self.launch_with(|dispatcher| spawn_local(dispatcher, future, launch))
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
            Some(core) if core.is_active() => {
                ScopeCore::track(&core, spawn_send(&self.dispatcher, future, Launch::EAGER))
            }
            _ => Job::finished(JobOutcome::Cancelled),
        }
    }
}

/// The coroutine switched to another dispatcher ended without a result
/// because it panicked or its dispatcher dropped it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the coroutine ended without producing a result")]
pub struct TaskFailed;

/// The result of a coroutine started with `async_` — Kotlin's `Deferred`.
///
/// Awaiting it yields the coroutine's output, or [`TaskFailed`] if the
/// coroutine panicked or was cancelled first. Awaiting a lazily started one
/// starts it. It dereferences to its [`Job`], the way Kotlin's `Deferred`
/// extends `Job`. Dropping it does not cancel the coroutine; its scope still
/// owns it.
pub struct Deferred<T> {
    job: Job,
    receiver: OneshotReceiver<T>,
}

impl<T> Deferred<T> {
    fn spawn(spawn: impl FnOnce(OneshotSender<T>) -> Job) -> Self {
        let (sender, receiver) = oneshot();
        Self {
            job: spawn(sender),
            receiver,
        }
    }
}

impl<T> Deref for Deferred<T> {
    type Target = Job;

    fn deref(&self) -> &Job {
        &self.job
    }
}

impl<T> Future for Deferred<T> {
    type Output = Result<T, TaskFailed>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<T, TaskFailed>> {
        let this = self.get_mut();
        this.job.start();
        Pin::new(&mut this.receiver)
            .poll(cx)
            .map(|value| value.ok_or(TaskFailed))
    }
}

impl Scope<Dispatcher> {
    /// Starts computing `future` concurrently — Kotlin's `async`.
    pub fn async_<T, F>(&self, future: F) -> Deferred<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        Deferred::spawn(|sender| {
            self.start(async move { sender.send(future.await) }, Launch::DEFERRED)
        })
    }

    /// Like [`async_`](Scope::async_), but starts only when awaited or
    /// started — Kotlin's `async(start = CoroutineStart.LAZY)`.
    pub fn async_lazy<T, F>(&self, future: F) -> Deferred<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        Deferred::spawn(|sender| {
            self.start(
                async move { sender.send(future.await) },
                Launch::LAZY_DEFERRED,
            )
        })
    }
}

impl Scope<ConfinedDispatcher> {
    /// Starts computing `future` concurrently on the main thread — Kotlin's
    /// `async`.
    pub fn async_<T, F>(&self, future: F) -> Deferred<T>
    where
        F: Future<Output = T> + 'static,
        T: 'static,
    {
        Deferred::spawn(|sender| {
            self.start(async move { sender.send(future.await) }, Launch::DEFERRED)
        })
    }

    /// Like [`async_`](Scope::async_), but starts only when awaited or
    /// started on the main thread — Kotlin's
    /// `async(start = CoroutineStart.LAZY)`.
    pub fn async_lazy<T, F>(&self, future: F) -> Deferred<T>
    where
        F: Future<Output = T> + 'static,
        T: 'static,
    {
        Deferred::spawn(|sender| {
            self.start(
                async move { sender.send(future.await) },
                Launch::LAZY_DEFERRED,
            )
        })
    }
}

/// Waits until every job in `jobs` has finished — Kotlin's `joinAll`.
pub async fn join_all(jobs: impl IntoIterator<Item = Job>) {
    for job in jobs {
        job.join().await;
    }
}

/// Waits for every deferred value and returns them in order, or the first
/// failure as soon as it happens — Kotlin's `awaitAll`.
pub fn await_all<T>(deferreds: Vec<Deferred<T>>) -> AwaitAll<T> {
    let results = deferreds.iter().map(|_| None).collect();
    AwaitAll { deferreds, results }
}

/// The future returned by [`await_all`].
pub struct AwaitAll<T> {
    deferreds: Vec<Deferred<T>>,
    results: Vec<Option<T>>,
}

impl<T> Unpin for AwaitAll<T> {}

impl<T> Future for AwaitAll<T> {
    type Output = Result<Vec<T>, TaskFailed>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let mut waiting = false;
        for (deferred, result) in this.deferreds.iter_mut().zip(this.results.iter_mut()) {
            if result.is_some() {
                continue;
            }
            match Pin::new(deferred).poll(cx) {
                Poll::Ready(Ok(value)) => *result = Some(value),
                Poll::Ready(Err(failed)) => return Poll::Ready(Err(failed)),
                Poll::Pending => waiting = true,
            }
        }
        if waiting {
            return Poll::Pending;
        }
        Poll::Ready(Ok(std::mem::take(&mut this.results)
            .into_iter()
            .flatten()
            .collect()))
    }
}

/// Runs `future` on `dispatcher` and waits for its result — Kotlin's
/// `withContext`.
///
/// Dropping the returned future cancels the work.
pub fn with_context<T, F>(dispatcher: &Dispatcher, future: F) -> WithContext<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    WithContext {
        deferred: Deferred::spawn(|sender| {
            spawn_send(
                dispatcher,
                async move { sender.send(future.await) },
                Launch::DEFERRED,
            )
        }),
    }
}

/// The future returned by [`with_context`].
pub struct WithContext<T> {
    deferred: Deferred<T>,
}

impl<T> Future for WithContext<T> {
    type Output = Result<T, TaskFailed>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<T, TaskFailed>> {
        Pin::new(&mut self.get_mut().deferred).poll(cx)
    }
}

impl<T> Drop for WithContext<T> {
    fn drop(&mut self) {
        self.deferred.job.cancel();
    }
}

/// A child of a [`coroutine_scope`] panicked, so the scope cancelled its other
/// work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("a child coroutine failed")]
pub struct ChildFailed;

/// Runs `block` with a scope for child coroutines and returns once `block`
/// and every child have finished — Kotlin's `coroutineScope`.
///
/// Children run on the dispatcher of the calling coroutine. If a child
/// panics, `block` and the other children are cancelled and the result is
/// [`ChildFailed`]. Dropping the returned future cancels everything.
pub async fn coroutine_scope<T, F, Fut>(block: F) -> Result<T, ChildFailed>
where
    F: FnOnce(ScopeHandle) -> Fut,
    Fut: Future<Output = T>,
{
    run_scope(block, false).await
}

/// Like [`coroutine_scope`], but a failing child does not cancel `block` or
/// its siblings — Kotlin's `supervisorScope`.
pub async fn supervisor_scope<T, F, Fut>(block: F) -> T
where
    F: FnOnce(ScopeHandle) -> Fut,
    Fut: Future<Output = T>,
{
    let dispatcher = current_dispatcher().unwrap_or_else(Dispatchers::default_pool);
    let scope = Scope::with_policy(dispatcher, true);
    let value = block(scope.handle()).await;
    std::future::poll_fn(|cx| scope.core.poll_idle(cx)).await;
    value
}

async fn run_scope<T, F, Fut>(block: F, supervisor: bool) -> Result<T, ChildFailed>
where
    F: FnOnce(ScopeHandle) -> Fut,
    Fut: Future<Output = T>,
{
    let dispatcher = current_dispatcher().unwrap_or_else(Dispatchers::default_pool);
    let scope = Scope::with_policy(dispatcher, supervisor);
    let failure = std::future::poll_fn(|cx| scope.core.poll_failed(cx));
    let value = match select(block(scope.handle()), failure).await {
        Either::Left(value) => value,
        Either::Right(()) => return Err(ChildFailed),
    };
    std::future::poll_fn(|cx| {
        if scope.core.has_failed() {
            return Poll::Ready(Err(ChildFailed));
        }
        scope.core.poll_idle(cx).map(|()| Ok(()))
    })
    .await?;
    if scope.core.has_failed() {
        Err(ChildFailed)
    } else {
        Ok(value)
    }
}
