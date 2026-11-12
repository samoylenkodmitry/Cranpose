use std::{
    ops::ControlFlow,
    time::{Duration, Instant},
};

use super::{
    App, PrimarySurface, SurfaceFrame, current_surface_texture, headless_requested_viewport,
    log_desktop_frame_telemetry, next_frame_anchor, pace_after_empty_redraw,
    presented_frame_anchor, record_pacing_event, request_redraw_once, should_chain_no_vsync_redraw,
    surface_reconfigure_requires_redraw,
};

impl App {
    // Takes the primary window's next image, draws the frame into it and
    // presents it. The frame started at `frame_started_at` and its update
    // ended at `after_update`. `Break` when the redraw ends without the
    // work that follows a frame: the surface needs a new configuration,
    // gives no image or the draw failed.
    pub(super) fn present_primary_frame(
        &mut self,
        (frame_started_at, after_update): (Instant, Instant),
        frame_interval: Option<Duration>,
    ) -> ControlFlow<()> {
        let viewport_override = headless_requested_viewport(&self.settings);
        let (Some(app), Some(window), Some(surface), Some(config)) = (
            self.app.as_mut(),
            self.window.as_ref(),
            self.surface.as_ref(),
            self.surface_config.as_mut(),
        ) else {
            return ControlFlow::Break(());
        };
        let mut primary = PrimarySurface {
            window,
            surface,
            config,
            viewport_override,
        };
        self.primary_surface_dirty = true;
        let acquire_started_at = Instant::now();
        let output = match current_surface_texture(primary.surface, "primary window") {
            SurfaceFrame::Ready(output) => output,
            SurfaceFrame::Reconfigure => {
                let size = window.surface_size();
                primary.configure(app, size.width, size.height);
                if surface_reconfigure_requires_redraw(size.width, size.height) {
                    request_redraw_once(window, &mut self.primary_redraw_pending);
                }
                return ControlFlow::Break(());
            }
            SurfaceFrame::Skip => {
                // No texture: the window is minimized, occluded or
                // timed out. The next attempt waits a frame
                // interval, as after an empty redraw, or the loop
                // retries at once and spins a core.
                pace_after_empty_redraw(
                    &mut self.last_frame_start_time,
                    &mut self.last_redraw_empty,
                    frame_started_at,
                    false,
                );
                self.robot.note_unpresentable();
                return ControlFlow::Break(());
            }
        };
        let after_acquire = Instant::now();

        let config = &primary.config;
        if let Err(err) = crate::wgpu_surface::render_surface_frame(
            app.renderer(),
            &output,
            config.format,
            (config.width, config.height),
        ) {
            log::error!("render failed: {err:?}");
            return ControlFlow::Break(());
        }
        let after_render = Instant::now();

        window.pre_present_notify();
        app.renderer().present(output);
        let after_present = Instant::now();
        record_pacing_event(|diag| &mut diag.presents);
        self.primary_surface_dirty = false;
        self.primary_initial_present_pending = false;
        app.record_presented_frame(frame_started_at, after_render);
        log_desktop_frame_telemetry(
            frame_started_at,
            after_update,
            after_acquire,
            after_render,
            after_present,
            "primary",
        );
        self.last_frame_start_time = Some(presented_frame_anchor(
            next_frame_anchor(self.last_frame_start_time, frame_started_at, frame_interval),
            frame_interval,
            after_acquire.duration_since(acquire_started_at),
        ));
        #[cfg(target_os = "macos")]
        self.application_menu.frame_presented();
        #[cfg(target_os = "macos")]
        super::reveal_staged_primary(
            &mut self.primary_staged,
            self.primary_shown
                .load(std::sync::atomic::Ordering::Relaxed),
            window.as_ref(),
        );
        self.device_loss.presented();
        self.robot.note_presented();
        if !self.robot.drives()
            && should_chain_no_vsync_redraw(frame_interval, app.frame_schedule().needs_frame)
        {
            request_redraw_once(window, &mut self.primary_redraw_pending);
        }
        ControlFlow::Continue(())
    }
}
