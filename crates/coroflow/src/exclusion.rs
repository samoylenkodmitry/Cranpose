use std::{
    collections::VecDeque,
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

use crate::sync::lock;

const HELD: &str = "a MutexGuard holds the value until it drops";

struct Waiter {
    ticket: u64,
    waker: Waker,
    granted: bool,
}

#[derive(Default)]
struct PermitState {
    available: usize,
    waiters: VecDeque<Waiter>,
    granted: usize,
    next_ticket: u64,
}

impl PermitState {
    fn release(&mut self) -> Option<Waker> {
        let Some(waiter) = self.waiters.get_mut(self.granted) else {
            self.available += 1;
            return None;
        };
        waiter.granted = true;
        self.granted += 1;
        Some(waiter.waker.clone())
    }

    fn position(&self, ticket: u64) -> Option<usize> {
        self.waiters
            .binary_search_by_key(&ticket, |waiter| waiter.ticket)
            .ok()
    }

    fn enqueue(&mut self, waker: &Waker) -> u64 {
        let ticket = self.next_ticket;
        self.next_ticket += 1;
        self.waiters.push_back(Waiter {
            ticket,
            waker: waker.clone(),
            granted: false,
        });
        ticket
    }

    fn poll_ticket(&mut self, ticket: u64, waker: &Waker) -> Option<Poll<()>> {
        let index = self.position(ticket)?;
        let waiter = self.waiters.get_mut(index)?;
        if !waiter.granted {
            waiter.waker.clone_from(waker);
            return Some(Poll::Pending);
        }
        self.waiters.remove(index);
        self.granted -= 1;
        Some(Poll::Ready(()))
    }

    fn leave(&mut self, ticket: u64) -> Option<Waker> {
        let waiter = self.waiters.remove(self.position(ticket)?)?;
        if !waiter.granted {
            return None;
        }
        self.granted -= 1;
        self.release()
    }
}

struct Permits {
    state: std::sync::Mutex<PermitState>,
}

impl Permits {
    fn new(count: usize) -> Self {
        Self {
            state: std::sync::Mutex::new(PermitState {
                available: count,
                ..PermitState::default()
            }),
        }
    }

    fn acquire(&self) -> WaitForPermit<'_> {
        WaitForPermit {
            permits: self,
            ticket: None,
        }
    }

    fn try_acquire(&self) -> bool {
        let mut state = lock(&self.state);
        let free = state.available > 0;
        if free {
            state.available -= 1;
        }
        free
    }

    fn available(&self) -> usize {
        lock(&self.state).available
    }

    fn release(&self) {
        let waker = lock(&self.state).release();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

struct WaitForPermit<'a> {
    permits: &'a Permits,
    ticket: Option<u64>,
}

impl Future for WaitForPermit<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        let mut state = lock(&this.permits.state);
        if let Some(poll) = this
            .ticket
            .and_then(|ticket| state.poll_ticket(ticket, cx.waker()))
        {
            if poll.is_ready() {
                this.ticket = None;
            }
            return poll;
        }
        if state.available > 0 {
            state.available -= 1;
            this.ticket = None;
            return Poll::Ready(());
        }
        this.ticket = Some(state.enqueue(cx.waker()));
        Poll::Pending
    }
}

impl Drop for WaitForPermit<'_> {
    fn drop(&mut self) {
        let Some(ticket) = self.ticket else {
            return;
        };
        let waker = lock(&self.permits.state).leave(ticket);
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

/// Limits how many coroutines run a section at once — Kotlin's `Semaphore`.
///
/// Waiting coroutines get permits in the order they asked for them, and a
/// cancelled waiter hands a permit it was just given to the next one. Clones
/// share the same permits.
#[derive(Clone)]
pub struct Semaphore {
    permits: Arc<Permits>,
}

impl Semaphore {
    /// A semaphore with `permits` free permits.
    pub fn new(permits: usize) -> Self {
        Self {
            permits: Arc::new(Permits::new(permits)),
        }
    }

    /// Waits for a permit — Kotlin's `acquire`. The permit is returned when
    /// the [`Permit`] drops, so holding it for a block is Kotlin's
    /// `withPermit`.
    pub async fn acquire(&self) -> Permit {
        self.permits.acquire().await;
        Permit {
            permits: Arc::clone(&self.permits),
        }
    }

    /// Takes a permit only if one is free now — Kotlin's `tryAcquire`.
    pub fn try_acquire(&self) -> Option<Permit> {
        self.permits.try_acquire().then(|| Permit {
            permits: Arc::clone(&self.permits),
        })
    }

    /// How many permits are free — Kotlin's `availablePermits`.
    pub fn available_permits(&self) -> usize {
        self.permits.available()
    }
}

/// A permit taken from a [`Semaphore`]; dropping it gives the permit back.
#[must_use = "the permit is given back as soon as it is dropped"]
pub struct Permit {
    permits: Arc<Permits>,
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.permits.release();
    }
}

struct MutexInner<T> {
    permits: Permits,
    value: std::sync::Mutex<Option<T>>,
}

/// Mutual exclusion a coroutine may hold across suspension points — Kotlin's
/// `Mutex`, guarding the value it owns the way `std::sync::Mutex` does.
///
/// Coroutines get the lock in the order they asked for it. The guard is
/// `Send` whenever the value is, so a background coroutine may hold it across
/// an `await`. Clones share the same lock and value.
pub struct Mutex<T> {
    inner: Arc<MutexInner<T>>,
}

impl<T> Clone for Mutex<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> Mutex<T> {
    /// A lock guarding `value`.
    pub fn new(value: T) -> Self {
        Self {
            inner: Arc::new(MutexInner {
                permits: Permits::new(1),
                value: std::sync::Mutex::new(Some(value)),
            }),
        }
    }

    /// Waits for the lock — Kotlin's `lock`. Dropping the guard unlocks, so
    /// holding it for a block is Kotlin's `withLock`.
    pub async fn lock(&self) -> MutexGuard<T> {
        self.inner.permits.acquire().await;
        self.guard()
    }

    /// Takes the lock only if it is free now — Kotlin's `tryLock`.
    pub fn try_lock(&self) -> Option<MutexGuard<T>> {
        self.inner.permits.try_acquire().then(|| self.guard())
    }

    /// Whether a coroutine holds the lock or is being handed it — Kotlin's
    /// `isLocked`.
    pub fn is_locked(&self) -> bool {
        self.inner.permits.available() == 0
    }

    fn guard(&self) -> MutexGuard<T> {
        MutexGuard {
            value: lock(&self.inner.value).take(),
            inner: Arc::clone(&self.inner),
        }
    }
}

/// Exclusive access to the value of a [`Mutex`]; dropping it unlocks.
#[must_use = "the lock is released as soon as the guard is dropped"]
pub struct MutexGuard<T> {
    inner: Arc<MutexInner<T>>,
    value: Option<T>,
}

impl<T> Deref for MutexGuard<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.value.as_ref().expect(HELD)
    }
}

impl<T> DerefMut for MutexGuard<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.value.as_mut().expect(HELD)
    }
}

impl<T> Drop for MutexGuard<T> {
    fn drop(&mut self) {
        *lock(&self.inner.value) = self.value.take();
        self.inner.permits.release();
    }
}
