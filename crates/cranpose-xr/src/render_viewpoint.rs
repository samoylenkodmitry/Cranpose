//! What one eye sees from.

use crate::{FieldOfView, Pose};

/// Where one eye's display looks from and how wide it sees. Jetpack XR's `RenderViewpoint`
/// state, which an app reads from [`XrFrame::left_eye`](crate::XrFrame::left_eye) and
/// [`XrFrame::right_eye`](crate::XrFrame::right_eye).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderViewpoint {
    /// The eye's place and turn in the app's room. Its forward axis is -z.
    pub pose: Pose,
    /// The edges of what the eye's display shows.
    pub field_of_view: FieldOfView,
}
