//! The state of one tracked hand in a frame.

use crate::{HandAim, HandJoint, HandJointType, TrackingState};

/// The state of one hand that the cameras track, in one frame. Jetpack XR's `Hand.State`.
/// [`XrFrame::hand`](crate::XrFrame::hand) gives one for each [`Hand`](crate::Hand), which in
/// Jetpack XR is `HandSide`. A hand that no tracker covers stays at rest.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HandState {
    /// Whether the cameras track the hand. While it is not [`Tracking`](TrackingState::Tracking)
    /// the joints are at rest: invalid, untracked, at the origin.
    pub tracking_state: TrackingState,
    /// The 26 joints, at the [`index`](HandJointType::index) of each [`HandJointType`]. Jetpack
    /// XR's `handJoints` map.
    pub joints: [HandJoint; HandJointType::COUNT],
    /// Where the hand points, or `None` while the runtime has no `XR_FB_hand_tracking_aim` or gives
    /// no aim.
    pub aim: Option<HandAim>,
}

impl HandState {
    /// The joint of `joint_type`.
    pub const fn joint(&self, joint_type: HandJointType) -> &HandJoint {
        &self.joints[joint_type.index()]
    }
}

#[cfg(test)]
#[path = "tests/hand_state_tests.rs"]
mod tests;
