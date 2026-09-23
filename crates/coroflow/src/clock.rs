#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Condvar, Mutex};
use std::{
    cmp::{Ordering, Reverse},
    collections::BinaryHeap,
    future::Future,
    pin::Pin,
    sync::{Arc, OnceLock},
    task::{Context, Poll, Waker},
    time::Duration,
};

use web_time::Instant;

use crate::dispatcher::current_dispatcher;
#[cfg(not(target_arch = "wasm32"))]
use crate::sync::lock;

/// A monotonic time source that can wake a task at a deadline.
///
/// Every [`Dispatcher`](crate::Dispatcher) carries one. Real dispatchers use
/// [`SystemClock`]; a [`TestScheduler`](crate::TestScheduler) supplies a virtual
/// clock, so [`delay`] and time-based operators such as
/// [`debounce`](crate::FlowExt::debounce) run on virtual time in tests.
pub trait Clock: Send + Sync + 'static {
    /// The time elapsed since this clock started.
    fn now(&self) -> Duration;

    /// Wakes `waker` once [`now`](Clock::now) reaches `deadline`.
    fn wake_at(&self, deadline: Duration, waker: &Waker);
}

pub(crate) struct TimerEntry {
    deadline: Duration,
    sequence: u64,
    waker: Waker,
}

impl PartialEq for TimerEntry {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for TimerEntry {}

impl PartialOrd for TimerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TimerEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.deadline, self.sequence).cmp(&(other.deadline, other.sequence))
    }
}

#[derive(Default)]
pub(crate) struct TimerHeap {
    entries: BinaryHeap<Reverse<TimerEntry>>,
    next_sequence: u64,
}

impl TimerHeap {
    pub(crate) fn push(&mut self, deadline: Duration, waker: &Waker) {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.entries.push(Reverse(TimerEntry {
            deadline,
            sequence,
            waker: waker.clone(),
        }));
    }

    pub(crate) fn next_deadline(&self) -> Option<Duration> {
        self.entries.peek().map(|Reverse(entry)| entry.deadline)
    }

    pub(crate) fn pop_due(&mut self, now: Duration) -> Option<Waker> {
        if self.next_deadline()? > now {
            return None;
        }
        self.entries.pop().map(|Reverse(entry)| entry.waker)
    }
}

/// The wall-clock [`Clock`] used by [`Dispatchers`](crate::Dispatchers).
///
/// On native targets one background thread serves every deadline in the
/// process; in the browser each deadline is a `setTimeout`.
pub struct SystemClock {
    started: Instant,
    #[cfg(not(target_arch = "wasm32"))]
    timers: Arc<(Mutex<TimerHeap>, Condvar)>,
}

