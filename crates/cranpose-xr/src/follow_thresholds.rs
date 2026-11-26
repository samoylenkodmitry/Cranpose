//! How far a target moves before soft content starts to follow it.

/// How far the target must move away from the content before a
/// [`SoftFollowMode`](crate::SoftFollowMode) starts to follow it. Jetpack XR's `FollowThresholds`.
///
/// The content starts to follow when any one value is exceeded: the distance between the target's
/// place and the content's place, or the difference in pitch, yaw or roll between the target's
/// turn and the content's turn. The turns count in the room's axes: yaw about the vertical axis,
/// pitch up and down, roll about the forward axis. A value of 0 lets the smallest move start the
/// follow.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FollowThresholds {
    /// The distance, in meters.
    pub translation_meters: f32,
    /// The difference in pitch, in degrees.
    pub pitch_degrees: f32,
    /// The difference in yaw, in degrees.
    pub yaw_degrees: f32,
    /// The difference in roll, in degrees.
    pub roll_degrees: f32,
}

impl FollowThresholds {
    /// No threshold: the smallest move starts the follow. Jetpack XR's `FollowThresholds.Zero`.
    pub const ZERO: Self = Self {
        translation_meters: 0.0,
        pitch_degrees: 0.0,
        yaw_degrees: 0.0,
        roll_degrees: 0.0,
    };
}

// A follow mode is a key of the composition, so it needs `Eq`. A threshold that is not a number
// never equals itself, which only makes the composition place the subspace again.
impl Eq for FollowThresholds {}
