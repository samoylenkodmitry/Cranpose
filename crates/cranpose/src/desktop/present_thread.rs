use std::{
    ops::ControlFlow,
    time::{Duration, Instant},
};

use winit::{event::WindowEvent, window::WindowId as WinitWindowId};

use super::{
    App, log_desktop_frame_telemetry, native_surface, next_frame_anchor, record_pacing_event,
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
        let WindowEvent::Occluded(occluded) = *event else {
            return;
        };
        let primary = self.primary_window_id() == Some(window_id);
        let Some(app) = self.app.as_mut() else {
            return;
        };
        if primary {
            if let Some(thread) = app.renderer().present_thread() {
                thread.set_occluded(occluded);
            }
        } else if let Some(mut surface) = self
            .native_windows
            .get(&window_id)
            .and_then(|native| native_surface(app, native))
            && let Some(thread) = surface.renderer().present_thread()
        {
            thread.set_occluded(occluded);
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
        let (Some(app), Some(present @ WindowPresent::OnPresentThread(_))) =
            (self.app.as_mut(), self.surface.as_mut())
        else {
            return;
        };
        let returns = present.drain(app.renderer());
        if returns.redraw {
            self.primary_surface_dirty = true;
            app.mark_dirty();
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
