use std::{
    ops::ControlFlow,
    time::{Duration, Instant},
};

use winit::{event::WindowEvent, window::WindowId as WinitWindowId};

use super::{
    App, log_desktop_frame_telemetry, native_surface, next_frame_anchor, pace_after_empty_redraw,
    record_pacing_event, request_redraw_once,
};
use crate::window_present::{PresentedFrame, WindowPresent};

// Logs the stages of `frame`, a frame of the window `label` names, when the
// frame telemetry asks for them.
pub(super) fn log_presented_frame(frame: &PresentedFrame, label: &str) {
    log_desktop_frame_telemetry(
        frame.started_at,
        frame.after_update,
        frame.after_acquire,
        frame.after_render,
        frame.after_present,
        label,
    );
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the present thread in its module"
)]
impl App {
    // Hands the primary window's frame to the present thread, which takes
    // the surface's next image, draws the frame into it and presents it. The
    // frame started at `times.0` and its update ended at `times.1`. `Break`
    // when no frame went.
    pub(super) fn publish_primary_frame(
        &mut self,
        times: (Instant, Instant),
        frame_interval: Option<Duration>,
    ) -> ControlFlow<()> {
        let (Some(app), Some(window), Some(present), Some(config)) = (
            self.app.as_mut(),
            self.window.as_ref(),
            self.surface.as_mut(),
            self.surface_config.as_ref(),
        ) else {
            return ControlFlow::Break(());
        };
        // Wayland asks for the frame callback before the surface commits.
        window.pre_present_notify();
        if !present.publish(app.renderer(), config, times) {
            return ControlFlow::Break(());
        }
        // The present thread holds the frame of a window the window server
        // shows none of until it shows the window. The next frame waits a
        // frame interval, as after a loop's surface gave no image, or the
        // loop retries at once and spins a core.
        if present.occluded() {
            pace_after_empty_redraw(
                &mut self.last_frame_start_time,
                &mut self.last_redraw_empty,
                times.0,
                false,
            );
            self.robot.note_unpresentable();
            return ControlFlow::Break(());
        }
        self.primary_surface_dirty = false;
        self.primary_initial_present_pending = false;
        self.last_frame_start_time = Some(next_frame_anchor(
            self.last_frame_start_time,
            times.0,
            frame_interval,
        ));
        ControlFlow::Continue(())
    }

    // Tells the present thread of the window `window_id` when an `event`
    // says whether the window server shows any of the window: AppKit
    // answers that on the main thread only.
    pub(super) fn hand_over_occlusion(&mut self, window_id: WinitWindowId, event: &WindowEvent) {
        if !matches!(event, WindowEvent::Occluded(_)) {
            return;
        }
        let primary = self.primary_window_id() == Some(window_id);
        let Some(app) = self.app.as_mut() else {
            return;
        };
        if primary {
            if let Some(present) = self.surface.as_mut() {
                present.observe_occlusion(app.renderer(), event);
            }
        } else if let Some(native) = self.native_windows.get_mut(&window_id)
            && let Some(mut surface) = native_surface(app, native)
        {
            native.present.observe_occlusion(surface.renderer(), event);
        }
    }

    // Folds the frames each window's present thread handed back into its
    // renderer, which frees their room.
    pub(super) fn drain_present_returns(&mut self) {
        self.drain_primary_present_returns();
        let Some(app) = self.app.as_mut() else {
            return;
        };
        for native in self.native_windows.values_mut() {
            Self::drain_native_present_returns(app, native);
        }
    }

    // A frame of the primary window on the screen ends as one the loop
    // presented; after a frame the thread refused, the next frame draws.
    pub(super) fn drain_primary_present_returns(&mut self) {
        let (Some(app), Some(present @ WindowPresent::OnPresentThread { .. })) =
            (self.app.as_mut(), self.surface.as_mut())
        else {
            return;
        };
        let returns = present.drain(app.renderer());
        // The renderer lowers the next frame from the scene it holds: a
        // refused frame owes a frame, not a new scene.
        if returns.redraw {
            self.primary_surface_dirty = true;
            if let Some(window) = &self.window {
                request_redraw_once(window, &mut self.primary_redraw_pending);
            }
        }
        if returns.unavailable {
            self.primary_surface_dirty = true;
            self.robot.note_unpresentable();
        }
        let Some(frame) = returns.presented else {
            return;
        };
        log_presented_frame(&frame, "primary");
        record_pacing_event(|diag| &mut diag.presents);
        app.record_presented_frame(frame.started_at, frame.after_render);
        self.primary_frame_shown();
    }
}
