use std::{
    cell::RefCell,
    marker::PhantomData,
    rc::Rc,
    sync::{Arc, OnceLock},
    thread::{self, ThreadId},
};
#[cfg(not(target_arch = "wasm32"))]
use std::{
    collections::VecDeque,
    sync::{Condvar, Mutex},
};

use crate::clock::{Clock, SystemClock};
#[cfg(not(target_arch = "wasm32"))]
use crate::sync::lock;

/// Runs scheduled coroutine steps somewhere: a thread pool, a UI event loop or
/// a test queue.
///
/// This is Kotlin's `CoroutineDispatcher.dispatch`. An implementation must not
/// run the [`Runnable`] before `dispatch` returns. It may block, for instance
/// until a UI thread accepts the work, so coroflow never wakes a coroutine
/// while holding one of its own locks.
pub trait Dispatch: Send + Sync + 'static {
    /// Queues `runnable` to run later.
    fn dispatch(&self, runnable: Runnable);
}

pub(crate) trait Schedule: Send + Sync + 'static {
    fn run(self: Arc<Self>);
}

/// One scheduled step of a coroutine, handed to a [`Dispatch`] implementation.
pub struct Runnable {
    task: Arc<dyn Schedule>,
}

impl Runnable {
    pub(crate) fn new(task: Arc<dyn Schedule>) -> Self {
        Self { task }
    }

    /// Runs the step on the current thread.
    pub fn run(self) {
        self.task.run();
    }
}

/// Where coroutines run and which [`Clock`] they measure time with.
///
/// Cheap to clone. Coroutines launched with a `Dispatcher` must be `Send`
/// because it may move them between threads; use a [`ConfinedDispatcher`] and
/// a [`MainScope`](crate::MainScope) for work that holds `Rc` or other
/// thread-bound values.
#[derive(Clone)]
pub struct Dispatcher {
    inner: Arc<DispatcherInner>,
}

struct DispatcherInner {
    executor: Box<dyn Dispatch>,
    clock: Arc<dyn Clock>,
}

impl Dispatcher {
    /// A dispatcher that schedules through `executor` and measures time on `clock`.
    pub fn new(executor: impl Dispatch, clock: Arc<dyn Clock>) -> Self {
        Self {
            inner: Arc::new(DispatcherInner {
                executor: Box::new(executor),
                clock,
            }),
        }
    }

    /// The clock coroutines on this dispatcher measure time with.
    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.inner.clock
    }

    pub(crate) fn dispatch(&self, runnable: Runnable) {
        self.inner.executor.dispatch(runnable);
    }
}

/// A [`Dispatcher`] whose executor runs everything on the thread that created
/// this handle — Kotlin's `Dispatchers.Main`.
///
/// Because every step runs on one thread, a [`MainScope`](crate::MainScope)
/// built on it accepts futures that are not `Send`. The handle itself cannot
/// leave its thread.
#[derive(Clone)]
pub struct ConfinedDispatcher {
    dispatcher: Dispatcher,
    thread: ThreadId,
    _not_send: PhantomData<Rc<()>>,
}

impl ConfinedDispatcher {
    /// Confines `executor` to the calling thread.
    ///
    /// `executor` must run every runnable on this thread; a step delivered to
    /// another thread is dropped with an error log.
    pub fn for_current_thread(executor: impl Dispatch, clock: Arc<dyn Clock>) -> Self {
        Self {
            dispatcher: Dispatcher::new(executor, clock),
            thread: thread::current().id(),
            _not_send: PhantomData,
        }
    }

    /// The underlying dispatcher, usable from any thread with
    /// [`with_context`](crate::with_context) to hop back onto this thread.
    pub fn dispatcher(&self) -> &Dispatcher {
        &self.dispatcher
    }

    pub(crate) fn thread(&self) -> ThreadId {
        self.thread
    }
}

/// The shared dispatchers every application gets — Kotlin's `Dispatchers`.
///
/// There is no `main` here: the UI framework owns the main thread and provides
/// its [`ConfinedDispatcher`]. In the browser there are no threads, so both
/// pools run their coroutines as tasks on the page's event loop; blocking work
/// dispatched to [`io`](Dispatchers::io) there blocks the page.
pub struct Dispatchers;

impl Dispatchers {
    /// A pool sized to the machine's parallelism, for CPU-bound work.
    pub fn default_pool() -> Dispatcher {
        static POOL: OnceLock<Dispatcher> = OnceLock::new();
        POOL.get_or_init(|| pool("coroflow-default", parallelism()))
            .clone()
    }

    /// A larger pool for work that blocks on I/O.
    pub fn io() -> Dispatcher {
        static POOL: OnceLock<Dispatcher> = OnceLock::new();
        POOL.get_or_init(|| pool("coroflow-io", parallelism().max(IO_POOL_MIN_THREADS)))
            .clone()
    }
}

/// The smallest number of threads [`Dispatchers::io`] starts with.
pub const IO_POOL_MIN_THREADS: usize = 16;

fn parallelism() -> usize {
    thread::available_parallelism().map_or(1, |count| count.get())
}

#[cfg(not(target_arch = "wasm32"))]
fn pool(name: &str, threads: usize) -> Dispatcher {
    ThreadPool::start(name, threads)
}

#[cfg(target_arch = "wasm32")]
fn pool(_name: &str, _threads: usize) -> Dispatcher {
    Dispatcher::new(EventLoopExecutor, SystemClock::shared())
}

#[cfg(target_arch = "wasm32")]
struct EventLoopExecutor;

#[cfg(target_arch = "wasm32")]
impl Dispatch for EventLoopExecutor {
    fn dispatch(&self, runnable: Runnable) {
        wasm_bindgen_futures::spawn_local(async move { runnable.run() });
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct ThreadPool {
    queue: Mutex<VecDeque<Runnable>>,
    available: Condvar,
}

#[cfg(not(target_arch = "wasm32"))]
impl ThreadPool {
    fn start(name: &str, threads: usize) -> Dispatcher {
        let pool = Arc::new(ThreadPool {
            queue: Mutex::new(VecDeque::new()),
            available: Condvar::new(),
        });
        for index in 0..threads {
            let worker = Arc::clone(&pool);
            let spawned = thread::Builder::new()
                .name(format!("{name}-{index}"))
                .spawn(move || worker.work());
            if let Err(error) = spawned {
                log::error!("coroflow: worker {name}-{index} could not start: {error}");
            }
        }
        Dispatcher::new(PoolExecutor { pool }, SystemClock::shared())
    }

    fn work(&self) {
        loop {
            let runnable = {
                let mut queue = lock(&self.queue);
                loop {
                    if let Some(runnable) = queue.pop_front() {
                        break runnable;
                    }
                    queue = match self.available.wait(queue) {
                        Ok(queue) => queue,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                }
            };
            runnable.run();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct PoolExecutor {
    pool: Arc<ThreadPool>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Dispatch for PoolExecutor {
    fn dispatch(&self, runnable: Runnable) {
        lock(&self.pool.queue).push_back(runnable);
        self.pool.available.notify_one();
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Dispatcher>> = const { RefCell::new(None) };
}

pub(crate) struct EnterGuard {
    previous: Option<Dispatcher>,
}

pub(crate) fn enter(dispatcher: &Dispatcher) -> EnterGuard {
    let previous = CURRENT.with(|current| current.replace(Some(dispatcher.clone())));
    EnterGuard { previous }
}

impl Drop for EnterGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CURRENT.with(|current| {
            current.replace(previous);
        });
    }
}

pub(crate) fn current_dispatcher() -> Option<Dispatcher> {
    CURRENT.with(|current| current.borrow().clone())
}
