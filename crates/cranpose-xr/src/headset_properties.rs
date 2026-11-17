//! What the headset is, which does not change during a session.

use cranpose_core::CompositionLocal;

use crate::locals::with_locals;

/// The size of the pictures that one eye's display takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EyeImageSize {
    /// The size that the runtime recommends, as width and height in pixels. It is the size at
    /// which the compositor's own sampling keeps a picture sharp and the GPU keeps its pace.
    pub recommended: [u32; 2],
    /// The largest size that the runtime accepts, as width and height in pixels.
    pub max: [u32; 2],
}

/// The facts of the headset that the runtime reports once for the session: its name, the size of
/// the pictures that its displays take and the refresh rates that it offers. An app reads them
/// through [`local_headset_properties`], for example to tell a host the size of the pictures to
/// render.
///
/// Jetpack XR has no equivalent. The fields come from OpenXR's `XrSystemProperties`,
/// `xrEnumerateViewConfigurationViews` and `XR_FB_display_refresh_rate`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeadsetProperties {
    /// The runtime's name for the headset's system, such as "Meta Quest 3".
    pub system_name: String,
    /// The size of the pictures for the left eye's display and for the right eye's.
    pub eye_image_sizes: [EyeImageSize; 2],
    /// The refresh rates in hertz that the runtime offers, or an empty list on a runtime without
    /// `XR_FB_display_refresh_rate`. [`XrFrame::refresh_rate`](crate::XrFrame::refresh_rate) gives
    /// the rate that the display runs at in a frame.
    pub refresh_rates: Vec<f32>,
}

/// The composition local that holds the [`HeadsetProperties`], or `None` outside a headset. The
/// value does not change during a session.
///
/// ```ignore
/// if let Some(properties) = local_headset_properties().current() {
///     tell_host(properties.eye_image_sizes[0].recommended);
/// }
/// ```
pub fn local_headset_properties() -> CompositionLocal<Option<HeadsetProperties>> {
    with_locals(|locals| locals.headset_properties.clone())
}
