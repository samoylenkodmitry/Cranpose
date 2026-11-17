//! What one eye sees from.

use crate::{FieldOfView, Pose};

/// Where one eye's display looks from and how wide it sees. Jetpack XR's `RenderViewpoint`
/// state, which an app reads from [`XrFrame::left_eye`](crate::XrFrame::left_eye) and
/// [`XrFrame::right_eye`](crate::XrFrame::right_eye).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderViewpoint {
    /// The eye's place and turn in the app's room. Its forward axis is -z. Jetpack XR's `pose`.
    pub pose: Pose,
    /// The eye's place and turn relative to the head, which is
    /// [`XrFrame::device_pose`](crate::XrFrame::device_pose). The distance between the two eyes'
    /// places is the interpupillary distance. Jetpack XR's `localPose`.
    pub local_pose: Pose,
    /// The edges of what the eye's display shows.
    pub field_of_view: FieldOfView,
}

impl RenderViewpoint {
    #[cfg_attr(
        not(any(test, all(feature = "openxr", target_os = "android"))),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // The viewpoint at `pose` in the room, seen from `head`, which is also a pose in the room.
    pub(crate) fn seen_from(head: &Pose, pose: Pose, field_of_view: FieldOfView) -> Self {
        Self {
            local_pose: head.inverse().then(&pose),
            pose,
            field_of_view,
        }
    }
}

#[cfg(test)]
#[path = "tests/render_viewpoint_tests.rs"]
mod tests;
