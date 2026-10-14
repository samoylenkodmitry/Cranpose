use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    FrameCallbackId,
    runtime::{FrameCallbackKind, RuntimeHandle},
};

/// Delivers display frames of one runtime to callbacks and futures, as Jetpack Compose's
/// `MonotonicFrameClock`.
#[derive(Clone)]
pub struct FrameClock {
    runtime: RuntimeHandle,
}

impl FrameClock {
    /// Creates a clock that delivers the frames of `runtime`.
    pub fn new(runtime: RuntimeHandle) -> Self {
        Self { runtime }
    }

    /// Returns a handle to the runtime of this clock.
    pub fn runtime_handle(&self) -> RuntimeHandle {
        self.runtime.clone()
    }

    /// Runs `callback` once on the next frame. It receives the frame time in nanoseconds on the
    /// runtime's monotonic animation clock. Dropping or cancelling the returned registration before
    /// the frame cancels the callback. Compose's `withFrameNanos`.
    pub fn with_frame_nanos(
        &self,
        callback: impl FnOnce(u64) + 'static,
    ) -> FrameCallbackRegistration {
        self.with_frame_nanos_kind(FrameCallbackKind::Transient, callback)
    }

    /// Runs `callback` once on the next frame with the frame time in nanoseconds, for a long-lived
    /// loop that registers again after every frame. The callback does not count as animation work
    /// for `RuntimeHandle::has_transient_frame_callbacks`.
    pub fn with_perpetual_frame_nanos(
        &self,
        callback: impl FnOnce(u64) + 'static,
    ) -> FrameCallbackRegistration {
        self.with_frame_nanos_kind(FrameCallbackKind::Perpetual, callback)
    }

    fn with_frame_nanos_kind(
        &self,
        kind: FrameCallbackKind,
        callback: impl FnOnce(u64) + 'static,
    ) -> FrameCallbackRegistration {
        let id = match kind {
            FrameCallbackKind::Transient => self.runtime.register_frame_callback(callback),
            FrameCallbackKind::Perpetual => {
                self.runtime.register_perpetual_frame_callback(callback)
            }
        };
        match id {
            Some(id) => FrameCallbackRegistration::new(self.runtime.clone(), id),
            None => FrameCallbackRegistration::inactive(self.runtime.clone()),
        }
    }

    /// Runs `callback` once on the next frame with the frame time in milliseconds.
    pub fn with_frame_millis(
        &self,
        callback: impl FnOnce(u64) + 'static,
    ) -> FrameCallbackRegistration {
        self.with_frame_nanos(move |nanos| {
            let millis = nanos / 1_000_000;
            callback(millis);
        })
    }

    /// Returns a future that completes on the next frame with the frame time in nanoseconds. It
    /// registers its callback when first polled and cancels it when dropped.
    pub fn next_frame(&self) -> NextFrame {
        NextFrame::new(self.clone(), FrameCallbackKind::Transient)
    }

    /// Returns a future that completes on the next frame with the frame time in nanoseconds. Its
    /// pending frame request does not count as animation work for
    /// `RuntimeHandle::has_transient_frame_callbacks`.
    pub fn next_perpetual_frame(&self) -> NextFrame {
        NextFrame::new(self.clone(), FrameCallbackKind::Perpetual)
    }
}

/// The handle of a registered frame callback. Dropping it cancels the callback if it has not run.
pub struct FrameCallbackRegistration {
    runtime: RuntimeHandle,
    id: Option<FrameCallbackId>,
}

/// The time of the next frame. Its task's waker waits in the runtime's frame
/// callbacks, so awaiting a frame boxes nothing.
pub struct NextFrame {
    clock: FrameClock,
    kind: FrameCallbackKind,
    registration: Option<FrameCallbackRegistration>,
}

impl NextFrame {
    fn new(clock: FrameClock, kind: FrameCallbackKind) -> Self {
        Self {
            clock,
            kind,
            registration: None,
        }
    }
}

impl Future for NextFrame {
    type Output = u64;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let runtime = &this.clock.runtime;
        let Some(registration) = &mut this.registration else {
            this.registration = Some(
                match runtime.register_frame_waker(this.kind, cx.waker().clone()) {
                    Some(id) => FrameCallbackRegistration::new(runtime.clone(), id),
                    None => FrameCallbackRegistration::inactive(runtime.clone()),
                },
            );
            return Poll::Pending;
        };
        let Some(id) = registration.id else {
            return Poll::Pending;
        };
        match runtime.frame_waker_time(id, cx.waker()) {
            Some(time) => {
                // The frame removed the entry: nothing is left to cancel.
                registration.id = None;
                Poll::Ready(time)
            }
            None => Poll::Pending,
        }
    }
}

impl FrameCallbackRegistration {
    fn new(runtime: RuntimeHandle, id: FrameCallbackId) -> Self {
        Self {
            runtime,
            id: Some(id),
        }
    }

    fn inactive(runtime: RuntimeHandle) -> Self {
        Self { runtime, id: None }
    }

    /// Cancels the callback if it has not run yet.
    pub fn cancel(mut self) {
        if let Some(id) = self.id.take() {
            self.runtime.cancel_frame_callback(id);
        }
    }
}

impl Drop for FrameCallbackRegistration {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            self.runtime.cancel_frame_callback(id);
        }
    }
}

#[cfg(test)]
#[path = "tests/frame_clock_tests.rs"]
mod tests;
