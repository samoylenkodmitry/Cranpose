use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

use crate::{
    FrameCallbackId,
    runtime::{FrameCallbackKind, RuntimeHandle},
};

#[derive(Clone)]
pub struct FrameClock {
    runtime: RuntimeHandle,
}

impl FrameClock {
    pub fn new(runtime: RuntimeHandle) -> Self {
        Self { runtime }
    }

    pub fn runtime_handle(&self) -> RuntimeHandle {
        self.runtime.clone()
    }

    pub fn with_frame_nanos(
        &self,
        callback: impl FnOnce(u64) + 'static,
    ) -> FrameCallbackRegistration {
        self.with_frame_nanos_kind(FrameCallbackKind::Transient, callback)
    }

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
        let mut callback_opt = Some(callback);
        let runtime = self.runtime.clone();
        let register = move |callback| match kind {
            FrameCallbackKind::Transient => runtime.register_frame_callback(callback),
            FrameCallbackKind::Perpetual => runtime.register_perpetual_frame_callback(callback),
        };
        match register(Box::new(move |time| {
            if let Some(callback) = callback_opt.take() {
                callback(time);
            }
        })) {
            Some(id) => FrameCallbackRegistration::new(self.runtime.clone(), id),
            None => FrameCallbackRegistration::inactive(self.runtime.clone()),
        }
    }

    pub fn with_frame_millis(
        &self,
        callback: impl FnOnce(u64) + 'static,
    ) -> FrameCallbackRegistration {
        self.with_frame_nanos(move |nanos| {
            let millis = nanos / 1_000_000;
            callback(millis);
        })
    }

    pub fn next_frame(&self) -> NextFrame {
        NextFrame::new(self.clone(), FrameCallbackKind::Transient)
    }

    pub fn next_perpetual_frame(&self) -> NextFrame {
        NextFrame::new(self.clone(), FrameCallbackKind::Perpetual)
    }
}

pub struct FrameCallbackRegistration {
    runtime: RuntimeHandle,
    id: Option<FrameCallbackId>,
}

struct NextFrameState {
    registration: Option<FrameCallbackRegistration>,
    time: Option<u64>,
    waker: Option<Waker>,
}

impl NextFrameState {
    fn new() -> Self {
        Self {
            registration: None,
            time: None,
            waker: None,
        }
    }
}

pub struct NextFrame {
    clock: FrameClock,
    kind: FrameCallbackKind,
    state: Rc<RefCell<NextFrameState>>,
}

impl NextFrame {
    fn new(clock: FrameClock, kind: FrameCallbackKind) -> Self {
        Self {
            clock,
            kind,
            state: Rc::new(RefCell::new(NextFrameState::new())),
        }
    }
}

impl Future for NextFrame {
    type Output = u64;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(time) = self.state.borrow().time {
            return Poll::Ready(time);
        }

        {
            let mut state = self.state.borrow_mut();
            state.waker = Some(cx.waker().clone());
            if state.registration.is_none() {
                drop(state);
                let state = Rc::downgrade(&self.state);
                let callback = move |time| {
                    let Some(state) = state.upgrade() else {
                        return;
                    };
                    let (registration, waker) = {
                        let mut state = state.borrow_mut();
                        state.time = Some(time);
                        (state.registration.take(), state.waker.take())
                    };
                    drop(registration);
                    if let Some(waker) = waker {
                        waker.wake();
                    }
                };
                let registration = match self.kind {
                    FrameCallbackKind::Transient => self.clock.with_frame_nanos(callback),
                    FrameCallbackKind::Perpetual => self.clock.with_perpetual_frame_nanos(callback),
                };
                self.state.borrow_mut().registration = Some(registration);
            }
        }

        if let Some(time) = self.state.borrow().time {
            Poll::Ready(time)
        } else {
            Poll::Pending
        }
    }
}

impl Drop for NextFrame {
    fn drop(&mut self) {
        if let Some(registration) = self.state.borrow_mut().registration.take() {
            drop(registration);
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
