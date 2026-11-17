//! The display refresh rate: the app asks the runtime for the highest one it offers.

use openxr as xr;

// Asks for the highest rate the runtime offers and logs the rates, the current one and the
// request. The runtime may decline, and it answers later with an event that the session's event
// loop logs. Panels draw only when their content changes, so a frame costs little at any rate:
// the runtime shows the last image of a layer again.
pub(super) fn request_highest(session: &xr::Session<xr::OpenGlEs>) {
    let rates = match session.enumerate_display_refresh_rates() {
        Ok(rates) => rates,
        Err(error) => {
            log::warn!("cranpose xr: refresh rates: {error}");
            return;
        }
    };
    let Some(highest) = rates.iter().copied().max_by(f32::total_cmp) else {
        log::warn!("cranpose xr: the runtime offers no refresh rate");
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
