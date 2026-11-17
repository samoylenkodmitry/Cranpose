//! The display refresh rate: the rates the runtime offers, and the app's request for the highest.

use openxr as xr;

use super::graphics::Api;

// The refresh rates in hertz that the runtime offers, or none while the runtime lacks
// `XR_FB_display_refresh_rate` or fails to list them.
pub(super) fn offered(instance: &xr::Instance, session: &xr::Session<Api>) -> Vec<f32> {
    if instance.exts().fb_display_refresh_rate.is_none() {
        return Vec::new();
    }
    session
        .enumerate_display_refresh_rates()
        .unwrap_or_else(|error| {
            log::warn!("cranpose xr: refresh rates: {error}");
            Vec::new()
        })
}

// Asks for the highest rate the runtime offers and logs the rates, the current one and the
// request. The runtime may decline, and it answers later with an event that the session's event
// loop logs. Panels draw only when their content changes, so a frame costs little at any rate:
// the runtime shows the last image of a layer again.
pub(super) fn request_highest(instance: &xr::Instance, session: &xr::Session<Api>) {
    let rates = offered(instance, session);
    let Some(highest) = rates.iter().copied().max_by(f32::total_cmp) else {
        log::info!("cranpose xr: the runtime offers no refresh rate to choose");
        return;
    };
    let current = session.get_display_refresh_rate().ok();
    log::info!(
        "cranpose xr: the display offers {rates:?} Hz and runs at {current:?} Hz; the app asks for {highest} Hz"
    );
    if let Err(error) = session.request_display_refresh_rate(highest) {
        log::warn!("cranpose xr: refresh rate request: {error}");
    }
}
