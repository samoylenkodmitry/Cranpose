//! One joint of a tracked hand.

use crate::Pose;

/// One joint of a tracked hand: where it is, and how far the runtime trusts it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HandJoint {
    /// The joint's place and turn in the app's room.
    pub pose: Pose,
    /// The radius in meters of a sphere that touches the skin around the joint.
    pub radius: f32,
    /// Whether the pose is usable. A joint that is valid and not [`tracked`](Self::tracked) holds
    /// the runtime's estimate, as of a finger that the cameras do not see.
    pub valid: bool,
    /// Whether the cameras see the joint now.
    pub tracked: bool,
}
