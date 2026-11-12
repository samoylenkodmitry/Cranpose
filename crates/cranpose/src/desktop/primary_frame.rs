use std::{
    ops::ControlFlow,
    rc::Rc,
    time::{Duration, Instant},
};

use winit::event_loop::ActiveEventLoop;

use super::{
    App, PrimaryPresent, PrimarySurface, SurfaceFrame, current_surface_texture,
    headless_requested_viewport, log_desktop_frame_telemetry, native_window_trace_enabled,
    next_frame_anchor, pace_after_empty_redraw, presented_frame_anchor,
    primary_declaration_host_needs_direct_update, print_native_window_trace, record_pacing_event,
    request_redraw_once, should_chain_no_vsync_redraw, surface_reconfigure_requires_redraw,
    update_declaration_host_frame,
};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the primary frame in its module"
)]
impl App {
    // Takes the primary window's next image, draws the frame into it and
    // presents it, or hands the frame to the present thread to do that.
    // The frame started at `frame_started_at` and its update ended at
    // `after_update`. `Break` when the redraw ends without the work that
    // follows a frame: the surface needs a new configuration, gives no
    // image or the draw failed.
    pub(super) fn present_primary_frame(
        &mut self,
        (frame_started_at, after_update): (Instant, Instant),
        frame_interval: Option<Duration>,
    ) -> ControlFlow<()> {
        let viewport_override = headless_requested_viewport(&self.settings);
        let (Some(app), Some(window), Some(present), Some(config)) = (
            self.app.as_mut(),
            self.window.as_ref(),
            self.surface.as_ref(),
            self.surface_config.as_mut(),
        ) else {
            return ControlFlow::Break(());
        };
        let PrimaryPresent::OnLoop(surface) = present else {
            return self.publish_primary_frame(frame_started_at, frame_interval);
        };
        let mut primary = PrimarySurface {
            window,
            present,
            config,
            viewport_override,
        };
        self.primary_surface_dirty = true;
        let acquire_started_at = Instant::now();
        let output = match current_surface_texture(surface, "primary window") {
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
        if !self.robot.drives()
            && should_chain_no_vsync_redraw(frame_interval, app.frame_schedule().needs_frame)
        {
            request_redraw_once(window, &mut self.primary_redraw_pending);
        }
        self.primary_frame_shown();
        ControlFlow::Continue(())
    }

    // What follows a frame of the primary window that reached the screen.
    pub(super) fn primary_frame_shown(&mut self) {
        #[cfg(target_os = "macos")]
        {
            self.application_menu.frame_presented();
            if let Some(window) = &self.window {
                super::reveal_staged_primary(
                    &mut self.primary_staged,
                    self.primary_shown
                        .load(std::sync::atomic::Ordering::Relaxed),
                    window.as_ref(),
                );
            }
        }
        self.device_loss.presented();
        self.robot.note_presented();
    }

    pub(super) fn handle_primary_frame_requested(&mut self, event_loop: &dyn ActiveEventLoop) {
        if !self.primary_frames_enabled() {
            return;
        }
        let registry = Rc::clone(&self.native_window_registry);
        let frame_interval = self.frame_interval();
        let waiting_for_frame_cap = self
            .last_frame_start_time
            .and_then(|started_at| frame_interval.map(|interval| started_at + interval))
            .is_some_and(|deadline| deadline > Instant::now());
        let primary_visible = self.primary_visible();
        let direct_declaration_update = {
            let Some(app) = &mut self.app else {
                return;
            };
            let needs_redraw = app.frame_schedule().needs_frame;
            if !needs_redraw {
                return;
            }
            let direct_declaration_update = primary_declaration_host_needs_direct_update(
                primary_visible,
                self.settings.headless,
                needs_redraw,
                waiting_for_frame_cap,
            );
            if direct_declaration_update {
                trace_native_window!(
                    "primary declaration host proxy update visible={} headless={}",
                    primary_visible,
                    self.settings.headless
                );
                let update_result = update_declaration_host_frame(
                    app,
                    &registry,
                    &mut self.last_frame_start_time,
                    frame_interval,
                );
                self.robot.record_idle_update_result(update_result);
            }
            direct_declaration_update
        };

        if direct_declaration_update {
            self.sync_native_windows(event_loop);
        } else if waiting_for_frame_cap {
        } else if let Some(window) = &self.window {
            request_redraw_once(window, &mut self.primary_redraw_pending);
        }
    }
}
