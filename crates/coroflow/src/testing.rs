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
    scope::{CoroutineScope, join_all},
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

    /// Runs queued work and fires every timer in deadline order until nothing
    /// is left — Kotlin's `advanceUntilIdle`. Like Kotlin's, it never returns
    /// while a coroutine keeps scheduling timers, such as a ticker.
    pub fn advance_until_idle(&self) {
        self.run_current();
        while self.fire_next_timer(None) {
            self.run_current();
        }
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
    ///
    /// Its awaiting assertions, such as [`Turbine::await_item`], run this
    /// scheduler and advance virtual time until the flow answers.
    pub fn turbine<F: Flow>(&self, flow: &F) -> Turbine<'_, F::Run> {
        let _entered = enter(&self.dispatcher);
        Turbine {
            run: Some(flow.open()),
            driver: Some(self),
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
/// upstreams running, and dropping it cancels the collection. One made by
/// [`TestScheduler::turbine`] drives that scheduler while it waits; one made by
/// [`Turbine::of`] only looks at what is ready. The awaiting assertions panic
/// with a message when the flow does something else, as Turbine's do.
pub struct Turbine<'a, S> {
    run: Option<S>,
    driver: Option<&'a TestScheduler>,
}

impl<S: Stream + Unpin> Turbine<'static, S> {
    /// Starts collecting `flow` outside any dispatcher.
    pub fn of<F: Flow<Run = S>>(flow: &F) -> Self {
        Self {
            run: Some(flow.open()),
            driver: None,
        }
    }
}

impl<S: Stream + Unpin> Turbine<'_, S> {
    /// The next value if one is ready right now: `Ready(Some(value))`,
    /// `Ready(None)` once the flow completed, or `Pending`.
    pub fn next_now(&mut self) -> Poll<Option<S::Item>> {
        let Some(run) = self.run.as_mut() else {
            return Poll::Ready(None);
        };
        let _entered = self.driver.map(|driver| enter(&driver.dispatcher));
        let mut cx = Context::from_waker(Waker::noop());
        Pin::new(run).poll_next(&mut cx)
    }

    fn next_event(&mut self) -> Poll<Option<S::Item>> {
        loop {
            if let Poll::Ready(event) = self.next_now() {
                return Poll::Ready(event);
            }
            let Some(driver) = self.driver else {
                return Poll::Pending;
            };
            driver.run_current();
            if let Poll::Ready(event) = self.next_now() {
                return Poll::Ready(event);
            }
            if !driver.fire_next_timer(None) {
                return Poll::Pending;
            }
        }
    }

    /// Waits for the next value and returns it — Turbine's `awaitItem`.
    ///
    /// # Panics
    ///
    /// When the flow completes or can make no more progress first.
    pub fn await_item(&mut self) -> S::Item {
        match self.next_event() {
            Poll::Ready(Some(value)) => value,
            Poll::Ready(None) => panic!("expected an item, but the flow completed"),
            Poll::Pending => panic!("expected an item, but none arrived"),
        }
    }

    /// Waits for the flow to complete — Turbine's `awaitComplete`.
    ///
    /// # Panics
    ///
    /// When the flow emits or can make no more progress first.
    pub fn await_complete(&mut self) {
        match self.next_event() {
            Poll::Ready(None) => self.run = None,
            Poll::Ready(Some(_)) => panic!("expected the flow to complete, but it emitted"),
            Poll::Pending => panic!("expected the flow to complete, but it is still running"),
        }
    }

    /// Checks that nothing is ready without advancing time — Turbine's
    /// `expectNoEvents`.
    ///
    /// # Panics
    ///
    /// When the flow has a value ready or has completed.
    pub fn expect_no_events(&mut self) {
        if let Some(driver) = self.driver {
            driver.run_current();
        }
        match self.next_now() {
            Poll::Pending => {}
            Poll::Ready(Some(_)) => panic!("expected no events, but the flow emitted"),
            Poll::Ready(None) => panic!("expected no events, but the flow completed"),
        }
    }

    /// Waits for and discards `count` values — Turbine's `skipItems`.
    ///
    /// # Panics
    ///
    /// When fewer than `count` values arrive.
    pub fn skip_items(&mut self, count: usize) {
        for _ in 0..count {
            self.await_item();
        }
    }

    /// Stops collecting and ignores whatever the flow would still do —
    /// Turbine's `cancelAndIgnoreRemainingEvents`.
    pub fn cancel_and_ignore_remaining_events(self) {}
}

/// What [`run_test`] hands its body — Kotlin's `TestScope`.
///
/// It dereferences to a [`CoroutineScope`] on the test's virtual time, whose
/// coroutines the test waits for before it ends.
pub struct TestScope {
    scheduler: TestScheduler,
    scope: CoroutineScope,
    background: CoroutineScope,
}

impl TestScope {
    /// The scheduler whose virtual time the test runs on — Kotlin's
    /// `testScheduler`.
    pub fn scheduler(&self) -> &TestScheduler {
        &self.scheduler
    }

    /// A scope for coroutines that run for the whole test and are cancelled
    /// when it ends — Kotlin's `backgroundScope`.
    pub fn background_scope(&self) -> &CoroutineScope {
        &self.background
    }
}

impl std::ops::Deref for TestScope {
    type Target = CoroutineScope;

    fn deref(&self) -> &CoroutineScope {
        &self.scope
    }
}

/// Runs a test body on virtual time and returns its result — Kotlin's
/// `runTest`.
///
/// Delays in `body` skip ahead instantly. Once `body` returns, the test waits
/// for the coroutines launched in its [`TestScope`], then cancels its
/// [`background_scope`](TestScope::background_scope). It is [`Stalled`] if
/// the body or those coroutines can never finish.
///
/// ```
/// use std::time::Duration;
///
/// use coroflow::{delay, run_test};
/// let finished_at = run_test(async |test| {
///     test.launch(async { delay(Duration::from_secs(5)).await });
///     delay(Duration::from_secs(1)).await;
///     test.scheduler().now()
/// });
/// assert_eq!(finished_at, Ok(Duration::from_secs(1)));
/// ```
pub fn run_test<T>(body: impl AsyncFnOnce(&TestScope) -> T) -> Result<T, Stalled> {
    let scheduler = TestScheduler::new();
    let test = TestScope {
        scope: CoroutineScope::new(scheduler.dispatcher()),
        background: CoroutineScope::new(scheduler.dispatcher()),
        scheduler,
    };
    let result = test.scheduler.block_on(async {
        let value = body(&test).await;
        join_all(test.scope.children()).await;
        value
    });
    test.background.cancel();
    test.scheduler.run_current();
    result
}
