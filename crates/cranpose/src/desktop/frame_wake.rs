//! The wake a state write sends the desktop event loop for a frame.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use winit::{event_loop::EventLoopProxy, window::Window};

// The wakes the event loop has yet to take up. A frame that invalidates a
// thousand scopes asks for the next frame a thousand times, and each ask
// is a call into the window system: only the first of its kind since the
// loop took one up goes through.
#[derive(Default)]
pub(super) struct FrameWakes {
    redraw: AtomicBool,
    proxy: AtomicBool,
}

impl FrameWakes {
    // The loop takes up the redraw of the primary window that `pending`
    // says it asked for: the next request goes through, as what the redraw
    // draws may be older than the write behind that request.
    pub(super) fn redraw_taken(&self, pending: &mut bool) {
        *pending = false;
        self.redraw.store(false, Ordering::SeqCst);
    }

    // The loop takes up a wake of its proxy.
    pub(super) fn proxy_taken(&self) {
        self.proxy.store(false, Ordering::SeqCst);
    }

    // The loop is about to wait. A redraw request the window system
    // dropped, as one for an occluded window, holds no later one back.
    pub(super) fn loop_waits(&self) {
        self.redraw.store(false, Ordering::SeqCst);
        self.proxy_taken();
    }
}

// The frame waker of the app whose primary window is `window`, made on the
// event loop's thread. `shown` says whether the window is shown, and is
// `None` for a headless app, whose window no redraw drives.
pub(super) fn window_frame_waker(
    shown: Option<Arc<AtomicBool>>,
    wakes: Arc<FrameWakes>,
    window: Arc<dyn Window>,
    proxy: EventLoopProxy,
) -> impl Fn() + Send + Sync + 'static {
    primary_frame_waker(
        std::thread::current().id(),
        shown,
        wakes,
        move || window.request_redraw(),
        move || proxy.wake_up(),
    )
}

// A waker that asks the primary window for a redraw from the event loop's
// thread while the window is shown, and wakes the loop otherwise: from
// another thread, for a hidden window and for a headless app.
fn primary_frame_waker(
    event_loop_thread: std::thread::ThreadId,
    shown: Option<Arc<AtomicBool>>,
    wakes: Arc<FrameWakes>,
    request_redraw: impl Fn() + Send + Sync + 'static,
    wake_event_loop: impl Fn() + Send + Sync + 'static,
) -> impl Fn() + Send + Sync + 'static {
    move || {
        let redraws = std::thread::current().id() == event_loop_thread
            && shown
                .as_ref()
                .is_some_and(|shown| shown.load(Ordering::Relaxed));
        if redraws {
            if !wakes.redraw.swap(true, Ordering::SeqCst) {
                request_redraw();
            }
        } else if !wakes.proxy.swap(true, Ordering::SeqCst) {
            wake_event_loop();
        }
    }
}

#[cfg(test)]
#[path = "tests/frame_wake_tests.rs"]
mod tests;
