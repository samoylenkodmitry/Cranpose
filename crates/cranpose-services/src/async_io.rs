//! Waking a future from another thread.
//!
//! A platform's own I/O is blocking — a synchronous HTTP read, a provider call,
//! a file descriptor a system API hands over — and the framework has no thread
//! pool to hide that behind. What it has is the waker: work runs on a thread of
//! its own and, when it has something, wakes whoever was awaiting it.
//!
//! Two shapes cover everything here. A [`Signal`] carries one value, which is
//! what a request's status line is. A [`ChunkChannel`] carries a stream of byte
//! chunks with the consumer's progress bounding the producer, which is what a
//! response body is — without the bound, a slow reader and a fast server put the
//! whole download in memory, which is the thing streaming exists to avoid.

use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{Arc, Condvar, Mutex, PoisonError},
    task::{Context, Poll, Waker},
};

/// How many chunks may wait ahead of the consumer before the producer stops
/// reading. Enough to keep a socket busy across one scheduling gap, and far
/// short of holding a download in memory.
pub const MAX_PENDING_CHUNKS: usize = 8;

struct SignalState<T> {
    value: Option<T>,
    waker: Option<Waker>,
    closed: bool,
}

/// A one-value hand-off between a worker and a future.
///
/// The worker calls [`Signal::set`]; whoever awaits [`Signal::wait`] is woken
/// with it. A worker that dies without setting anything closes the signal, and
/// the wait resolves to `None` rather than hanging for ever.
pub struct Signal<T> {
    state: Arc<Mutex<SignalState<T>>>,
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
        }
    }
}

impl<T> Default for Signal<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Signal<T> {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(SignalState {
                value: None,
                waker: None,
                closed: false,
            })),
        }
    }

    /// Delivers the value and wakes the waiter. A second call is ignored: one
    /// signal carries one value.
    pub fn set(&self, value: T) {
        let waker = {
            let mut state = lock(&self.state);
            if state.closed {
                return;
            }
            state.value = Some(value);
            state.closed = true;
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Ends the signal with no value, so a waiter stops waiting.
    pub fn close(&self) {
        let waker = {
            let mut state = lock(&self.state);
            if state.closed {
                return;
            }
            state.closed = true;
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Resolves with the value, or `None` when the signal was closed empty.
    pub fn wait(&self) -> SignalWait<T> {
        SignalWait {
            state: Arc::clone(&self.state),
        }
    }
}

/// The future [`Signal::wait`] returns.
pub struct SignalWait<T> {
    state: Arc<Mutex<SignalState<T>>>,
}

impl<T> Future for SignalWait<T> {
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<T>> {
        let mut state = lock(&self.state);
        if let Some(value) = state.value.take() {
            return Poll::Ready(Some(value));
        }
        if state.closed {
            return Poll::Ready(None);
        }
        state.waker = Some(context.waker().clone());
        Poll::Pending
    }
}

struct ChunkState<E> {
    ready: VecDeque<Vec<u8>>,
    error: Option<E>,
    finished: bool,
    abandoned: bool,
    waker: Option<Waker>,
}

struct ChunkShared<E> {
    state: Mutex<ChunkState<E>>,
    room: Condvar,
}

/// The producing half of a chunked byte stream.
///
/// Held by whatever is doing the blocking read. Dropping it without calling
/// [`ChunkChannel::finish`] or [`ChunkChannel::fail`] ends the stream, so a
/// worker that panics does not leave a reader waiting for ever.
pub struct ChunkChannel<E> {
    shared: Arc<ChunkShared<E>>,
}

impl<E> ChunkChannel<E> {
    /// Creates a channel and its reading half.
    pub fn new() -> (Self, ChunkStream<E>) {
        let shared = Arc::new(ChunkShared {
            state: Mutex::new(ChunkState {
                ready: VecDeque::new(),
                error: None,
                finished: false,
                abandoned: false,
                waker: None,
            }),
            room: Condvar::new(),
        });
        (
            Self {
                shared: Arc::clone(&shared),
            },
            ChunkStream { shared },
        )
    }

    /// Publishes one chunk, waiting while the consumer is more than
    /// [`MAX_PENDING_CHUNKS`] behind.
    ///
    /// Returns `false` once the consumer has gone, which is the producer's
    /// signal to stop reading.
    pub fn push(&self, chunk: Vec<u8>) -> bool {
        let waker = {
            let mut state = lock(&self.shared.state);
            #[cfg(not(target_arch = "wasm32"))]
            while state.ready.len() >= MAX_PENDING_CHUNKS && !state.abandoned {
                state = self
                    .shared
                    .room
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            if state.abandoned || state.finished {
                return false;
            }
            state.ready.push_back(chunk);
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
        true
    }

    /// Ends the stream with an error.
    pub fn fail(&self, error: E) {
        let waker = {
            let mut state = lock(&self.shared.state);
            if state.finished {
                return;
            }
            state.error = Some(error);
            state.finished = true;
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Ends the stream normally.
    pub fn finish(&self) {
        let waker = {
            let mut state = lock(&self.shared.state);
            if state.finished {
                return;
            }
            state.finished = true;
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Whether the consumer has gone.
    pub fn is_abandoned(&self) -> bool {
        lock(&self.shared.state).abandoned
    }
}

impl<E> Drop for ChunkChannel<E> {
    fn drop(&mut self) {
        self.finish();
    }
}

/// The consuming half of a chunked byte stream.
pub struct ChunkStream<E> {
    shared: Arc<ChunkShared<E>>,
}

impl<E> ChunkStream<E> {
    /// Resolves with the next chunk, `Ok(None)` at the end of the stream, or
    /// the error the producer ended with.
    pub fn next(&self) -> ChunkNext<'_, E> {
        ChunkNext { stream: self }
    }
}

impl<E> Drop for ChunkStream<E> {
    fn drop(&mut self) {
        let mut state = lock(&self.shared.state);
        state.abandoned = true;
        drop(state);
        self.shared.room.notify_all();
    }
}

/// The future [`ChunkStream::next`] returns.
pub struct ChunkNext<'a, E> {
    stream: &'a ChunkStream<E>,
}

impl<E> Future for ChunkNext<'_, E> {
    type Output = Result<Option<Vec<u8>>, E>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let shared = Arc::clone(&self.stream.shared);
        let mut state = lock(&shared.state);
        if let Some(chunk) = state.ready.pop_front() {
            drop(state);
            shared.room.notify_one();
            return Poll::Ready(Ok(Some(chunk)));
        }
        if let Some(error) = state.error.take() {
            return Poll::Ready(Err(error));
        }
        if state.finished {
            return Poll::Ready(Ok(None));
        }
        state.waker = Some(context.waker().clone());
        Poll::Pending
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
#[path = "tests/async_io_tests.rs"]
mod tests;
