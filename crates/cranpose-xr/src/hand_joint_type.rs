//! The joints of a tracked hand.

/// A joint of a tracked hand. Jetpack XR's `HandJointType`, with its names and OpenXR's order: the
/// wrist and palm first, then the thumb, index, middle, ring and little finger from the hand toward
/// the tip. A [`HandState`](crate::HandState) holds one [`HandJoint`](crate::HandJoint) for each, at
/// the joint's [`index`](Self::index).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HandJointType {
    /// OpenXR's `XR_HAND_JOINT_PALM_EXT`.
    Palm,
    /// OpenXR's `XR_HAND_JOINT_WRIST_EXT`.
    Wrist,
    /// OpenXR's `XR_HAND_JOINT_THUMB_METACARPAL_EXT`.
    ThumbMetacarpal,
    /// OpenXR's `XR_HAND_JOINT_THUMB_PROXIMAL_EXT`.
    ThumbProximal,
    /// OpenXR's `XR_HAND_JOINT_THUMB_DISTAL_EXT`.
    ThumbDistal,
    /// OpenXR's `XR_HAND_JOINT_THUMB_TIP_EXT`.
    ThumbTip,
    /// OpenXR's `XR_HAND_JOINT_INDEX_METACARPAL_EXT`.
    IndexMetacarpal,
    /// OpenXR's `XR_HAND_JOINT_INDEX_PROXIMAL_EXT`.
    IndexProximal,
    /// OpenXR's `XR_HAND_JOINT_INDEX_INTERMEDIATE_EXT`.
    IndexIntermediate,
    /// OpenXR's `XR_HAND_JOINT_INDEX_DISTAL_EXT`.
    IndexDistal,
    /// OpenXR's `XR_HAND_JOINT_INDEX_TIP_EXT`.
    IndexTip,
    /// OpenXR's `XR_HAND_JOINT_MIDDLE_METACARPAL_EXT`.
    MiddleMetacarpal,
    /// OpenXR's `XR_HAND_JOINT_MIDDLE_PROXIMAL_EXT`.
    MiddleProximal,
    /// OpenXR's `XR_HAND_JOINT_MIDDLE_INTERMEDIATE_EXT`.
    MiddleIntermediate,
    /// OpenXR's `XR_HAND_JOINT_MIDDLE_DISTAL_EXT`.
    MiddleDistal,
    /// OpenXR's `XR_HAND_JOINT_MIDDLE_TIP_EXT`.
    MiddleTip,
    /// OpenXR's `XR_HAND_JOINT_RING_METACARPAL_EXT`.
    RingMetacarpal,
    /// OpenXR's `XR_HAND_JOINT_RING_PROXIMAL_EXT`.
    RingProximal,
    /// OpenXR's `XR_HAND_JOINT_RING_INTERMEDIATE_EXT`.
    RingIntermediate,
    /// OpenXR's `XR_HAND_JOINT_RING_DISTAL_EXT`.
    RingDistal,
    /// OpenXR's `XR_HAND_JOINT_RING_TIP_EXT`.
    RingTip,
    /// OpenXR's `XR_HAND_JOINT_LITTLE_METACARPAL_EXT`.
    LittleMetacarpal,
    /// OpenXR's `XR_HAND_JOINT_LITTLE_PROXIMAL_EXT`.
    LittleProximal,
    /// OpenXR's `XR_HAND_JOINT_LITTLE_INTERMEDIATE_EXT`.
    LittleIntermediate,
    /// OpenXR's `XR_HAND_JOINT_LITTLE_DISTAL_EXT`.
    LittleDistal,
    /// OpenXR's `XR_HAND_JOINT_LITTLE_TIP_EXT`.
    LittleTip,
}

impl HandJointType {
    /// How many joints or shapes there are.
    pub const COUNT: usize = 26;

    /// Every value, in the order of the OpenXR enum that numbers them.
    pub const ALL: [Self; 26] = [
        Self::Palm,
        Self::Wrist,
        Self::ThumbMetacarpal,
        Self::ThumbProximal,
        Self::ThumbDistal,
        Self::ThumbTip,
        Self::IndexMetacarpal,
        Self::IndexProximal,
        Self::IndexIntermediate,
        Self::IndexDistal,
        Self::IndexTip,
        Self::MiddleMetacarpal,
        Self::MiddleProximal,
        Self::MiddleIntermediate,
        Self::MiddleDistal,
        Self::MiddleTip,
        Self::RingMetacarpal,
        Self::RingProximal,
        Self::RingIntermediate,
        Self::RingDistal,
        Self::RingTip,
        Self::LittleMetacarpal,
        Self::LittleProximal,
        Self::LittleIntermediate,
        Self::LittleDistal,
        Self::LittleTip,
    ];

    /// The value's place in the arrays that hold one entry for each.
    pub const fn index(self) -> usize {
        self as usize
    }
}
