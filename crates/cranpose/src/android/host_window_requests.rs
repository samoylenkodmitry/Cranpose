use std::time::Instant;

use cranpose_ui::{Point, Size};

use super::overlay_window;
use crate::{android_host_window, app_launcher::AndroidOverlayWindowOptions};

#[derive(Clone, Copy)]
pub(super) struct PendingHostWindowSizeRequest {
    pub(super) state: Option<android_host_window::AndroidHostWindowState>,
    pub(super) requested: Size,
    pub(super) requested_at: Instant,
}

pub(super) fn dispatch_android_surface_size_request(
    app: &android_activity::AndroidApp,
    requested: Size,
    position: Point,
    density: f32,
    overlay_options: Option<AndroidOverlayWindowOptions>,
) -> Result<(), String> {
    let requested =
        android_host_window::validate_logical_size(requested).map_err(|error| error.to_string())?;
    if overlay_options.is_some() {
        return overlay_window::update_android_overlay_window_bounds(
            app, position, requested, density,
        );
    }

    let (width_px, height_px) =
        android_host_window::logical_to_physical_window_size(requested, density);
    super::activity_window_layout::set(app, width_px, height_px)
}

pub(super) fn dispatch_registered_android_surface_size_request(
    app: &android_activity::AndroidApp,
    host_window_registry: &android_host_window::AndroidHostWindowRegistry,
    density: f32,
    overlay_options: Option<AndroidOverlayWindowOptions>,
    last_dispatched: &mut Option<(android_host_window::AndroidHostWindowState, u64, u64)>,
    pending_confirmation: &mut Option<PendingHostWindowSizeRequest>,
) {
    let Some(request) =
        android_host_window::latest_android_host_window_request(host_window_registry)
    else {
        return;
    };
    let dispatch_key = (
        request.state,
        request.size_revision,
        if overlay_options.is_some() {
            request.position_revision
        } else {
            0
        },
    );
    if *last_dispatched == Some(dispatch_key) {
        return;
    }

    let position = overlay_options
        .filter(|_| request.position_revision == 0)
        .map_or(request.position, |options| {
            Point::new(options.x as f32, options.y as f32)
        });
    request.state.mark_pending(request.size);
    match dispatch_android_surface_size_request(
        app,
        request.size,
        position,
        density,
        overlay_options,
    ) {
        Ok(()) => {
            *last_dispatched = Some(dispatch_key);
            *pending_confirmation = Some(PendingHostWindowSizeRequest {
                state: Some(request.state),
                requested: request.size,
                requested_at: Instant::now(),
            });
            let target = if overlay_options.is_some() {
                "Android overlay surface"
            } else {
                "Android host-window"
            };
            if overlay_options.is_some() {
                log::info!(
                    "Requested {target} bounds {:.1}x{:.1} dp at {:.1},{:.1} dp",
                    request.size.width,
                    request.size.height,
                    position.x,
                    position.y
                );
            } else {
                log::info!(
                    "Requested {target} size {:.1}x{:.1} dp",
                    request.size.width,
                    request.size.height
                );
            }
        }
        Err(message) => {
            *last_dispatched = Some(dispatch_key);
            request.state.mark_dispatch_failed(request.size, message);
        }
    }
}

pub(super) fn confirm_android_host_window_request(
    pending_confirmation: &mut Option<PendingHostWindowSizeRequest>,
    actual_size: Size,
) {
    let Some(pending) = *pending_confirmation else {
        return;
    };

    if android_host_window::sizes_match(pending.requested, actual_size) {
        if let Some(state) = pending.state {
            state.mark_applied(pending.requested, actual_size);
        }
        *pending_confirmation = None;
        return;
    }

    if pending.requested_at.elapsed() >= android_host_window::HOST_WINDOW_CONFIRMATION_TIMEOUT {
        if let Some(state) = pending.state {
            state.mark_unsupported(pending.requested, actual_size);
        }
        log::info!(
            "Android surface size request {:.1}x{:.1} dp was not honored; actual is {:.1}x{:.1} dp",
            pending.requested.width,
            pending.requested.height,
            actual_size.width,
            actual_size.height
        );
        *pending_confirmation = None;
    }
}
