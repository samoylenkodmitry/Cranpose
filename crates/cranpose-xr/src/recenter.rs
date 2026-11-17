//! The runtime's change of the room's origin.

use std::time::Duration;

use crate::Pose;

/// The runtime moved the origin of the app's room, which the user asked for by recentering.
/// OpenXR's `ReferenceSpaceChangePending` event for the LOCAL space.
///
/// Every pose that the app gets for a time at or after [`change_time`](Self::change_time) is
/// relative to the new origin. A streaming app tells its host, so the host moves its own origin
/// the same way. [`XrFrame::recenter`](crate::XrFrame::recenter) holds it for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Recenter {
    /// When the new origin takes effect, on the clock of
    /// [`XrFrame::predicted_display_time`](crate::XrFrame::predicted_display_time). It may lie
    /// after the frame that reports the event.
    pub change_time: Duration,
    /// The new origin in the room as it was before, or `None` when the runtime gives no pose.
    pub pose_in_previous_space: Option<Pose>,
}

impl Recenter {
    // The change that two changes make together, when the runtime sends the second before the
    // app took the first: the second's previous room is the first's new one.
    #[cfg_attr(
        not(any(test, all(feature = "openxr", target_os = "android"))),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    pub(crate) fn then(self, later: Self) -> Self {
        Self {
            change_time: later.change_time,
            pose_in_previous_space: self
                .pose_in_previous_space
                .zip(later.pose_in_previous_space)
                .map(|(first, second)| first.then(&second)),
        }
    }
}

#[cfg(test)]
#[path = "tests/recenter_tests.rs"]
mod tests;
