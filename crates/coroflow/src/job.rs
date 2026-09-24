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
    fn child_finished(&self, outcome: JobOutcome);
}

#[derive(Default)]
struct JobState {
    outcome: Option<JobOutcome>,
    cancel_requested: bool,
    task_waker: Option<Waker>,
    joiners: Vec<Waker>,
    parent: Option<Weak<dyn JobParent>>,
}

impl Job {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(JobState::default())),
        }
    }

    pub(crate) fn finished(outcome: JobOutcome) -> Self {
        let job = Self::new();
        job.finish(outcome);
        job
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
        let finished = {
            let mut state = lock(&self.inner);
            if state.outcome.is_none() {
                state.parent = Some(parent.clone());
            }
            state.outcome
        };
        if let (Some(outcome), Some(parent)) = (finished, parent.upgrade()) {
            parent.child_finished(outcome);
        }
    }

    pub(crate) fn finish(&self, outcome: JobOutcome) {
        let (joiners, parent) = {
            let mut state = lock(&self.inner);
            if state.outcome.is_some() {
                return;
            }
            state.outcome = Some(outcome);
            state.task_waker = None;
            (std::mem::take(&mut state.joiners), state.parent.take())
        };
        for waker in joiners {
            waker.wake();
        }
        if let Some(parent) = parent.and_then(|parent| parent.upgrade()) {
            parent.child_finished(outcome);
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
