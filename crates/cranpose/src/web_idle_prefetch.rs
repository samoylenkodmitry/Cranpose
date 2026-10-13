//! Composes, between a web app's frames, the lazy list items its frames left
//! beyond their viewports ([`AppShell::run_idle_prefetch`]), as Android does
//! while it waits for a frame. `requestIdleCallback` says how long the
//! browser's idle period lasts. A browser without it (Safari) runs the passes
//! from a zero-delay timeout after a frame, within the frame's interval.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    time::Duration,
};

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use wasm_bindgen::{JsCast, closure::Closure};
use web_time::Instant;

/// Where the browser has no idle callbacks: how long after a frame's start
/// the passes may run, a 60 Hz interval less a margin for the next frame.
const TIMEOUT_IDLE_BUDGET: Duration = Duration::from_millis(12);

pub(crate) struct IdlePrefetch {
    app: Weak<RefCell<AppShell<WgpuRenderer>>>,
    queued: Cell<bool>,
}

impl IdlePrefetch {
    pub(crate) fn new(app: &Rc<RefCell<AppShell<WgpuRenderer>>>) -> Rc<Self> {
        Rc::new(Self {
            app: Rc::downgrade(app),
            queued: Cell::new(false),
        })
    }

    /// Asks for an idle period after the frame that started at
    /// `frame_started`, when a lazy list waits for one. One is asked at a
    /// time.
    pub(crate) fn queue_after_frame(self: &Rc<Self>, frame_started: Instant) {
        if self.queued.get() || !self.has_work() {
            return;
        }
        let Some(window) = crate::web_frame_host::window() else {
            return;
        };
        self.queued.set(true);
        if self.ask_idle_period(&window) {
            return;
        }
        let this = Rc::clone(self);
        let timeout = Closure::once_into_js(move || {
            this.run(frame_started + TIMEOUT_IDLE_BUDGET);
        });
        if window
            .set_timeout_with_callback_and_timeout_and_arguments_0(timeout.unchecked_ref(), 0)
            .is_err()
        {
            self.queued.set(false);
        }
    }

    /// Asks the browser for its next idle period. `false` where it has no
    /// idle callbacks.
    fn ask_idle_period(self: &Rc<Self>, window: &web_sys::Window) -> bool {
        let this = Rc::clone(self);
        let idle = Closure::once_into_js(move |deadline: web_sys::IdleDeadline| {
            let remaining = Duration::from_secs_f64(deadline.time_remaining().max(0.0) / 1000.0);
            this.run(Instant::now() + remaining);
            // A list still waiting gets the next idle period; a timeout
            // runs only after a frame, whose start bounds it.
            if this.has_work()
                && !this.queued.replace(true)
                && let Some(window) = crate::web_frame_host::window()
                && !this.ask_idle_period(&window)
            {
                this.queued.set(false);
            }
        });
        window.request_idle_callback(idle.unchecked_ref()).is_ok()
    }

    fn run(&self, deadline: Instant) {
        self.queued.set(false);
        if let Some(app) = self.app.upgrade()
            && let Ok(mut app) = app.try_borrow_mut()
        {
            app.run_idle_prefetch(Some(deadline), |_| false);
        }
    }

    fn has_work(&self) -> bool {
        self.app.upgrade().is_some_and(|app| {
            app.try_borrow()
                .is_ok_and(|app| app.has_idle_prefetch_work())
        })
    }
}
