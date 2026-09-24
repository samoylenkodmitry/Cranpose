use std::{
    collections::VecDeque,
    future::Future,
    pin::{Pin, pin},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};

use futures_core::Stream;

use crate::{
    clock::{Clock, TimerHeap},
    dispatcher::{ConfinedDispatcher, Dispatch, Dispatcher, Runnable, enter},
    flow::Flow,
    sync::lock,
};

/// A deterministic, single-threaded scheduler with virtual time — Kotlin's
/// `StandardTestDispatcher` plus `runTest`.
///
/// Every dispatcher it hands out queues onto the same thread, and every
/// [`delay`](crate::delay) or [`debounce`](crate::FlowExt::debounce) waits on its
/// virtual clock, so a test controls exactly when work runs and time passes.
pub struct TestScheduler {
    queue: Arc<Mutex<VecDeque<Runnable>>>,
    clock: Arc<TestClock>,
    dispatcher: Dispatcher,
}

/// A [`TestScheduler`] ran out of work before the future it drives finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("no task or timer can make progress")]
pub struct Stalled;

#[derive(Default)]
struct TestClock {
    state: Mutex<TestClockState>,
}

#[derive(Default)]
struct TestClockState {
    now: Duration,
    timers: TimerHeap,
}

impl Clock for TestClock {
    fn now(&self) -> Duration {
        lock(&self.state).now
    }

    fn wake_at(&self, deadline: Duration, waker: &Waker) {
        lock(&self.state).timers.push(deadline, waker);
    }
}

struct TestExecutor {
    queue: Arc<Mutex<VecDeque<Runnable>>>,
}

impl Dispatch for TestExecutor {
    fn dispatch(&self, runnable: Runnable) {
        lock(&self.queue).push_back(runnable);
    }
}

struct Flag(AtomicBool);

impl Wake for Flag {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::Release);
    }
}

impl Default for TestScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl TestScheduler {
    /// A scheduler at virtual time zero with nothing queued.
    pub fn new() -> Self {
        let queue: Arc<Mutex<VecDeque<Runnable>>> = Arc::default();
        let clock: Arc<TestClock> = Arc::default();
        let dispatcher = Dispatcher::new(
            TestExecutor {
                queue: Arc::clone(&queue),
            },
            Arc::clone(&clock) as Arc<dyn Clock>,
        );
        Self {
            queue,
            clock,
            dispatcher,
        }
    }

    /// A dispatcher for work that would run on a background pool.
    pub fn dispatcher(&self) -> Dispatcher {
        self.dispatcher.clone()
    }

    /// A main-thread dispatcher confined to the calling thread.
    pub fn main_dispatcher(&self) -> ConfinedDispatcher {
        ConfinedDispatcher::for_current_thread(
            self.executor(),
            Arc::clone(&self.clock) as Arc<dyn Clock>,
        )
    }

    /// The current virtual time.
    pub fn now(&self) -> Duration {
        self.clock.now()
    }

    /// Runs queued work, including work it queues, without advancing time.
    pub fn run_current(&self) {
        loop {
            let next = lock(&self.queue).pop_front();
            match next {
                Some(runnable) => runnable.run(),
                None => return,
            }
        }
    }

    /// Advances virtual time by `duration`, firing each timer at its deadline
    /// and running the work it releases.
    pub fn advance_time_by(&self, duration: Duration) {
        let target = self.now() + duration;
        self.run_current();
        while self.fire_next_timer(Some(target)) {
            self.run_current();
        }
        lock(&self.clock.state).now = target;
        self.run_current();
    }

    /// Drives `future` to completion, advancing virtual time whenever nothing
    /// else can run — Kotlin's `runTest`.
    ///
    /// `future` itself runs as if on [`dispatcher`](TestScheduler::dispatcher),
    /// so its delays use virtual time too.
    pub fn block_on<T>(&self, future: impl Future<Output = T>) -> Result<T, Stalled> {
        let mut future = pin!(future);
        let flag = Arc::new(Flag(AtomicBool::new(true)));
        let waker = Waker::from(Arc::clone(&flag));
        let mut cx = Context::from_waker(&waker);
        loop {
            if flag.0.swap(false, Ordering::AcqRel) {
                let _entered = enter(&self.dispatcher);
                if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
                    return Ok(value);
                }
            }
            self.run_current();
            if flag.0.load(Ordering::Acquire) {
                continue;
            }
            if !self.fire_next_timer(None) {
                return Err(Stalled);
            }
        }
    }

    fn fire_next_timer(&self, limit: Option<Duration>) -> bool {
        let wakers = {
            let mut state = lock(&self.clock.state);
            let Some(deadline) = state.timers.next_deadline() else {
                return false;
            };
            if limit.is_some_and(|limit| deadline > limit) {
                return false;
            }
            state.now = state.now.max(deadline);
            let now = state.now;
            let mut due = Vec::new();
            while let Some(waker) = state.timers.pop_due(now) {
                due.push(waker);
            }
            due
        };
        for waker in wakers {
            waker.wake();
        }
        true
    }

    /// Starts collecting `flow` as a coroutine on this scheduler would, so
    /// work the flow starts, such as a [`channel_flow`](crate::channel_flow)
    /// producer, runs on virtual time.
    pub fn turbine<F: Flow>(&self, flow: &F) -> Turbine<F::Run> {
        Turbine {
            run: flow.open(),
            context: Some(self.dispatcher.clone()),
        }
    }

    fn executor(&self) -> TestExecutor {
        TestExecutor {
            queue: Arc::clone(&self.queue),
        }
    }
}

/// Steps through a flow by hand in a test — Kotlin's Turbine.
///
/// Holding a `Turbine` counts as a collector, so it keeps `WhileSubscribed`
/// upstreams running, and dropping it cancels the collection.
pub struct Turbine<S> {
    run: S,
    context: Option<Dispatcher>,
}

impl<S: Stream + Unpin> Turbine<S> {
    /// Starts collecting `flow` outside any dispatcher.
    pub fn of<F: Flow<Run = S>>(flow: &F) -> Self {
        Self {
            run: flow.open(),
            context: None,
        }
    }

    /// The next value if one is ready right now: `Ready(Some(value))`,
    /// `Ready(None)` once the flow completed, or `Pending`.
    pub fn next_now(&mut self) -> Poll<Option<S::Item>> {
        let _entered = self.context.as_ref().map(enter);
        let mut cx = Context::from_waker(Waker::noop());
        Pin::new(&mut self.run).poll_next(&mut cx)
    }
}
