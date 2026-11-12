use std::time::Instant;

use cranpose_app_shell::AppShell;
use cranpose_core::collections::map::HashMap;
use cranpose_render_wgpu::WgpuRenderer;
use winit::window::WindowId as WinitWindowId;

use super::{
    NATIVE_WINDOW_GLOBAL_POINTER_POLLED, NativeWindowSurface, native_surface,
    native_surface_needs_frame, native_window_drag_poll_deadline,
    native_window_position_poll_needed,
};

// When the loop wakes for the native windows: to poll a drag, to poll their
// positions, at the earliest frame cap and at the earliest timed event.
pub(super) struct NativeWindowWakes {
    pub(super) drag_poll: Option<Instant>,
    pub(super) position_poll: Option<Instant>,
    pub(super) frame_cap: Option<Instant>,
    pub(super) next_event: Option<Instant>,
}

// Asks each native window of `app` that owes a frame and waits for no frame
// cap at `now` for a redraw, and gives when the loop wakes for them next;
// their positions are polled next at `position_poll_at`.
pub(super) fn native_window_wakes(
    app: &mut AppShell<WgpuRenderer>,
    native_windows: &mut HashMap<WinitWindowId, NativeWindowSurface>,
    position_poll_at: Instant,
    now: Instant,
) -> NativeWindowWakes {
    let mut native_drag_deadline: Option<Instant> = None;
    let native_position_poll_deadline = native_windows
        .values()
        .any(|native| {
            native_window_position_poll_needed(
                native.options.visible,
                native.active_drag.is_some(),
                native.pending_outer_positions.has_pending(),
            )
        })
        .then_some(position_poll_at);
    let mut native_frame_cap_deadline: Option<Instant> = None;
    let mut native_next_event_time: Option<Instant> = None;
    let pacing_mode = app.frame_pacing_mode();
    for native in native_windows.values_mut() {
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
        }
        if let Some(next_poll_at) = native.active_drag.and_then(|active_drag| {
            native_window_drag_poll_deadline(
                active_drag.next_poll_at,
                NATIVE_WINDOW_GLOBAL_POINTER_POLLED,
            )
        }) {
            native_drag_deadline = Some(
                native_drag_deadline.map_or(next_poll_at, |current| current.min(next_poll_at)),
            );
        }
        if waiting_for_frame_cap && let Some(deadline) = next_frame_time {
            native_frame_cap_deadline =
                Some(native_frame_cap_deadline.map_or(deadline, |current| current.min(deadline)));
        }
        if let Some(next_time) = frame_schedule.next_deadline {
            native_next_event_time =
                Some(native_next_event_time.map_or(next_time, |current| current.min(next_time)));
        }
    }
    NativeWindowWakes {
        drag_poll: native_drag_deadline,
        position_poll: native_position_poll_deadline,
        frame_cap: native_frame_cap_deadline,
        next_event: native_next_event_time,
    }
}
