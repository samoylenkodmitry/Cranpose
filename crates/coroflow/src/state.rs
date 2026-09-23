use std::{
    collections::VecDeque,
    ops::Deref,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use futures_core::Stream;

use crate::{
    flow::Flow,
    sync::{WakerSet, Watch, lock, wake_and_empty},
};

#[derive(Clone, Copy, Default)]
pub(crate) struct Subscribers {
    pub(crate) current: usize,
    pub(crate) opened: u64,
}

pub(crate) struct StateShared<T> {
    pub(crate) value: Watch<T>,
    pub(crate) subscribers: Watch<Subscribers>,
}

impl<T> StateShared<T> {
    pub(crate) fn new(value: T) -> Arc<Self> {
        Arc::new(Self {
            value: Watch::new(value),
            subscribers: Watch::new(Subscribers::default()),
        })
    }

    fn subscription_count(&self) -> usize {
        self.subscribers.with(|subscribers| subscribers.current)
    }

    pub(crate) fn set(&self, value: T)
    where
        T: PartialEq,
    {
        self.value.update(|current| {
            if *current == value {
                return false;
            }
            *current = value;
            true
        });
    }
}

/// A hot, observable holder of one value — Kotlin's `MutableStateFlow`.
///
/// Collectors receive the current value first and then every change;
/// a slow collector sees only the latest value. Setting an equal value is not a
/// change. Cheap to clone; clones share the value.
pub struct MutableStateFlow<T> {
    state: StateFlow<T>,
}

impl<T> Clone for MutableStateFlow<T> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl<T> Deref for MutableStateFlow<T> {
    type Target = StateFlow<T>;

    fn deref(&self) -> &StateFlow<T> {
        &self.state
    }
}

impl<T: Clone + PartialEq> MutableStateFlow<T> {
    /// A state flow holding `initial`.
    pub fn new(initial: T) -> Self {
        Self {
            state: StateFlow::from_shared(StateShared::new(initial)),
        }
    }

    /// Replaces the value and notifies collectors when it changed.
    pub fn set(&self, value: T) {
        self.state.shared.set(value);
    }

    /// Atomically replaces the value with `transform(current)` — Kotlin's
    /// `update`.
    ///
    /// `transform` runs without holding the flow's lock, so it may read this
    /// flow; if another write lands first it runs again on the newer value.
    pub fn update(&self, mut transform: impl FnMut(&T) -> T) {
        let watch = &self.state.shared.value;
        loop {
            let (current, version) = watch.read_versioned();
            let next = transform(&current);
            let committed = watch.update_at(version, |value| {
                if *value == next {
                    return false;
                }
                *value = next;
                true
            });
            if committed {
                return;
            }
        }
    }

    /// A read-only view — Kotlin's `asStateFlow()`.
    pub fn as_state_flow(&self) -> StateFlow<T> {
        self.state.clone()
    }
}

/// The read-only side of a [`MutableStateFlow`] — Kotlin's `StateFlow`.
///
/// A [`MutableStateFlow`] dereferences to it, the way Kotlin's
/// `MutableStateFlow` extends `StateFlow`.
pub struct StateFlow<T> {
    shared: Arc<StateShared<T>>,
}

impl<T> Clone for StateFlow<T> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T> StateFlow<T> {
    pub(crate) fn from_shared(shared: Arc<StateShared<T>>) -> Self {
        Self { shared }
    }

    /// Whether both handles observe the same state.
    pub fn same_as(&self, other: &StateFlow<T>) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }
}

impl<T: Clone> StateFlow<T> {
    /// The current value.
    pub fn value(&self) -> T {
        self.shared.value.with(T::clone)
    }

    /// How many collectors are running right now.
    pub fn subscription_count(&self) -> usize {
        self.shared.subscription_count()
    }
}

/// One subscription to a [`StateFlow`]; dropping it unsubscribes.
pub struct StateRun<T> {
    shared: Arc<StateShared<T>>,
    key: usize,
    seen: Option<u64>,
}

fn open_state<T>(shared: &Arc<StateShared<T>>) -> StateRun<T> {
    shared.subscribers.update(|subscribers| {
        subscribers.current += 1;
        subscribers.opened += 1;
        true
    });
    StateRun {
        shared: Arc::clone(shared),
        key: shared.value.subscribe(),
        seen: None,
    }
}

impl<T: Clone> Flow for StateFlow<T> {
    type Item = T;
    type Run = StateRun<T>;

    fn open(&self) -> StateRun<T> {
        open_state(&self.shared)
    }
}

