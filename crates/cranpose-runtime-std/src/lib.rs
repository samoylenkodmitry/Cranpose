//! Standard runtime services backed by Rust's `std` library.
//!
//! This crate provides concrete implementations of the platform
//! abstraction traits defined in `cranpose-core`. Applications can
//! construct a [`StdRuntime`] and pass it to [`cranpose_core::Composition`]
//! to power the runtime with `std` primitives.

#![deny(unsafe_code)]

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;
use web_time::Instant;

#[cfg(feature = "internal")]
use cranpose_core::internal::FrameClock;
use cranpose_core::{Clock, Runtime, RuntimeHandle, RuntimeScheduler};

#[cfg(not(target_arch = "wasm32"))]
type NativeFrameWaker = Arc<dyn Fn() + Send + Sync + 'static>;

/// Scheduler that delegates work to Rust's threading primitives.
pub struct StdScheduler {
    frame_requested: AtomicBool,
    #[cfg(not(target_arch = "wasm32"))]
    frame_waker: RwLock<Option<NativeFrameWaker>>,
    #[cfg(target_arch = "wasm32")]
    frame_waker: RefCell<Option<Box<dyn Fn() + 'static>>>,
}

impl StdScheduler {
    pub fn new() -> Self {
        Self {
            frame_requested: AtomicBool::new(false),
            frame_waker: Default::default(),
        }
    }

    /// Returns whether a frame has been requested since the last call.
    pub fn take_frame_request(&self) -> bool {
        self.frame_requested.swap(false, Ordering::SeqCst)
    }

    /// Returns whether a frame is currently pending without consuming the request.
    pub fn has_frame_request(&self) -> bool {
        self.frame_requested.load(Ordering::SeqCst)
    }

    /// Registers a waker that will be invoked whenever a new frame is scheduled.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_frame_waker(&self, waker: impl Fn() + Send + Sync + 'static) {
        let old_waker = {
            let mut frame_waker = self.frame_waker_write();
            frame_waker.replace(Arc::new(waker))
        };
        drop(old_waker);
    }

    #[cfg(target_arch = "wasm32")]
    pub fn set_frame_waker(&self, waker: impl Fn() + 'static) {
        *self.frame_waker.borrow_mut() = Some(Box::new(waker));
    }

    /// Clears any registered frame waker.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn clear_frame_waker(&self) {
        let old_waker = {
            let mut frame_waker = self.frame_waker_write();
            frame_waker.take()
        };
        drop(old_waker);
    }

    /// Clears any registered frame waker.
    #[cfg(target_arch = "wasm32")]
    pub fn clear_frame_waker(&self) {
        *self.frame_waker.borrow_mut() = None;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn wake(&self) {
        let waker = self.frame_waker_read().clone();
        if let Some(waker) = waker {
            waker();
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn wake(&self) {
        if let Some(waker) = self.frame_waker.borrow().as_ref() {
            waker();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn frame_waker_read(&self) -> RwLockReadGuard<'_, Option<NativeFrameWaker>> {
        match self.frame_waker.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn frame_waker_write(&self) -> RwLockWriteGuard<'_, Option<NativeFrameWaker>> {
        match self.frame_waker.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

impl Default for StdScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for StdScheduler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StdScheduler")
            .field(
                "frame_requested",
                &self.frame_requested.load(Ordering::SeqCst),
            )
            .finish()
    }
}

impl RuntimeScheduler for StdScheduler {
    fn schedule_frame(&self) {
        self.frame_requested.store(true, Ordering::SeqCst);
        self.wake();
    }
}

/// Shared handle to a [`StdScheduler`].
///
/// Mirrors [`cranpose_core::SchedulerRef`]: native code wakes the runtime
/// from other threads, so the handle needs to be an atomically
/// reference-counted `Arc`. On wasm `StdScheduler` keeps its frame waker in a
/// `RefCell` because the host is single-threaded, so the type is not
/// `Send + Sync` there and an `Rc` handle is used instead of paying for
/// synchronisation the target has no use for.
#[cfg(not(target_arch = "wasm32"))]
pub type StdSchedulerRef = Arc<StdScheduler>;

/// See the native definition of [`StdSchedulerRef`] for why this is `Rc` on wasm.
#[cfg(target_arch = "wasm32")]
pub type StdSchedulerRef = std::rc::Rc<StdScheduler>;

/// Clock implementation backed by a cross-platform monotonic timer.
#[derive(Debug, Default, Clone)]
pub struct StdClock;

impl Clock for StdClock {
    type Instant = Instant;

    fn now(&self) -> Self::Instant {
        Instant::now()
    }

    fn elapsed_millis(&self, since: Self::Instant) -> u64 {
        since.elapsed().as_millis() as u64
    }
}

impl StdClock {
    /// Returns the elapsed time as a [`Duration`] for convenience.
    pub fn elapsed(&self, since: Instant) -> Duration {
        since.elapsed()
    }
}

/// Convenience container bundling the standard scheduler and clock.
#[derive(Clone)]
pub struct StdRuntime {
    scheduler: StdSchedulerRef,
    clock: Arc<StdClock>,
    runtime: Runtime,
}

impl StdRuntime {
    /// Creates a new standard runtime instance.
    pub fn new() -> Self {
        let scheduler = StdSchedulerRef::new(StdScheduler::default());
        let runtime = Runtime::new(scheduler.clone());
        Self {
            scheduler,
            clock: Arc::new(StdClock),
            runtime,
        }
    }

    /// Returns a [`cranpose_core::Runtime`] configured with the standard scheduler.
    pub fn runtime(&self) -> Runtime {
        self.runtime.clone()
    }

    /// Returns a handle to the runtime.
    pub fn runtime_handle(&self) -> RuntimeHandle {
        self.runtime.handle()
    }

    /// Returns the runtime's frame clock.
    #[cfg(feature = "internal")]
    pub fn frame_clock(&self) -> FrameClock {
        self.runtime.frame_clock()
    }

    /// Returns the scheduler implementation.
    pub fn scheduler(&self) -> StdSchedulerRef {
        StdSchedulerRef::clone(&self.scheduler)
    }

    /// Returns the clock implementation.
    pub fn clock(&self) -> Arc<StdClock> {
        Arc::clone(&self.clock)
    }

    /// Returns whether a frame was requested since the last poll.
    pub fn take_frame_request(&self) -> bool {
        self.scheduler.take_frame_request()
    }

    pub fn has_frame_request(&self) -> bool {
        self.scheduler.has_frame_request()
    }

    /// Registers a waker to be called when the runtime schedules a new frame.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_frame_waker(&self, waker: impl Fn() + Send + Sync + 'static) {
        self.scheduler.set_frame_waker(waker);
    }

    #[cfg(target_arch = "wasm32")]
    pub fn set_frame_waker(&self, waker: impl Fn() + 'static) {
        self.scheduler.set_frame_waker(waker);
    }

    /// Clears any previously registered frame waker.
    pub fn clear_frame_waker(&self) {
        self.scheduler.clear_frame_waker();
    }

    /// Drains pending frame callbacks using the provided frame timestamp in nanoseconds.
    pub fn drain_frame_callbacks(&self, frame_time_nanos: u64) {
        self.runtime_handle()
            .drain_frame_callbacks(frame_time_nanos);
    }
}

impl fmt::Debug for StdRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StdRuntime")
            .field("scheduler", &self.scheduler)
            .field("clock", &self.clock)
            .finish()
    }
}

impl Default for StdRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "tests/std_runtime_tests.rs"]
mod tests;
