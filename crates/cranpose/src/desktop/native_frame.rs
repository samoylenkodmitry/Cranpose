use std::{rc::Rc, time::Instant};

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;

use super::{
    App, DesktopGpuContext, NativeWindowSurface, WindowPresent, log_desktop_frame_telemetry,
    native_surface, native_window_redraw_held_while_hidden, native_window_timing_enabled,
    native_window_trace_enabled, note_native_window_presented, pace_after_empty_redraw,
    present_thread::log_presented_frame, presented_frame_anchor, print_native_window_timing,
    print_native_window_trace, should_chain_no_vsync_redraw, surface_reconfigure_requires_redraw,
    update_app_with_native_window_registry,
};
use crate::{
    native_window::{self, NativeWindowOptions},
    wgpu_surface::{
        PresentStep, SurfaceFrame, current_surface_texture,
        present_initial_placeholder_frame_cleared_to, surface_present_step,
    },
};

// Configures a native window's `surface` with `config` on the loop's
// thread and shows the window's background on it until its first frame.
pub(super) fn show_native_surface_cleared(
    context: &DesktopGpuContext,
    surface: &wgpu::Surface<'static>,
    config: &wgpu::SurfaceConfiguration,
    options: &NativeWindowOptions,
) {
    surface.configure(&context.gpu.device, config);
    let placeholder_presented = present_initial_placeholder_frame_cleared_to(
        surface,
        &context.gpu.device,
        &context.gpu.queue,
        config.format,
        "native window initial present",
        cranpose_render_wgpu::frame_clear_color(options.transparent),
    );
    trace_native_window_timing!(
        "{} placeholder_presented={placeholder_presented}",
        options.title
    );
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "native window frames in their module"
)]
impl App {
    // Updates `app` and draws the frame of `native` it owes, on the loop's
    // thread or on the window's present thread. True when a frame went.
    pub(super) fn redraw_native_window(
        app: &mut AppShell<WgpuRenderer>,
        native: &mut NativeWindowSurface,
        registry: &Rc<native_window::NativeWindowRegistry>,
    ) -> bool {
        if native_window_redraw_held_while_hidden(native.presentable()) {
            return false;
        }
        Self::drain_native_present_returns(app, native);
        if !native_surface(app, native)
            .is_some_and(|mut surface| surface.renderer().has_frame_credit())
        {
            return false;
        }
        let frame_started_at = Instant::now();
        update_app_with_native_window_registry(app, registry);
        let after_update = Instant::now();
        let Some(mut surface) = native_surface(app, native) else {
            return false;
        };
        let frame_owed = surface.take_frame_owed();
        let present = surface_present_step(native.surface_dirty, frame_owed, &mut surface);
        pace_after_empty_redraw(
            &mut native.last_frame_start_time,
            &mut native.last_redraw_empty,
            frame_started_at,
            present == PresentStep::Present,
        );
        if present != PresentStep::Present {
            return false;
        }
        let times = (frame_started_at, after_update);
        if matches!(native.present, WindowPresent::OnPresentThread(_)) {
            // Wayland asks for the frame callback before the surface commits.
            native.window.pre_present_notify();
            let published =
                native
                    .present
                    .publish(surface.renderer(), &native.surface_config, times);
            if published {
                native.surface_dirty = false;
                native.last_frame_start_time = Some(frame_started_at);
            }
            return published;
        }
        native.surface_dirty = true;
        Self::present_native_frame_on_loop(app, native, times)
    }

    // Takes the next image of the surface `native` holds, draws the frame
    // into it and presents it. The frame started at `times.0` and its
    // update ended at `times.1`.
    fn present_native_frame_on_loop(
        app: &mut AppShell<WgpuRenderer>,
        native: &mut NativeWindowSurface,
        (frame_started_at, after_update): (Instant, Instant),
    ) -> bool {
        let WindowPresent::OnLoop(loop_surface) = &native.present else {
            return false;
        };
        let acquire_started_at = Instant::now();
        let output = match current_surface_texture(loop_surface, "native window") {
            SurfaceFrame::Ready(output) => output,
            SurfaceFrame::Reconfigure => {
                trace_native_window!("redraw surface outdated key={:?}", native.key);
                let size = native.window.surface_size();
                Self::resize_native_surface(app, native, size.width, size.height);
                if surface_reconfigure_requires_redraw(size.width, size.height) {
                    native.window.request_redraw();
                }
                return false;
            }
            SurfaceFrame::Skip => {
                trace_native_window!("redraw surface unavailable key={:?}", native.key);
                pace_after_empty_redraw(
                    &mut native.last_frame_start_time,
                    &mut native.last_redraw_empty,
                    frame_started_at,
                    false,
                );
                return false;
            }
        };
        let after_acquire = Instant::now();

        let Some(mut surface) = native_surface(app, native) else {
            return false;
        };
        let config = &native.surface_config;
        if let Err(error) = crate::wgpu_surface::render_surface_frame(
            surface.renderer(),
            &output,
            config.format,
            (config.width, config.height),
        ) {
            log::error!("native window render failed: {error:?}");
            return false;
        }
        let after_render = Instant::now();

        native.window.pre_present_notify();
        surface.renderer().present(output);
        note_native_window_presented(native.state, true);
        let after_present = Instant::now();
        trace_native_window_timing!(
            "{} presented {}x{} key={:?}",
            native.options.title,
            native.surface_config.width,
            native.surface_config.height,
            native.key
        );
        native.surface_dirty = false;
        app.record_presented_frame(frame_started_at, after_render);
        log_desktop_frame_telemetry(
            frame_started_at,
            after_update,
            after_acquire,
            after_render,
            after_present,
            "native",
        );
        native.last_frame_start_time = Some(presented_frame_anchor(
            frame_started_at,
            native.frame_interval(app.frame_pacing_mode()),
            after_acquire.duration_since(acquire_started_at),
        ));
        let needs_frame =
            native_surface(app, native).is_some_and(|surface| surface.frame_schedule().needs_frame);
        if should_chain_no_vsync_redraw(native.frame_interval(app.frame_pacing_mode()), needs_frame)
        {
            native.window.request_redraw();
        }
        true
    }

    // Folds the frames the present thread of `native` handed back into its
    // renderer, which frees their room. A frame the thread refused leaves
    // the surface owing one.
    pub(super) fn drain_native_present_returns(
        app: &mut AppShell<WgpuRenderer>,
        native: &mut NativeWindowSurface,
    ) {
        if !matches!(native.present, WindowPresent::OnPresentThread(_)) {
            return;
        }
        let Some(mut surface) = native_surface(app, native) else {
            return;
        };
        let returns = native.present.drain(surface.renderer());
        native.surface_dirty |= returns.redraw || returns.unavailable;
        if let Some(frame) = returns.presented {
            log_presented_frame(&frame, "native");
            note_native_window_presented(native.state, true);
            app.record_presented_frame(frame.started_at, frame.after_render);
        }
    }
}
