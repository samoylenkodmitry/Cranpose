// Runs a web app's work in the browser's idle periods: between frames, the
// lazy list items its frames left beyond their viewports
// (`AppShell::run_idle_prefetch`), as Android composes them while it waits
// for a frame, and while the page asks for no frame, the pipeline builds that
// wait for one (`Renderer::build_while_idle`). `requestIdleCallback` says how
// long the browser's idle period lasts. A browser without it (Safari) runs
// the work from a zero-delay timeout after a frame, within the frame's
// interval, and looks again for builds from a timeout.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    time::Duration,
};

use cranpose_app_shell::{AppShell, FrameSchedule};
use cranpose_render_common::Renderer;
use cranpose_render_wgpu::WgpuRenderer;
use wasm_bindgen::{JsCast, closure::Closure};
use web_time::Instant;

// Where the browser has no idle callbacks: how long after a frame's start
// the passes may run, a 60 Hz interval less a margin for the next frame.
const TIMEOUT_IDLE_BUDGET: Duration = Duration::from_millis(12);

// Where the browser has no idle callbacks: when a build that waits for the
// GPU process to link the last one looks again, as Chrome's longest idle
// period comes again.
const TIMEOUT_BUILD_POLL_MS: i32 = 50;

pub(crate) struct IdleWork {
    app: Weak<RefCell<AppShell<WgpuRenderer>>>,
    // Whether the page has asked for its next frame.
    frame_pending: Rc<Cell<bool>>,
    // Whether the app asked for nothing at its last frame's end: no frame,
    // no update and no timed event, so no animation waits behind a build.
    stands_still: Cell<bool>,
    queued: Cell<bool>,
}

impl IdleWork {
    pub(crate) fn new(
        app: &Rc<RefCell<AppShell<WgpuRenderer>>>,
        frame_pending: &Rc<Cell<bool>>,
    ) -> Rc<Self> {
        Rc::new(Self {
            app: Rc::downgrade(app),
            frame_pending: Rc::clone(frame_pending),
            stands_still: Cell::new(false),
            queued: Cell::new(false),
        })
    }

    // Asks for an idle period after the frame that started at
    // `frame_started`, when a lazy list waits for one or the app's
    // `schedule` asks for nothing, which leaves builds no frame to run at.
    // One is asked at a time.
    pub(crate) fn queue_after_frame(
        self: &Rc<Self>,
        frame_started: Instant,
        schedule: FrameSchedule,
    ) {
        self.stands_still.set(schedule.is_idle());
        if self.queued.get() || !(self.stands_still.get() || self.has_prefetch_work()) {
            return;
        }
        let Some(window) = super::frame_host::window() else {
            return;
        };
        self.queued.set(true);
        if !self.ask_idle_period(&window) {
            self.ask_timeout(&window, 0, Some(frame_started));
        }
    }

    // Asks the browser for its next idle period. `false` where it has no
    // idle callbacks.
    fn ask_idle_period(self: &Rc<Self>, window: &web_sys::Window) -> bool {
        let this = Rc::clone(self);
        let idle = Closure::once_into_js(move |deadline: web_sys::IdleDeadline| {
            let remaining = Duration::from_secs_f64(deadline.time_remaining().max(0.0) / 1000.0);
            let builds_wait = this.run(Instant::now() + remaining);
            // A list or a build still waiting gets the next idle period; a
            // timeout runs passes only after a frame, whose start bounds it.
            if (builds_wait || this.has_prefetch_work())
                && !this.queued.replace(true)
                && let Some(window) = super::frame_host::window()
                && !this.ask_idle_period(&window)
            {
                this.queued.set(false);
            }
        });
        window.request_idle_callback(idle.unchecked_ref()).is_ok()
    }

    // Runs the work from a timeout `delay_ms` from now, the passes until
    // 12 ms after the frame that started at `frame_started`, or with no
    // frame, 12 ms after the timeout fires.
    fn ask_timeout(
        self: &Rc<Self>,
        window: &web_sys::Window,
        delay_ms: i32,
        frame_started: Option<Instant>,
    ) {
        let this = Rc::clone(self);
        let timeout = Closure::once_into_js(move || {
            let started = frame_started.unwrap_or_else(Instant::now);
            if this.run(started + TIMEOUT_IDLE_BUDGET)
                && !this.queued.replace(true)
                && let Some(window) = super::frame_host::window()
            {
                this.ask_timeout(&window, TIMEOUT_BUILD_POLL_MS, None);
            }
        });
        if window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                timeout.unchecked_ref(),
                delay_ms,
            )
            .is_err()
        {
            self.queued.set(false);
        }
    }

    // Runs the passes that fit before `deadline` and, while the app stands
    // still, a build: whether builds still wait.
    fn run(&self, deadline: Instant) -> bool {
        self.queued.set(false);
        let Some(app) = self.app.upgrade() else {
            return false;
        };
        let Ok(mut app) = app.try_borrow_mut() else {
            return false;
        };
        app.run_idle_prefetch(Some(deadline), |_| false);
        // A frame asked for since runs first, and its end asks again.
        let builds_wait = self.stands_still.get()
            && !self.frame_pending.get()
            && app.renderer().build_while_idle();
        self.stands_still.set(builds_wait);
        builds_wait
    }

    fn has_prefetch_work(&self) -> bool {
        self.app.upgrade().is_some_and(|app| {
            app.try_borrow()
                .is_ok_and(|app| app.has_idle_prefetch_work())
        })
    }
}
