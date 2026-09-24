use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    task::{Context, Poll, Waker},
};

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Default)]
pub(crate) struct WakerSet {
    slots: Vec<Option<Waker>>,
    free: Vec<usize>,
    spare: Vec<Waker>,
}

impl WakerSet {
    pub(crate) fn insert(&mut self) -> usize {
        let key = match self.free.pop() {
            Some(key) => key,
            None => {
                self.slots.push(None);
                self.slots.len() - 1
            }
        };
        self.spare.reserve(self.slots.len());
        key
    }

    pub(crate) fn remove(&mut self, key: usize) {
        if let Some(slot) = self.slots.get_mut(key) {
            *slot = None;
            self.free.push(key);
        }
    }

    pub(crate) fn register(&mut self, key: usize, waker: &Waker) {
        if let Some(slot) = self.slots.get_mut(key) {
            match slot {
                Some(existing) if existing.will_wake(waker) => {}
                _ => *slot = Some(waker.clone()),
            }
        }
    }

    pub(crate) fn take_wakers(&mut self) -> Vec<Waker> {
        let mut wakers = std::mem::take(&mut self.spare);
        wakers.extend(self.slots.iter().flatten().cloned());
        wakers
    }

    pub(crate) fn recycle(&mut self, wakers: Vec<Waker>) {
        if wakers.capacity() > self.spare.capacity() {
            self.spare = wakers;
        }
    }
}

pub(crate) fn wake_and_empty(mut wakers: Vec<Waker>) -> Vec<Waker> {
    for waker in wakers.drain(..) {
        waker.wake();
    }
    wakers
}

fn wake_optional(waker: Option<Waker>) {
    if let Some(waker) = waker {
        waker.wake();
    }
}

pub(crate) struct Watch<T> {
    inner: Mutex<WatchInner<T>>,
}

struct WatchInner<T> {
    value: T,
    version: u64,
    wakers: WakerSet,
}

impl<T> Watch<T> {
    pub(crate) fn new(value: T) -> Self {
        Self {
            inner: Mutex::new(WatchInner {
                value,
                version: 0,
                wakers: WakerSet::default(),
            }),
        }
    }

    pub(crate) fn with<R>(&self, read: impl FnOnce(&T) -> R) -> R {
        read(&lock(&self.inner).value)
    }

    pub(crate) fn update(&self, change: impl FnOnce(&mut T) -> bool) {
        self.commit(None, change);
    }

    pub(crate) fn read_versioned(&self) -> (T, u64)
    where
        T: Clone,
    {
        let inner = lock(&self.inner);
        (inner.value.clone(), inner.version)
    }

    pub(crate) fn update_at(&self, version: u64, change: impl FnOnce(&mut T) -> bool) -> bool {
        self.commit(Some(version), change)
    }

    fn commit(&self, expected: Option<u64>, change: impl FnOnce(&mut T) -> bool) -> bool {
        let wakers = {
            let mut inner = lock(&self.inner);
            if expected.is_some_and(|version| version != inner.version) {
                return false;
            }
            if !change(&mut inner.value) {
                return true;
            }
            inner.version += 1;
            inner.wakers.take_wakers()
        };
        let emptied = wake_and_empty(wakers);
        lock(&self.inner).wakers.recycle(emptied);
        true
    }

    pub(crate) fn subscribe(&self) -> usize {
        lock(&self.inner).wakers.insert()
    }

    pub(crate) fn unsubscribe(&self, key: usize) {
        lock(&self.inner).wakers.remove(key);
    }

    pub(crate) fn poll_changed<R>(
        &self,
        key: usize,
        seen: &mut Option<u64>,
        cx: &mut Context<'_>,
        read: impl FnOnce(&T) -> R,
    ) -> Poll<R> {
        let mut inner = lock(&self.inner);
        if *seen == Some(inner.version) {
            inner.wakers.register(key, cx.waker());
            return Poll::Pending;
        }
        *seen = Some(inner.version);
        Poll::Ready(read(&inner.value))
    }
}

struct OneshotState<T> {
    value: Option<T>,
    closed: bool,
    waker: Option<Waker>,
}

pub(crate) struct OneshotSender<T> {
    shared: Arc<Mutex<OneshotState<T>>>,
}

pub(crate) struct OneshotReceiver<T> {
    shared: Arc<Mutex<OneshotState<T>>>,
}

pub(crate) fn oneshot<T>() -> (OneshotSender<T>, OneshotReceiver<T>) {
    let shared = Arc::new(Mutex::new(OneshotState {
        value: None,
        closed: false,
        waker: None,
    }));
    (
        OneshotSender {
            shared: Arc::clone(&shared),
        },
        OneshotReceiver { shared },
    )
}

impl<T> OneshotSender<T> {
    pub(crate) fn send(self, value: T) {
        lock(&self.shared).value = Some(value);
    }
}

impl<T> Drop for OneshotSender<T> {
    fn drop(&mut self) {
        let waker = {
            let mut state = lock(&self.shared);
            state.closed = true;
            state.waker.take()
        };
        wake_optional(waker);
    }
}

impl<T> Future for OneshotReceiver<T> {
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let mut state = lock(&self.shared);
        if let Some(value) = state.value.take() {
            return Poll::Ready(Some(value));
        }
        if state.closed {
            return Poll::Ready(None);
        }
        state.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}
