//! One joint of a tracked body.

use crate::Pose;

/// One joint of a tracked body: where it is, and how far the runtime trusts it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BodyJoint {
    /// The joint's place and turn in the app's room.
    pub pose: Pose,
    /// Whether the pose is usable. A joint that is valid and not [`tracked`](Self::tracked) holds
    /// the runtime's estimate, as of a joint that no camera sees.
    pub valid: bool,
    /// Whether the runtime measures the joint now.
    pub tracked: bool,
}
