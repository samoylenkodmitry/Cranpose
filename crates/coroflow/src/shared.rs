use std::{
    collections::VecDeque,
    future::Future,
    ops::Deref,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

use futures_core::Stream;

use crate::{
    flow::Flow,
    state::SubscriberCount,
    sync::{WakerSet, lock, wake_and_empty},
};

/// What a [`MutableSharedFlow`] does when an emission finds collectors too far
/// behind — Kotlin's `BufferOverflow`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferOverflow {
    /// `emit` waits until the slowest collector catches up; `try_emit` fails.
    Suspend,
    /// The oldest buffered value is dropped; slow collectors skip it.
    DropOldest,
    /// The new value is dropped.
    DropLatest,
}

struct SharedState<T> {
    buffer: VecDeque<T>,
    first_sequence: u64,
    cursors: Vec<Option<u64>>,
    wakers: WakerSet,
    emitters: Vec<Waker>,
}

impl<T> SharedState<T> {
    fn end(&self) -> u64 {
        self.first_sequence + self.buffer.len() as u64
    }

    fn slowest(&self) -> Option<u64> {
        self.cursors.iter().flatten().copied().min()
    }

    fn unconsumed(&self) -> u64 {
        self.slowest().map_or(0, |slowest| self.end() - slowest)
    }

    fn trim(&mut self, replay: usize) {
        let keep_replay = self.end().saturating_sub(replay as u64);
        let floor = self.slowest().unwrap_or(keep_replay).min(keep_replay);
        while self.first_sequence < floor && self.buffer.pop_front().is_some() {
            self.first_sequence += 1;
        }
    }

    fn drop_oldest(&mut self) {
        if self.buffer.pop_front().is_some() {
            self.first_sequence += 1;
        }
    }
}

struct SharedInner<T> {
    state: Mutex<SharedState<T>>,
    subscribers: SubscriberCount,
    replay: usize,
    capacity: u64,
    overflow: BufferOverflow,
}

enum Offer {
    Delivered(Vec<Waker>),
    Rejected,
    Queued(u64, Vec<Waker>),
}

impl<T> SharedInner<T> {
    fn offer(&self, value: T, may_wait: bool) -> Offer {
        let mut state = lock(&self.state);
        state.trim(self.replay);
        let crowded = state.slowest().is_some() && state.unconsumed() >= self.capacity;
        if crowded {
            match self.overflow {
                BufferOverflow::DropLatest => return Offer::Rejected,
                BufferOverflow::Suspend if !may_wait => return Offer::Rejected,
                BufferOverflow::DropOldest => state.drop_oldest(),
                BufferOverflow::Suspend => {}
            }
        }
        let sequence = state.end();
        state.buffer.push_back(value);
        state.trim(self.replay);
        let wakers = state.wakers.take_wakers();
        if crowded && self.overflow == BufferOverflow::Suspend {
            Offer::Queued(sequence, wakers)
        } else {
            Offer::Delivered(wakers)
        }
    }

    fn wake_collectors(&self, wakers: Vec<Waker>) {
        let emptied = wake_and_empty(wakers);
        lock(&self.state).wakers.recycle(emptied);
    }

    fn poll_room(&self, sequence: u64, cx: &mut Context<'_>) -> Poll<()> {
        let mut state = lock(&self.state);
        let caught_up = state
            .slowest()
            .is_none_or(|slowest| slowest + self.capacity > sequence);
        if caught_up {
            return Poll::Ready(());
        }
        if !state
            .emitters
            .iter()
            .any(|waker| waker.will_wake(cx.waker()))
        {
            state.emitters.push(cx.waker().clone());
        }
        Poll::Pending
    }
}

/// A hot broadcast of values to every current collector — Kotlin's
/// `MutableSharedFlow`.
///
/// It keeps the last `replay` values for new collectors and lets the slowest
/// collector fall up to `replay + extra_capacity` values behind; what happens
/// beyond that is its [`BufferOverflow`]. With no collectors, emitting never
/// waits and only the replay values are kept.
pub struct MutableSharedFlow<T> {
    flow: SharedFlow<T>,
}

impl<T> Clone for MutableSharedFlow<T> {
    fn clone(&self) -> Self {
        Self {
            flow: self.flow.clone(),
        }
    }
}

impl<T> Deref for MutableSharedFlow<T> {
    type Target = SharedFlow<T>;

    fn deref(&self) -> &SharedFlow<T> {
        &self.flow
    }
}

impl<T: Clone> MutableSharedFlow<T> {
    /// A shared flow whose `emit` waits for slow collectors — Kotlin's
    /// `MutableSharedFlow(replay, extraBufferCapacity)`.
    pub fn new(replay: usize, extra_capacity: usize) -> Self {
        Self::with_overflow(replay, extra_capacity, BufferOverflow::Suspend)
    }

    /// A shared flow with an explicit overflow policy.
    pub fn with_overflow(replay: usize, extra_capacity: usize, overflow: BufferOverflow) -> Self {
        let capacity = (replay + extra_capacity) as u64;
        let capacity = if overflow == BufferOverflow::Suspend {
            capacity
        } else {
            capacity.max(1)
        };
        let inner = Arc::new(SharedInner {
            state: Mutex::new(SharedState {
                buffer: VecDeque::with_capacity(replay + extra_capacity),
                first_sequence: 0,
                cursors: Vec::new(),
                wakers: WakerSet::default(),
                emitters: Vec::new(),
            }),
            subscribers: SubscriberCount::default(),
            replay,
            capacity,
            overflow,
        });
        Self {
            flow: SharedFlow { inner },
        }
    }

