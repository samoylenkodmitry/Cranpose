// How long the desktop event loop waits before it looks again: for a frame
// the pacing caps, a poll, a timed event or nothing; and the lazy list items
// it composes ahead while it waits.

use std::time::{Duration, Instant};

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use winit::event_loop::ControlFlow;

use super::{
    NATIVE_WINDOW_GLOBAL_POINTER_POLLED, NativeWindowSurface, native_surface,
    native_surface_needs_frame, native_window_drag_poll_deadline,
};

pub(super) struct LoopControlInputs {
    pub(super) robot_needs_poll: bool,
    pub(super) free_running: bool,
    // Whether a window asked for a redraw the platform delivers next.
    pub(super) redraw_requested: bool,
    pub(super) primary_pointer_polled: bool,
    pub(super) drag_poll_deadline: Option<Instant>,
    pub(super) position_poll_deadline: Option<Instant>,
    pub(super) frame_cap_deadline: Option<Instant>,
    pub(super) has_active_animations: bool,
    pub(super) next_event_time: Option<Instant>,
}

pub(super) fn event_loop_control_flow(now: Instant, inputs: LoopControlInputs) -> ControlFlow {
    let due = |deadline: Option<Instant>| deadline.is_some_and(|deadline| deadline <= now);
    if inputs.robot_needs_poll
        || inputs.free_running
        || inputs.primary_pointer_polled
        || due(inputs.drag_poll_deadline)
        || due(inputs.position_poll_deadline)
    {
        return ControlFlow::Poll;
    }
    let deadline = [
        inputs.frame_cap_deadline,
        inputs.position_poll_deadline,
        inputs.drag_poll_deadline,
    ]
    .into_iter()
    .flatten()
    .min();
    match deadline {
        Some(deadline) => ControlFlow::WaitUntil(deadline),
        None if inputs.has_active_animations => ControlFlow::Poll,
        None => inputs
            .next_event_time
            .map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
    }
}

impl LoopControlInputs {
    // Composes lazy list items that frames left beyond their viewports
    // (`AppShell::run_idle_prefetch`) while the loop has nothing due before
    // it waits: until the frame the pacing caps starts, or while no frame
    // waits, for one vsync interval, and before the next timed event or
    // poll. A frame due now, asked for or free running, starts as soon as
    // the loop waits, so it runs none then. While passes ran and items are
    // left, the loop looks again at once, after the events that came
    // meanwhile; while no pass fits, it waits as it would have.
    pub(super) fn prefetch_idle(
        mut self,
        app: &mut AppShell<WgpuRenderer>,
        now: Instant,
        vsync_interval: Duration,
    ) -> Self {
        if self.free_running || self.redraw_requested || !app.has_idle_prefetch_work() {
            return self;
        }
        let until = [
            self.next_event_time,
            self.drag_poll_deadline,
            self.position_poll_deadline,
        ]
        .into_iter()
        .flatten()
        .fold(
            self.frame_cap_deadline.unwrap_or(now + vsync_interval),
            Instant::min,
        );
        if crate::idle_prefetch::prefetch_until(app, now, until) {
            self.next_event_time = Some(now);
        }
        self
    }
}

// What the presentable native windows wait for before the loop looks again.
pub(super) struct NativeWaits {
    pub(super) drag_poll: Option<Instant>,
    pub(super) frame_cap: Option<Instant>,
    pub(super) next_event: Option<Instant>,
    pub(super) redraw_requested: bool,
}

impl NativeWaits {
    // Asks each presentable native window whose frame is due for a redraw,
    // and gathers what the others wait for.
    pub(super) fn request_frames<'a>(
        app: &mut AppShell<WgpuRenderer>,
        native_windows: impl Iterator<Item = &'a mut NativeWindowSurface>,
        now: Instant,
    ) -> Self {
        let mut waits = Self {
            drag_poll: None,
            frame_cap: None,
            next_event: None,
            redraw_requested: false,
        };
        let pacing_mode = app.frame_pacing_mode();
        for native in native_windows {
            if !native.presentable() {
                continue;
            }
            let Some(surface) = native_surface(app, native) else {
                continue;
            };
            let frame_schedule = surface.frame_schedule();
            let needs_redraw = native_surface_needs_frame(
                frame_schedule.needs_frame,
                surface.frame_owed(),
                surface.needs_redraw(),
                native.surface_dirty,
            );
            let next_frame_time = native.last_frame_start_time.and_then(|started_at| {
                native
                    .frame_interval(pacing_mode)
                    .map(|interval| started_at + interval)
            });
            let waiting_for_frame_cap =
                needs_redraw && next_frame_time.is_some_and(|deadline| deadline > now);
            if needs_redraw && !waiting_for_frame_cap {
                native.window.request_redraw();
                waits.redraw_requested = true;
            }
            let drag_poll = native.active_drag.and_then(|active_drag| {
                native_window_drag_poll_deadline(
                    active_drag.next_poll_at,
                    NATIVE_WINDOW_GLOBAL_POINTER_POLLED,
                )
            });
            waits.drag_poll = earliest(waits.drag_poll, drag_poll);
            let frame_cap = next_frame_time.filter(|_| waiting_for_frame_cap);
            waits.frame_cap = earliest(waits.frame_cap, frame_cap);
            waits.next_event = earliest(waits.next_event, frame_schedule.next_deadline);
        }
        waits
    }
}

fn earliest(first: Option<Instant>, second: Option<Instant>) -> Option<Instant> {
    first.into_iter().chain(second).min()
}

#[cfg(test)]
#[path = "tests/loop_control_tests.rs"]
mod tests;