impl SystemClock {
    /// The process-wide system clock.
    pub fn shared() -> Arc<dyn Clock> {
        static SHARED: OnceLock<Arc<SystemClock>> = OnceLock::new();
        let clock = SHARED.get_or_init(|| Arc::new(SystemClock::start()));
        Arc::clone(clock) as Arc<dyn Clock>
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start() -> Self {
        let clock = SystemClock {
            started: Instant::now(),
            timers: Arc::new((Mutex::new(TimerHeap::default()), Condvar::new())),
        };
        let timers = Arc::clone(&clock.timers);
        let started = clock.started;
        let spawned = std::thread::Builder::new()
            .name("coroflow-timer".into())
            .spawn(move || run_timer_thread(&timers, started));
        if let Err(error) = spawned {
            log::error!("coroflow: the timer thread could not start: {error}");
        }
        clock
    }

    #[cfg(target_arch = "wasm32")]
    fn start() -> Self {
        SystemClock {
            started: Instant::now(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn run_timer_thread(timers: &(Mutex<TimerHeap>, Condvar), started: Instant) {
    let (heap, changed) = timers;
    let mut due = Vec::new();
    let mut guard = lock(heap);
    loop {
        let now = started.elapsed();
        while let Some(waker) = guard.pop_due(now) {
            due.push(waker);
        }
        if !due.is_empty() {
            drop(guard);
            for waker in due.drain(..) {
                waker.wake();
            }
            guard = lock(heap);
            continue;
        }
        guard = match guard.next_deadline() {
            Some(deadline) => {
                let wait = deadline.saturating_sub(now);
                match changed.wait_timeout(guard, wait) {
                    Ok((next, _)) => next,
                    Err(poisoned) => poisoned.into_inner().0,
                }
            }
            None => match changed.wait(guard) {
                Ok(next) => next,
                Err(poisoned) => poisoned.into_inner(),
            },
        };
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.started.elapsed()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn wake_at(&self, deadline: Duration, waker: &Waker) {
        let (heap, changed) = &*self.timers;
        lock(heap).push(deadline, waker);
        changed.notify_one();
    }

    #[cfg(target_arch = "wasm32")]
    fn wake_at(&self, deadline: Duration, waker: &Waker) {
        use wasm_bindgen::JsCast;
        let millis = deadline
            .saturating_sub(self.now())
            .as_millis()
            .min(i32::MAX as u128) as i32;
        let waker = waker.clone();
        let callback = wasm_bindgen::closure::Closure::once_into_js(move || waker.wake());
        let scheduled = web_sys::window().and_then(|window| {
            window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    callback.unchecked_ref(),
                    millis,
                )
                .ok()
        });
        if scheduled.is_none() {
            log::error!("coroflow: no browser timer is available; a delay will never end");
        }
    }
}

pub(crate) fn current_clock() -> Arc<dyn Clock> {
    current_dispatcher()
        .map(|dispatcher| Arc::clone(dispatcher.clock()))
        .unwrap_or_else(SystemClock::shared)
}

#[derive(Default)]
pub(crate) struct Timer {
    clock: Option<Arc<dyn Clock>>,
    deadline: Option<Duration>,
    registered: Option<(Duration, Waker)>,
}

impl Timer {
    pub(crate) fn start(&mut self, after: Duration) {
        let clock = self.clock.get_or_insert_with(current_clock);
        self.deadline = Some(clock.now() + after);
    }

    pub(crate) fn cancel(&mut self) {
        self.deadline = None;
        self.registered = None;
    }

    pub(crate) fn is_armed(&self) -> bool {
        self.deadline.is_some()
    }

    pub(crate) fn poll_elapsed(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let (Some(clock), Some(deadline)) = (&self.clock, self.deadline) else {
            return Poll::Pending;
        };
        if clock.now() >= deadline {
            self.cancel();
            return Poll::Ready(());
        }
        let already = matches!(
            &self.registered,
            Some((at, waker)) if *at == deadline && waker.will_wake(cx.waker())
        );
        if !already {
            clock.wake_at(deadline, cx.waker());
            self.registered = Some((deadline, cx.waker().clone()));
        }
        Poll::Pending
    }
}

/// Suspends the calling coroutine for `duration` — Kotlin's `delay`.
///
/// The deadline is measured on the clock of the dispatcher running the
/// coroutine, so the same code runs on virtual time under a
/// [`TestScheduler`](crate::TestScheduler).
pub fn delay(duration: Duration) -> Delay {
    Delay {
        duration,
        started: false,
        timer: Timer::default(),
    }
}

/// The future returned by [`delay`].
pub struct Delay {
    duration: Duration,
    started: bool,
    timer: Timer,
}

impl Future for Delay {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        if !this.started {
            this.started = true;
            this.timer.start(this.duration);
        }
        this.timer.poll_elapsed(cx)
    }
}

/// The work given to [`with_timeout`] did not finish in time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the work did not finish in time")]
pub struct TimedOut;

/// Runs `future`, giving up after `timeout` — Kotlin's `withTimeoutOrNull`.
///
/// The deadline is measured on the clock of the dispatcher running the
/// caller; when it passes, `future` is dropped, which cancels it.
pub fn with_timeout<F: Future>(timeout: Duration, future: F) -> WithTimeout<F> {
    WithTimeout {
        future: Box::pin(future),
        timeout,
        started: false,
        timer: Timer::default(),
    }
}

/// The future returned by [`with_timeout`].
pub struct WithTimeout<F> {
    future: Pin<Box<F>>,
    timeout: Duration,
    started: bool,
    timer: Timer,
}

impl<F: Future> Future for WithTimeout<F> {
    type Output = Result<F::Output, TimedOut>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if !this.started {
            this.started = true;
            this.timer.start(this.timeout);
        }
        if let Poll::Ready(value) = this.future.as_mut().poll(cx) {
            return Poll::Ready(Ok(value));
        }
        this.timer.poll_elapsed(cx).map(|()| Err(TimedOut))
    }
}
