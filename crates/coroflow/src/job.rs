use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, Weak},
    task::{Context, Poll, Waker},
};

use crate::sync::lock;

/// How a coroutine finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobOutcome {
    /// The future ran to completion.
    Completed,
    /// The coroutine was cancelled and its future dropped.
    Cancelled,
    /// The future panicked while being polled.
    Panicked,
}

/// A handle to one launched coroutine — Kotlin's `Job`.
///
/// Cancelling drops the coroutine's future on its own dispatcher, which is how
/// Rust cancels async work: there is no cooperative `isActive` check to write.
#[derive(Clone)]
pub struct Job {
    inner: Arc<Mutex<JobState>>,
}

pub(crate) trait JobParent: Send + Sync {
    fn child_finished(&self, outcome: JobOutcome, failure: Option<&str>);
}

/// How a coroutine is launched: whether it waits for `start()` — Kotlin's
/// `CoroutineStart.LAZY` — and whether its panic reaches the scope's failure
/// handler, which `async` coroutines leave to their `Deferred`.
#[derive(Clone, Copy)]
pub(crate) struct Launch {
    pub(crate) lazy: bool,
    pub(crate) reports_failure: bool,
}

impl Launch {
    pub(crate) const EAGER: Self = Self {
        lazy: false,
        reports_failure: true,
    };
    pub(crate) const LAZY: Self = Self {
        lazy: true,
        reports_failure: true,
    };
    pub(crate) const DEFERRED: Self = Self {
        lazy: false,
        reports_failure: false,
    };
    pub(crate) const LAZY_DEFERRED: Self = Self {
        lazy: true,
        reports_failure: false,
    };
}

type CompletionHandler = Box<dyn FnOnce(JobOutcome) + Send>;

struct JobState {
    outcome: Option<JobOutcome>,
    cancel_requested: bool,
    started: bool,
    reports_failure: bool,
    task_waker: Option<Waker>,
    joiners: Vec<Waker>,
    completion: Vec<CompletionHandler>,
    parent: Option<Weak<dyn JobParent>>,
    unreported_failure: Option<String>,
}

impl Job {
    pub(crate) fn new(launch: Launch) -> Self {
        Self {
            inner: Arc::new(Mutex::new(JobState {
                outcome: None,
                cancel_requested: false,
                started: !launch.lazy,
                reports_failure: launch.reports_failure,
                task_waker: None,
                joiners: Vec::new(),
                completion: Vec::new(),
                parent: None,
                unreported_failure: None,
            })),
        }
    }

    pub(crate) fn finished(outcome: JobOutcome) -> Self {
        let job = Self::new(Launch::EAGER);
        job.finish(outcome);
        job
    }

    /// Starts a coroutine launched lazily and reports whether this call
    /// started it — Kotlin's `start()`. Joining or awaiting starts it too.
    pub fn start(&self) -> bool {
        let waker = {
            let mut state = lock(&self.inner);
            if state.started || state.outcome.is_some() {
                return false;
            }
            state.started = true;
            state.task_waker.clone()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
        true
    }

    /// Runs `handler` with the outcome once the coroutine finishes, or at once
    /// if it already has — Kotlin's `invokeOnCompletion`.
    pub fn invoke_on_completion(&self, handler: impl FnOnce(JobOutcome) + Send + 'static) {
        let outcome = {
            let mut state = lock(&self.inner);
            if state.outcome.is_none() {
                state.completion.push(Box::new(handler));
                return;
            }
            state.outcome
        };
        if let Some(outcome) = outcome {
            handler(outcome);
        }
    }

    /// Cancels the coroutine and waits until it has ended — Kotlin's
    /// `cancelAndJoin`.
    pub fn cancel_and_join(&self) -> Join {
        self.cancel();
        self.join()
    }

    /// Requests cancellation. The future is dropped the next time its
    /// dispatcher runs it, and never polled again.
    pub fn cancel(&self) {
        let waker = {
            let mut state = lock(&self.inner);
            if state.outcome.is_some() || state.cancel_requested {
                return;
            }
            state.cancel_requested = true;
            state.task_waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Whether the coroutine is still running and was not asked to stop.
    pub fn is_active(&self) -> bool {
        let state = lock(&self.inner);
        state.outcome.is_none() && !state.cancel_requested
    }

    /// Whether cancellation was requested or the coroutine ended cancelled.
    pub fn is_cancelled(&self) -> bool {
        let state = lock(&self.inner);
        state.cancel_requested || state.outcome == Some(JobOutcome::Cancelled)
    }

    /// How the coroutine finished, or `None` while it is still running.
    pub fn outcome(&self) -> Option<JobOutcome> {
        lock(&self.inner).outcome
    }

    /// Waits for the coroutine to finish and reports how.
    pub fn join(&self) -> Join {
        Join { job: self.clone() }
    }

    pub(crate) fn cancel_requested(&self) -> bool {
        lock(&self.inner).cancel_requested
    }

    pub(crate) fn set_task_waker(&self, waker: Waker) {
        let mut state = lock(&self.inner);
        if state.outcome.is_none() {
            state.task_waker = Some(waker);
        }
    }

    pub(crate) fn set_parent(&self, parent: Weak<dyn JobParent>) {
        let (finished, failure) = {
            let mut state = lock(&self.inner);
            if state.outcome.is_none() {
                state.parent = Some(parent.clone());
            }
            (state.outcome, state.unreported_failure.take())
        };
        if let (Some(outcome), Some(parent)) = (finished, parent.upgrade()) {
            parent.child_finished(outcome, failure.as_deref());
        }
    }

    pub(crate) fn finish(&self, outcome: JobOutcome) {
        self.end(outcome, None);
    }

    pub(crate) fn fail(&self, message: &str) {
        self.end(JobOutcome::Panicked, Some(message));
    }

    fn end(&self, outcome: JobOutcome, failure: Option<&str>) {
        let (joiners, completion, parent, reported) = {
            let mut state = lock(&self.inner);
            if state.outcome.is_some() {
                return;
            }
            state.outcome = Some(outcome);
            state.task_waker = None;
            let failure = failure.filter(|_| state.reports_failure);
            if state.parent.is_none() {
                state.unreported_failure = failure.map(str::to_owned);
            }
            (
                std::mem::take(&mut state.joiners),
                std::mem::take(&mut state.completion),
                state.parent.take(),
                failure,
            )
        };
        for waker in joiners {
            waker.wake();
        }
        for handler in completion {
            handler(outcome);
        }
        if let Some(parent) = parent.and_then(|parent| parent.upgrade()) {
            parent.child_finished(outcome, reported);
        }
    }
}

/// The future returned by [`Job::join`].
pub struct Join {
    job: Job,
}

impl Future for Join {
    type Output = JobOutcome;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<JobOutcome> {
        self.job.start();
        let mut state = lock(&self.job.inner);
        if let Some(outcome) = state.outcome {
            return Poll::Ready(outcome);
        }
        if !state
            .joiners
            .iter()
            .any(|waker| waker.will_wake(cx.waker()))
        {
            state.joiners.push(cx.waker().clone());
        }
        Poll::Pending
    }
}
