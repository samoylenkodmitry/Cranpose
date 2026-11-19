//! The state of the tracked body in a frame.

use crate::{BodyJoint, BodyJointType};

/// The user's body in one frame, from `XR_FB_body_tracking` and, where the runtime offers it,
/// `XR_META_body_tracking_full_body`. Jetpack XR has no body API.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyState {
    /// Whether the runtime tracks the body now.
    pub active: bool,
    /// How far the runtime trusts the joints, from 0 to 1.
    pub confidence: f32,
    /// How many joints the runtime gives: 70 for the upper body and the hands, or 84 with the legs
    /// and feet of the full body. 0 while no tracker runs.
    pub joint_count: usize,
    /// The joints, at the [`index`](BodyJointType::index) of each [`BodyJointType`]. Those from
    /// `joint_count` on have not been written.
    pub joints: [BodyJoint; BodyJointType::COUNT],
}

impl Default for BodyState {
    fn default() -> Self {
        Self {
            active: false,
            confidence: 0.0,
            joint_count: 0,
            joints: [BodyJoint::default(); BodyJointType::COUNT],
        }
    }
}

impl BodyState {
    /// The joints that the runtime gives, in the order of [`BodyJointType::ALL`].
    pub fn tracked_joints(&self) -> &[BodyJoint] {
        &self.joints[..self.joint_count.min(self.joints.len())]
    }

    /// The joint of `joint_type`, or `None` while the runtime gives fewer joints, as the legs of
    /// a runtime that tracks the upper body only.
    pub fn joint(&self, joint_type: BodyJointType) -> Option<&BodyJoint> {
        self.tracked_joints().get(joint_type.index())
    }
}

#[cfg(test)]
#[path = "tests/body_state_tests.rs"]
mod tests;
