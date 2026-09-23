use std::{
    ops::Deref,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures_core::Stream;

use crate::{flow::Flow, sync::Watch};

#[derive(Clone, Copy, Default)]
pub(crate) struct Subscribers {
    pub(crate) current: usize,
    pub(crate) opened: u64,
}

#[derive(Clone)]
pub(crate) struct SubscriberCount {
    watch: Arc<Watch<Subscribers>>,
}

impl Default for SubscriberCount {
    fn default() -> Self {
        Self {
            watch: Arc::new(Watch::new(Subscribers::default())),
        }
    }
}

impl SubscriberCount {
    pub(crate) fn opened(&self) {
        self.watch.update(|subscribers| {
            subscribers.current += 1;
            subscribers.opened += 1;
            true
        });
    }

    pub(crate) fn closed(&self) {
        self.watch.update(|subscribers| {
            subscribers.current = subscribers.current.saturating_sub(1);
            true
        });
    }

    pub(crate) fn current(&self) -> usize {
        self.watch.with(|subscribers| subscribers.current)
    }

    pub(crate) fn watch(&self) -> &Watch<Subscribers> {
        &self.watch
    }
}

pub(crate) struct StateShared<T> {
    pub(crate) value: Watch<T>,
    pub(crate) subscribers: SubscriberCount,
}

impl<T> StateShared<T> {
    pub(crate) fn new(value: T) -> Arc<Self> {
        Arc::new(Self {
            value: Watch::new(value),
            subscribers: SubscriberCount::default(),
        })
    }

    fn subscription_count(&self) -> usize {
        self.subscribers.current()
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

    pub(crate) fn publish(&self, value: T)
    where
        T: PartialEq,
    {
        self.shared.set(value);
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
    shared.subscribers.opened();
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
        self.shared.subscribers.closed();
    }
}
