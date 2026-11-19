//! The eyes' gaze in a frame.

use crate::{EyeState, Pose};

/// Where the eyes look in one frame. The two eyes come from `XR_FB_eye_tracking_social`. The
/// combined gaze comes from `XR_EXT_eye_gaze_interaction`, which an Android XR headset offers for
/// the user's selection.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Eyes {
    /// The left eye.
    pub left: EyeState,
    /// The right eye.
    pub right: EyeState,
    /// The pose that points from between the eyes at what the user looks at, in the app's room, or
    /// `None` while the runtime gives no gaze interaction. It points along its -z.
    pub gaze: Option<Pose>,
}