impl<T: Clone> Flow for MutableStateFlow<T> {
    type Item = T;
    type Run = StateRun<T>;

    fn open(&self) -> StateRun<T> {
        self.state.open()
    }
}

impl<T> Unpin for StateRun<T> {}

impl<T: Clone> Stream for StateRun<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let this = self.get_mut();
        this.shared
            .value
            .poll_changed(this.key, &mut this.seen, cx, T::clone)
            .map(Some)
    }
}

impl<T> Drop for StateRun<T> {
    fn drop(&mut self) {
        self.shared.value.unsubscribe(self.key);
        self.shared.subscribers.update(|subscribers| {
            subscribers.current = subscribers.current.saturating_sub(1);
            true
        });
    }
}

struct SharedState<T> {
    buffer: VecDeque<T>,
    first_sequence: u64,
    wakers: WakerSet,
    subscribers: usize,
}

struct SharedInner<T> {
    state: Mutex<SharedState<T>>,
    replay: usize,
    capacity: usize,
}

/// A hot broadcast of events to every current collector — Kotlin's
/// `MutableSharedFlow` with `BufferOverflow.DROP_OLDEST`.
///
/// It keeps the last `replay + extra_capacity` values (at least one). A new
/// collector first receives the last `replay` of them; a collector that falls
/// further behind than the buffer skips the values it missed.
pub struct MutableSharedFlow<T> {
    inner: Arc<SharedInner<T>>,
}

impl<T> Clone for MutableSharedFlow<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T: Clone> MutableSharedFlow<T> {
    /// A shared flow that replays `replay` values and buffers
    /// `extra_capacity` more for slow collectors.
    pub fn new(replay: usize, extra_capacity: usize) -> Self {
        let capacity = (replay + extra_capacity).max(1);
        Self {
            inner: Arc::new(SharedInner {
                state: Mutex::new(SharedState {
                    buffer: VecDeque::with_capacity(capacity),
                    first_sequence: 0,
                    wakers: WakerSet::default(),
                    subscribers: 0,
                }),
                replay,
                capacity,
            }),
        }
    }

    /// Broadcasts `value`, dropping the oldest buffered value when full.
    pub fn emit(&self, value: T) {
        let (evicted, wakers) = {
            let mut state = lock(&self.inner.state);
            let evicted = if state.buffer.len() == self.inner.capacity {
                state.first_sequence += 1;
                state.buffer.pop_front()
            } else {
                None
            };
            state.buffer.push_back(value);
            (evicted, state.wakers.take_wakers())
        };
        drop(evicted);
        let emptied = wake_and_empty(wakers);
        lock(&self.inner.state).wakers.recycle(emptied);
    }

    /// A read-only view — Kotlin's `asSharedFlow()`.
    pub fn as_shared_flow(&self) -> SharedFlow<T> {
        SharedFlow {
            inner: Arc::clone(&self.inner),
        }
    }

    /// How many collectors are running right now.
    pub fn subscription_count(&self) -> usize {
        lock(&self.inner.state).subscribers
    }
}

/// The read-only side of a [`MutableSharedFlow`] — Kotlin's `SharedFlow`.
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

/// One subscription to a [`SharedFlow`]; dropping it unsubscribes.
pub struct SharedRun<T> {
    inner: Arc<SharedInner<T>>,
    key: usize,
    cursor: u64,
}

fn open_shared<T>(inner: &Arc<SharedInner<T>>) -> SharedRun<T> {
    let mut state = lock(&inner.state);
    state.subscribers += 1;
    let end = state.first_sequence + state.buffer.len() as u64;
    let replayed = state.buffer.len().min(inner.replay) as u64;
    let key = state.wakers.insert();
    SharedRun {
        inner: Arc::clone(inner),
        key,
        cursor: end - replayed,
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
        open_shared(&self.inner)
    }
}

impl<T> Unpin for SharedRun<T> {}

impl<T: Clone> Stream for SharedRun<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let this = self.get_mut();
        let mut state = lock(&this.inner.state);
        this.cursor = this.cursor.max(state.first_sequence);
        let index = (this.cursor - state.first_sequence) as usize;
        match state.buffer.get(index) {
            Some(value) => {
                this.cursor += 1;
                Poll::Ready(Some(value.clone()))
            }
            None => {
                state.wakers.register(this.key, cx.waker());
                Poll::Pending
            }
        }
    }
}

impl<T> Drop for SharedRun<T> {
    fn drop(&mut self) {
        let mut state = lock(&self.inner.state);
        state.subscribers = state.subscribers.saturating_sub(1);
        state.wakers.remove(self.key);
    }
}
