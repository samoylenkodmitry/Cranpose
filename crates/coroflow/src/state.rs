use std::{
    ops::Deref,
    pin::Pin,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll},
};

use futures_core::Stream;

use crate::{flow::Flow, sync::Watch};

#[derive(Default)]
struct CountInner {
    count: OnceLock<Arc<StateShared<usize>>>,
    opened: AtomicU64,
}

#[derive(Clone, Default)]
pub(crate) struct SubscriberCount {
    inner: Arc<CountInner>,
}

impl SubscriberCount {
    fn shared(&self) -> &Arc<StateShared<usize>> {
        self.inner.count.get_or_init(|| StateShared::new(0))
    }

    pub(crate) fn opened(&self) {
        self.inner.opened.fetch_add(1, Ordering::AcqRel);
        self.shared().value.update(|count| {
            *count += 1;
            true
        });
    }

    pub(crate) fn closed(&self) {
        self.shared().value.update(|count| {
            *count = count.saturating_sub(1);
            true
        });
    }

    pub(crate) fn opened_total(&self) -> u64 {
        self.inner.opened.load(Ordering::Acquire)
    }

    pub(crate) fn watch(&self) -> &Watch<usize> {
        &self.shared().value
    }

    pub(crate) fn as_flow(&self) -> StateFlow<usize> {
        StateFlow::from_shared(Arc::clone(self.shared()))
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
    pub fn update(&self, transform: impl FnMut(&T) -> T) {
        self.update_reporting(transform, |_, _| ());
    }

    /// Like [`update`](MutableStateFlow::update), returning the new value —
    /// Kotlin's `updateAndGet`.
    pub fn update_and_get(&self, transform: impl FnMut(&T) -> T) -> T {
        self.update_reporting(transform, |_, next| next.clone())
    }

    /// Like [`update`](MutableStateFlow::update), returning the value it
    /// replaced — Kotlin's `getAndUpdate`.
    pub fn get_and_update(&self, transform: impl FnMut(&T) -> T) -> T {
        self.update_reporting(transform, |previous, _| previous.clone())
    }

    /// Sets the value to `update` if it currently equals `expect`, and reports
    /// whether it did — Kotlin's `compareAndSet`.
    pub fn compare_and_set(&self, expect: &T, update: T) -> bool {
        let mut matched = false;
        self.state.shared.value.update(|current| {
            if current != expect {
                return false;
            }
            matched = true;
            if *current == update {
                return false;
            }
            *current = update;
            true
        });
        matched
    }

    fn update_reporting<R>(
        &self,
        mut transform: impl FnMut(&T) -> T,
        report: impl Fn(&T, &T) -> R,
    ) -> R {
        let watch = &self.state.shared.value;
        loop {
            let (current, version) = watch.read_versioned();
            let next = transform(&current);
            let reported = report(&current, &next);
            let committed = watch.update_at(version, |value| {
                if *value == next {
                    return false;
                }
                *value = next;
                true
            });
            if committed {
                return reported;
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

    /// How many collectors are running, as a state flow of its own — Kotlin's
    /// `subscriptionCount`.
    pub fn subscription_count(&self) -> StateFlow<usize> {
        self.shared.subscribers.as_flow()
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