    /// Emits `value`, waiting while the slowest collector is too far behind
    /// under [`BufferOverflow::Suspend`] — Kotlin's `emit`. Collectors see the
    /// value from the first poll on, even if the wait is then abandoned.
    pub fn emit(&self, value: T) -> EmitShared<'_, T> {
        EmitShared {
            flow: self,
            value: Some(value),
            queued: None,
        }
    }

    /// Emits `value` if that needs no waiting and reports whether it did —
    /// Kotlin's `tryEmit`. A value dropped by
    /// [`BufferOverflow::DropLatest`] counts as not emitted.
    pub fn try_emit(&self, value: T) -> bool {
        match self.flow.inner.offer(value, false) {
            Offer::Delivered(wakers) | Offer::Queued(_, wakers) => {
                self.flow.inner.wake_collectors(wakers);
                true
            }
            Offer::Rejected => false,
        }
    }

    /// A read-only view — Kotlin's `asSharedFlow()`.
    pub fn as_shared_flow(&self) -> SharedFlow<T> {
        self.flow.clone()
    }
}

/// The future returned by [`MutableSharedFlow::emit`].
#[must_use = "the value is emitted only when the future is awaited"]
pub struct EmitShared<'a, T> {
    flow: &'a MutableSharedFlow<T>,
    value: Option<T>,
    queued: Option<u64>,
}

impl<T> Unpin for EmitShared<'_, T> {}

impl<T> Future for EmitShared<'_, T> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        let inner = &this.flow.flow.inner;
        if let Some(value) = this.value.take() {
            match inner.offer(value, true) {
                Offer::Delivered(wakers) => {
                    inner.wake_collectors(wakers);
                    return Poll::Ready(());
                }
                Offer::Rejected => return Poll::Ready(()),
                Offer::Queued(sequence, wakers) => {
                    inner.wake_collectors(wakers);
                    this.queued = Some(sequence);
                }
            }
        }
        match this.queued {
            Some(sequence) => inner.poll_room(sequence, cx),
            None => Poll::Ready(()),
        }
    }
}

/// The read-only side of a [`MutableSharedFlow`] — Kotlin's `SharedFlow`.
///
/// A [`MutableSharedFlow`] dereferences to it, the way Kotlin's
/// `MutableSharedFlow` extends `SharedFlow`.
pub struct SharedFlow<T> {
    inner: Arc<SharedInner<T>>,
}

impl<T> Clone for SharedFlow<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> SharedFlow<T> {
    /// How many collectors are running right now.
    pub fn subscription_count(&self) -> usize {
        self.inner.subscribers.current()
    }

    pub(crate) fn subscribers(&self) -> &SubscriberCount {
        &self.inner.subscribers
    }
}

/// One subscription to a [`SharedFlow`]; dropping it unsubscribes.
pub struct SharedRun<T> {
    inner: Arc<SharedInner<T>>,
    key: usize,
}

fn open_shared<T>(inner: &Arc<SharedInner<T>>) -> SharedRun<T> {
    inner.subscribers.opened();
    let mut state = lock(&inner.state);
    let replayed = state.buffer.len().min(inner.replay) as u64;
    let cursor = state.end() - replayed;
    let key = state.wakers.insert();
    if state.cursors.len() <= key {
        state.cursors.resize(key + 1, None);
    }
    state.cursors[key] = Some(cursor);
    SharedRun {
        inner: Arc::clone(inner),
        key,
    }
}

impl<T: Clone> Flow for SharedFlow<T> {
    type Item = T;
    type Run = SharedRun<T>;

    fn open(&self) -> SharedRun<T> {
        open_shared(&self.inner)
    }
}

impl<T: Clone> Flow for MutableSharedFlow<T> {
    type Item = T;
    type Run = SharedRun<T>;

    fn open(&self) -> SharedRun<T> {
        self.flow.open()
    }
}

impl<T> Unpin for SharedRun<T> {}

impl<T: Clone> Stream for SharedRun<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let this = self.get_mut();
        let (value, emitters) = {
            let mut state = lock(&this.inner.state);
            let first = state.first_sequence;
            let Some(cursor) = state.cursors.get(this.key).copied().flatten() else {
                return Poll::Ready(None);
            };
            let cursor = cursor.max(first);
            let Some(value) = state.buffer.get((cursor - first) as usize).cloned() else {
                state.cursors[this.key] = Some(cursor);
                state.wakers.register(this.key, cx.waker());
                return Poll::Pending;
            };
            state.cursors[this.key] = Some(cursor + 1);
            state.trim(this.inner.replay);
            (value, std::mem::take(&mut state.emitters))
        };
        for emitter in emitters {
            emitter.wake();
        }
        Poll::Ready(Some(value))
    }
}

impl<T> Drop for SharedRun<T> {
    fn drop(&mut self) {
        let emitters = {
            let mut state = lock(&self.inner.state);
            state.wakers.remove(self.key);
            if let Some(cursor) = state.cursors.get_mut(self.key) {
                *cursor = None;
            }
            state.trim(self.inner.replay);
            std::mem::take(&mut state.emitters)
        };
        for emitter in emitters {
            emitter.wake();
        }
        self.inner.subscribers.closed();
    }
}
