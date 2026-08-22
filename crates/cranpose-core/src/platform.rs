//! Platform abstraction traits for Compose runtime services.
//!
//! These traits allow Compose to delegate scheduling and clock
//! responsibilities to the host platform, enabling integration with
//! different environments without depending directly on `std` APIs.

/// Schedules work for the Compose runtime.
///
/// Implementations are responsible for triggering frame processing and
/// executing background tasks on behalf of Compose.
#[cfg(not(target_arch = "wasm32"))]
pub trait RuntimeScheduler: Send + Sync {
    /// Request that the host schedule a new frame.
    fn schedule_frame(&self);
}

/// Schedules work for the Compose runtime on single-threaded wasm hosts.
#[cfg(target_arch = "wasm32")]
pub trait RuntimeScheduler {
    /// Request that the host schedule a new frame.
    fn schedule_frame(&self);
}

/// Shared handle to a [`RuntimeScheduler`].
///
/// A native host wakes the runtime from other threads (a background task
/// finishing, a timer firing), so the handle has to be an atomically
/// reference-counted pointer to a `Send + Sync` scheduler. A wasm host runs
/// everything on a single thread and its scheduler holds JS values that can
/// only ever live on that thread, so it cannot be `Send + Sync`; wrapping it
/// in `Arc` there would buy synchronisation the target has no use for, so a
/// plain `Rc` is used instead.
#[cfg(not(target_arch = "wasm32"))]
pub type SchedulerRef = std::sync::Arc<dyn RuntimeScheduler>;

/// Shared handle to a [`RuntimeScheduler`]. See the native definition for why
/// this is `Rc` on wasm instead of `Arc`.
#[cfg(target_arch = "wasm32")]
pub type SchedulerRef = std::rc::Rc<dyn RuntimeScheduler>;

/// Wraps `scheduler` in a [`SchedulerRef`].
///
/// A trait-object alias cannot expose its own `new` the way a concrete type
/// can (`Arc<dyn Trait>::new` has no `Sized` value to accept), so this is the
/// one place that picks `Arc` on native and `Rc` on wasm; callers that need a
/// [`SchedulerRef`] from a concrete scheduler go through this instead of
/// repeating that choice.
#[cfg(not(target_arch = "wasm32"))]
pub fn scheduler_ref<S: RuntimeScheduler + 'static>(scheduler: S) -> SchedulerRef {
    std::sync::Arc::new(scheduler)
}

/// See the native definition of [`scheduler_ref`] for why this is `Rc` on wasm.
#[cfg(target_arch = "wasm32")]
pub fn scheduler_ref<S: RuntimeScheduler + 'static>(scheduler: S) -> SchedulerRef {
    std::rc::Rc::new(scheduler)
}

/// Provides timing information for the runtime.
pub trait Clock: Send + Sync {
    /// Instant type produced by this clock implementation.
    type Instant: Copy + Send + Sync;

    /// Returns the current instant.
    fn now(&self) -> Self::Instant;

    /// Returns the number of milliseconds elapsed since `since`.
    fn elapsed_millis(&self, since: Self::Instant) -> u64;
}
