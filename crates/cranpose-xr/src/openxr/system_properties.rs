//! The headset's facts that the runtime reports once.

use openxr as xr;

use super::{hand_actions::failed, refresh_rate, session::XrSession};
use crate::{EyeImageSize, HeadsetProperties};

const VIEW_CONFIGURATION: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;

// Asks the runtime for the headset's name, the size of the pictures that its eyes take and the
// refresh rates that it offers.
pub(crate) fn query(xr_session: &XrSession) -> Result<HeadsetProperties, String> {
    let instance = xr_session.instance();
    let system = instance
        .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
        .map_err(failed("system"))?;
    let system_name = instance.system_properties(system).map_or_else(
        |error| {
            log::warn!("cranpose xr: system properties: {error}");
            String::new()
        },
        |properties| properties.system_name,
    );
    let views = instance
        .enumerate_view_configuration_views(system, VIEW_CONFIGURATION)
        .map_err(failed("view configuration views"))?;
    let size = |eye: usize| {
        views
            .get(eye)
            .map_or_else(EyeImageSize::default, |view| EyeImageSize {
                recommended: [
                    view.recommended_image_rect_width,
                    view.recommended_image_rect_height,
                ],
                max: [view.max_image_rect_width, view.max_image_rect_height],
            })
    };
    let refresh_rates = refresh_rate::offered(instance, &xr_session.session);
    Ok(HeadsetProperties {
        system_name,
        eye_image_sizes: [size(0), size(1)],
        refresh_rates,
    })
}
