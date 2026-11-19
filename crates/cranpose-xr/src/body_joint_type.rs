//! The joints of a tracked body.

/// A joint of a tracked body, with Meta's names. Jetpack XR has no body API. The first 70 values
/// are the joints of `XR_FB_body_tracking`, which covers the upper body and the hands. The last 14
/// are the legs and feet that `XR_META_body_tracking_full_body` adds. A
/// [`BodyState`](crate::BodyState) holds one [`BodyJoint`](crate::BodyJoint) for each, at the joint's
/// [`index`](Self::index).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BodyJointType {
    /// OpenXR's `XR_BODY_JOINT_ROOT_FB`.
    Root,
    /// OpenXR's `XR_BODY_JOINT_HIPS_FB`.
    Hips,
    /// OpenXR's `XR_BODY_JOINT_SPINE_LOWER_FB`.
    SpineLower,
    /// OpenXR's `XR_BODY_JOINT_SPINE_MIDDLE_FB`.
    SpineMiddle,
    /// OpenXR's `XR_BODY_JOINT_SPINE_UPPER_FB`.
    SpineUpper,
    /// OpenXR's `XR_BODY_JOINT_CHEST_FB`.
    Chest,
    /// OpenXR's `XR_BODY_JOINT_NECK_FB`.
    Neck,
    /// OpenXR's `XR_BODY_JOINT_HEAD_FB`.
    Head,
    /// OpenXR's `XR_BODY_JOINT_LEFT_SHOULDER_FB`.
    LeftShoulder,
    /// OpenXR's `XR_BODY_JOINT_LEFT_SCAPULA_FB`.
    LeftScapula,
    /// OpenXR's `XR_BODY_JOINT_LEFT_ARM_UPPER_FB`.
    LeftArmUpper,
    /// OpenXR's `XR_BODY_JOINT_LEFT_ARM_LOWER_FB`.
    LeftArmLower,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_WRIST_TWIST_FB`.
    LeftHandWristTwist,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_SHOULDER_FB`.
    RightShoulder,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_SCAPULA_FB`.
    RightScapula,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_ARM_UPPER_FB`.
    RightArmUpper,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_ARM_LOWER_FB`.
    RightArmLower,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_WRIST_TWIST_FB`.
    RightHandWristTwist,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_PALM_FB`.
    LeftHandPalm,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_WRIST_FB`.
    LeftHandWrist,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_THUMB_METACARPAL_FB`.
    LeftHandThumbMetacarpal,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_THUMB_PROXIMAL_FB`.
    LeftHandThumbProximal,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_THUMB_DISTAL_FB`.
    LeftHandThumbDistal,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_THUMB_TIP_FB`.
    LeftHandThumbTip,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_INDEX_METACARPAL_FB`.
    LeftHandIndexMetacarpal,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_INDEX_PROXIMAL_FB`.
    LeftHandIndexProximal,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_INDEX_INTERMEDIATE_FB`.
    LeftHandIndexIntermediate,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_INDEX_DISTAL_FB`.
    LeftHandIndexDistal,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_INDEX_TIP_FB`.
    LeftHandIndexTip,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_MIDDLE_METACARPAL_FB`.
    LeftHandMiddleMetacarpal,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_MIDDLE_PROXIMAL_FB`.
    LeftHandMiddleProximal,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_MIDDLE_INTERMEDIATE_FB`.
    LeftHandMiddleIntermediate,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_MIDDLE_DISTAL_FB`.
    LeftHandMiddleDistal,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_MIDDLE_TIP_FB`.
    LeftHandMiddleTip,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_RING_METACARPAL_FB`.
    LeftHandRingMetacarpal,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_RING_PROXIMAL_FB`.
    LeftHandRingProximal,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_RING_INTERMEDIATE_FB`.
    LeftHandRingIntermediate,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_RING_DISTAL_FB`.
    LeftHandRingDistal,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_RING_TIP_FB`.
    LeftHandRingTip,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_LITTLE_METACARPAL_FB`.
    LeftHandLittleMetacarpal,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_LITTLE_PROXIMAL_FB`.
    LeftHandLittleProximal,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_LITTLE_INTERMEDIATE_FB`.
    LeftHandLittleIntermediate,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_LITTLE_DISTAL_FB`.
    LeftHandLittleDistal,
    /// OpenXR's `XR_BODY_JOINT_LEFT_HAND_LITTLE_TIP_FB`.
    LeftHandLittleTip,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_PALM_FB`.
    RightHandPalm,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_WRIST_FB`.
    RightHandWrist,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_THUMB_METACARPAL_FB`.
    RightHandThumbMetacarpal,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_THUMB_PROXIMAL_FB`.
    RightHandThumbProximal,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_THUMB_DISTAL_FB`.
    RightHandThumbDistal,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_THUMB_TIP_FB`.
    RightHandThumbTip,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_INDEX_METACARPAL_FB`.
    RightHandIndexMetacarpal,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_INDEX_PROXIMAL_FB`.
    RightHandIndexProximal,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_INDEX_INTERMEDIATE_FB`.
    RightHandIndexIntermediate,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_INDEX_DISTAL_FB`.
    RightHandIndexDistal,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_INDEX_TIP_FB`.
    RightHandIndexTip,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_MIDDLE_METACARPAL_FB`.
    RightHandMiddleMetacarpal,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_MIDDLE_PROXIMAL_FB`.
    RightHandMiddleProximal,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_MIDDLE_INTERMEDIATE_FB`.
    RightHandMiddleIntermediate,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_MIDDLE_DISTAL_FB`.
    RightHandMiddleDistal,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_MIDDLE_TIP_FB`.
    RightHandMiddleTip,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_RING_METACARPAL_FB`.
    RightHandRingMetacarpal,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_RING_PROXIMAL_FB`.
    RightHandRingProximal,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_RING_INTERMEDIATE_FB`.
    RightHandRingIntermediate,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_RING_DISTAL_FB`.
    RightHandRingDistal,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_RING_TIP_FB`.
    RightHandRingTip,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_LITTLE_METACARPAL_FB`.
    RightHandLittleMetacarpal,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_LITTLE_PROXIMAL_FB`.
    RightHandLittleProximal,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_LITTLE_INTERMEDIATE_FB`.
    RightHandLittleIntermediate,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_LITTLE_DISTAL_FB`.
    RightHandLittleDistal,
    /// OpenXR's `XR_BODY_JOINT_RIGHT_HAND_LITTLE_TIP_FB`.
    RightHandLittleTip,
    /// OpenXR's `XR_FULL_BODY_JOINT_LEFT_UPPER_LEG_META`.
    LeftUpperLeg,
    /// OpenXR's `XR_FULL_BODY_JOINT_LEFT_LOWER_LEG_META`.
    LeftLowerLeg,
    /// OpenXR's `XR_FULL_BODY_JOINT_LEFT_FOOT_ANKLE_TWIST_META`.
    LeftFootAnkleTwist,
    /// OpenXR's `XR_FULL_BODY_JOINT_LEFT_FOOT_ANKLE_META`.
    LeftFootAnkle,
    /// OpenXR's `XR_FULL_BODY_JOINT_LEFT_FOOT_SUBTALAR_META`.
    LeftFootSubtalar,
    /// OpenXR's `XR_FULL_BODY_JOINT_LEFT_FOOT_TRANSVERSE_META`.
    LeftFootTransverse,
    /// OpenXR's `XR_FULL_BODY_JOINT_LEFT_FOOT_BALL_META`.
    LeftFootBall,
    /// OpenXR's `XR_FULL_BODY_JOINT_RIGHT_UPPER_LEG_META`.
    RightUpperLeg,
    /// OpenXR's `XR_FULL_BODY_JOINT_RIGHT_LOWER_LEG_META`.
    RightLowerLeg,
    /// OpenXR's `XR_FULL_BODY_JOINT_RIGHT_FOOT_ANKLE_TWIST_META`.
    RightFootAnkleTwist,
    /// OpenXR's `XR_FULL_BODY_JOINT_RIGHT_FOOT_ANKLE_META`.
    RightFootAnkle,
    /// OpenXR's `XR_FULL_BODY_JOINT_RIGHT_FOOT_SUBTALAR_META`.
    RightFootSubtalar,
    /// OpenXR's `XR_FULL_BODY_JOINT_RIGHT_FOOT_TRANSVERSE_META`.
    RightFootTransverse,
    /// OpenXR's `XR_FULL_BODY_JOINT_RIGHT_FOOT_BALL_META`.
    RightFootBall,
}

