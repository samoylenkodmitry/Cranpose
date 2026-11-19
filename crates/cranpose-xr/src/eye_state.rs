//! The state of one eye in a frame.

use crate::{Pose, TrackingState};

/// One eye in one frame: where it is and which way it looks. Jetpack XR's `Eye.State`, without its
/// `isOpen`, which OpenXR's eye extensions do not give. The weights `EyesClosedLeft` and
/// `EyesClosedRight` of [`FaceState`](crate::FaceState) tell whether the eyes are shut.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EyeState {
    /// Whether the runtime tracks the eye.
    pub tracking_state: TrackingState,
    /// The eye's place in the app's room. It looks along its -z.
    pub pose: Pose,
    /// How far the runtime trusts the gaze, from 0 to 1, or 1 where the extension gives none.
    pub confidence: f32,
}
