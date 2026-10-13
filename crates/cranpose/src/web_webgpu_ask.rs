//! Asking the browser for a WebGPU adapter while the page does other work.
//!
//! A browser that has `navigator.gpu` answers the ask only after its GPU
//! process has tried to make an adapter. Chrome on a Mate 20 X (Android 10),
//! which has no adapter to give, answered 10-22 ms after the ask on every
//! start, and the page waited idle for it before it could make its WebGL
//! context. The page asks first and resolves its fonts and makes its
//! renderer, which need no GPU, before it waits for the answer.

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

/// A WebGPU adapter ask the browser may still be answering.
pub(crate) enum WebGpuAsk {
    Answered(bool),
    Asking(Pin<Box<dyn Future<Output = bool>>>),
}

impl WebGpuAsk {
    /// Sends the ask; the browser starts on it before this returns.
    pub(crate) fn send() -> Self {
        let mut ask: Pin<Box<dyn Future<Output = bool>>> =
            Box::pin(wgpu::util::is_browser_webgpu_supported());
        // The first poll makes the request. Its waker never wakes anything:
        // the await in `present` polls again with the task's own.
        match ask.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(present) => Self::Answered(present),
            Poll::Pending => Self::Asking(ask),
        }
    }

    /// Whether the browser gave an adapter.
    pub(crate) async fn present(self) -> bool {
        match self {
            Self::Answered(present) => present,
            Self::Asking(ask) => ask.await,
        }
    }
}