impl BodyJointType {
    /// How many joints or shapes there are.
    pub const COUNT: usize = 84;

    /// Every value, in the order of the OpenXR enum that numbers them.
    pub const ALL: [Self; 84] = [
        Self::Root,
        Self::Hips,
        Self::SpineLower,
        Self::SpineMiddle,
        Self::SpineUpper,
        Self::Chest,
        Self::Neck,
        Self::Head,
        Self::LeftShoulder,
        Self::LeftScapula,
        Self::LeftArmUpper,
        Self::LeftArmLower,
        Self::LeftHandWristTwist,
        Self::RightShoulder,
        Self::RightScapula,
        Self::RightArmUpper,
        Self::RightArmLower,
        Self::RightHandWristTwist,
        Self::LeftHandPalm,
        Self::LeftHandWrist,
        Self::LeftHandThumbMetacarpal,
        Self::LeftHandThumbProximal,
        Self::LeftHandThumbDistal,
        Self::LeftHandThumbTip,
        Self::LeftHandIndexMetacarpal,
        Self::LeftHandIndexProximal,
        Self::LeftHandIndexIntermediate,
        Self::LeftHandIndexDistal,
        Self::LeftHandIndexTip,
        Self::LeftHandMiddleMetacarpal,
        Self::LeftHandMiddleProximal,
        Self::LeftHandMiddleIntermediate,
        Self::LeftHandMiddleDistal,
        Self::LeftHandMiddleTip,
        Self::LeftHandRingMetacarpal,
        Self::LeftHandRingProximal,
        Self::LeftHandRingIntermediate,
        Self::LeftHandRingDistal,
        Self::LeftHandRingTip,
        Self::LeftHandLittleMetacarpal,
        Self::LeftHandLittleProximal,
        Self::LeftHandLittleIntermediate,
        Self::LeftHandLittleDistal,
        Self::LeftHandLittleTip,
        Self::RightHandPalm,
        Self::RightHandWrist,
        Self::RightHandThumbMetacarpal,
        Self::RightHandThumbProximal,
        Self::RightHandThumbDistal,
        Self::RightHandThumbTip,
        Self::RightHandIndexMetacarpal,
        Self::RightHandIndexProximal,
        Self::RightHandIndexIntermediate,
        Self::RightHandIndexDistal,
        Self::RightHandIndexTip,
        Self::RightHandMiddleMetacarpal,
        Self::RightHandMiddleProximal,
        Self::RightHandMiddleIntermediate,
        Self::RightHandMiddleDistal,
        Self::RightHandMiddleTip,
        Self::RightHandRingMetacarpal,
        Self::RightHandRingProximal,
        Self::RightHandRingIntermediate,
        Self::RightHandRingDistal,
        Self::RightHandRingTip,
        Self::RightHandLittleMetacarpal,
        Self::RightHandLittleProximal,
        Self::RightHandLittleIntermediate,
        Self::RightHandLittleDistal,
        Self::RightHandLittleTip,
        Self::LeftUpperLeg,
        Self::LeftLowerLeg,
        Self::LeftFootAnkleTwist,
        Self::LeftFootAnkle,
        Self::LeftFootSubtalar,
        Self::LeftFootTransverse,
        Self::LeftFootBall,
        Self::RightUpperLeg,
        Self::RightLowerLeg,
        Self::RightFootAnkleTwist,
        Self::RightFootAnkle,
        Self::RightFootSubtalar,
        Self::RightFootTransverse,
        Self::RightFootBall,
    ];

    /// The value's place in the arrays that hold one entry for each.
    pub const fn index(self) -> usize {
        self as usize
    }
}
