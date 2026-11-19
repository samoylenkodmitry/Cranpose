//! The blend shapes of a face.

use crate::FaceExpression;

/// A blend shape of a face. Jetpack XR's `FaceBlendShapeType`, with its names, in the order of
/// OpenXR's `XR_ANDROID_face_tracking` parameters. A [`FaceState`](crate::FaceState) holds one weight
/// from 0 to 1 for each, at the shape's [`index`](Self::index).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FaceBlendShapeType {
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_BROW_LOWERER_L_ANDROID`.
    BrowLowererLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_BROW_LOWERER_R_ANDROID`.
    BrowLowererRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_CHEEK_PUFF_L_ANDROID`.
    CheekPuffLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_CHEEK_PUFF_R_ANDROID`.
    CheekPuffRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_CHEEK_RAISER_L_ANDROID`.
    CheekRaiserLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_CHEEK_RAISER_R_ANDROID`.
    CheekRaiserRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_CHEEK_SUCK_L_ANDROID`.
    CheekSuckLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_CHEEK_SUCK_R_ANDROID`.
    CheekSuckRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_CHIN_RAISER_B_ANDROID`.
    ChinRaiserBottom,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_CHIN_RAISER_T_ANDROID`.
    ChinRaiserTop,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_DIMPLER_L_ANDROID`.
    DimplerLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_DIMPLER_R_ANDROID`.
    DimplerRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_EYES_CLOSED_L_ANDROID`.
    EyesClosedLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_EYES_CLOSED_R_ANDROID`.
    EyesClosedRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_EYES_LOOK_DOWN_L_ANDROID`.
    EyesLookDownLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_EYES_LOOK_DOWN_R_ANDROID`.
    EyesLookDownRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_EYES_LOOK_LEFT_L_ANDROID`.
    EyesLookLeftLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_EYES_LOOK_LEFT_R_ANDROID`.
    EyesLookLeftRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_EYES_LOOK_RIGHT_L_ANDROID`.
    EyesLookRightLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_EYES_LOOK_RIGHT_R_ANDROID`.
    EyesLookRightRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_EYES_LOOK_UP_L_ANDROID`.
    EyesLookUpLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_EYES_LOOK_UP_R_ANDROID`.
    EyesLookUpRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_INNER_BROW_RAISER_L_ANDROID`.
    InnerBrowRaiserLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_INNER_BROW_RAISER_R_ANDROID`.
    InnerBrowRaiserRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_JAW_DROP_ANDROID`.
    JawDrop,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_JAW_SIDEWAYS_LEFT_ANDROID`.
    JawSidewaysLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_JAW_SIDEWAYS_RIGHT_ANDROID`.
    JawSidewaysRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_JAW_THRUST_ANDROID`.
    JawThrust,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LID_TIGHTENER_L_ANDROID`.
    LidTightenerLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LID_TIGHTENER_R_ANDROID`.
    LidTightenerRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_CORNER_DEPRESSOR_L_ANDROID`.
    LipCornerDepressorLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_CORNER_DEPRESSOR_R_ANDROID`.
    LipCornerDepressorRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_CORNER_PULLER_L_ANDROID`.
    LipCornerPullerLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_CORNER_PULLER_R_ANDROID`.
    LipCornerPullerRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_FUNNELER_LB_ANDROID`.
    LipFunnelerLeftBottom,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_FUNNELER_LT_ANDROID`.
    LipFunnelerLeftTop,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_FUNNELER_RB_ANDROID`.
    LipFunnelerRightBottom,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_FUNNELER_RT_ANDROID`.
    LipFunnelerRightTop,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_PRESSOR_L_ANDROID`.
    LipPressorLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_PRESSOR_R_ANDROID`.
    LipPressorRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_PUCKER_L_ANDROID`.
    LipPuckerLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_PUCKER_R_ANDROID`.
    LipPuckerRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_STRETCHER_L_ANDROID`.
    LipStretcherLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_STRETCHER_R_ANDROID`.
    LipStretcherRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_SUCK_LB_ANDROID`.
    LipSuckLeftBottom,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_SUCK_LT_ANDROID`.
    LipSuckLeftTop,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_SUCK_RB_ANDROID`.
    LipSuckRightBottom,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_SUCK_RT_ANDROID`.
    LipSuckRightTop,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_TIGHTENER_L_ANDROID`.
    LipTightenerLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIP_TIGHTENER_R_ANDROID`.
    LipTightenerRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LIPS_TOWARD_ANDROID`.
    LipsToward,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LOWER_LIP_DEPRESSOR_L_ANDROID`.
    LowerLipDepressorLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_LOWER_LIP_DEPRESSOR_R_ANDROID`.
    LowerLipDepressorRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_MOUTH_LEFT_ANDROID`.
    MouthLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_MOUTH_RIGHT_ANDROID`.
    MouthRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_NOSE_WRINKLER_L_ANDROID`.
    NoseWrinklerLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_NOSE_WRINKLER_R_ANDROID`.
    NoseWrinklerRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_OUTER_BROW_RAISER_L_ANDROID`.
    OuterBrowRaiserLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_OUTER_BROW_RAISER_R_ANDROID`.
    OuterBrowRaiserRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_UPPER_LID_RAISER_L_ANDROID`.
    UpperLidRaiserLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_UPPER_LID_RAISER_R_ANDROID`.
    UpperLidRaiserRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_UPPER_LIP_RAISER_L_ANDROID`.
    UpperLipRaiserLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_UPPER_LIP_RAISER_R_ANDROID`.
    UpperLipRaiserRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_TONGUE_OUT_ANDROID`.
    TongueOut,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_TONGUE_LEFT_ANDROID`.
    TongueLeft,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_TONGUE_RIGHT_ANDROID`.
    TongueRight,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_TONGUE_UP_ANDROID`.
    TongueUp,
    /// OpenXR's `XR_FACE_PARAMETER_INDICES_TONGUE_DOWN_ANDROID`.
    TongueDown,
}

impl FaceBlendShapeType {
    /// How many joints or shapes there are.
    pub const COUNT: usize = 68;

    /// Every value, in the order of the OpenXR enum that numbers them.
    pub const ALL: [Self; 68] = [
        Self::BrowLowererLeft,
        Self::BrowLowererRight,
        Self::CheekPuffLeft,
        Self::CheekPuffRight,
        Self::CheekRaiserLeft,
        Self::CheekRaiserRight,
        Self::CheekSuckLeft,
        Self::CheekSuckRight,
        Self::ChinRaiserBottom,
        Self::ChinRaiserTop,
        Self::DimplerLeft,
        Self::DimplerRight,
        Self::EyesClosedLeft,
        Self::EyesClosedRight,
        Self::EyesLookDownLeft,
        Self::EyesLookDownRight,
        Self::EyesLookLeftLeft,
        Self::EyesLookLeftRight,
        Self::EyesLookRightLeft,
        Self::EyesLookRightRight,
        Self::EyesLookUpLeft,
        Self::EyesLookUpRight,
        Self::InnerBrowRaiserLeft,
        Self::InnerBrowRaiserRight,
        Self::JawDrop,
        Self::JawSidewaysLeft,
        Self::JawSidewaysRight,
        Self::JawThrust,
        Self::LidTightenerLeft,
        Self::LidTightenerRight,
        Self::LipCornerDepressorLeft,
        Self::LipCornerDepressorRight,
        Self::LipCornerPullerLeft,
        Self::LipCornerPullerRight,
        Self::LipFunnelerLeftBottom,
        Self::LipFunnelerLeftTop,
        Self::LipFunnelerRightBottom,
        Self::LipFunnelerRightTop,
        Self::LipPressorLeft,
        Self::LipPressorRight,
        Self::LipPuckerLeft,
        Self::LipPuckerRight,
        Self::LipStretcherLeft,
        Self::LipStretcherRight,
        Self::LipSuckLeftBottom,
        Self::LipSuckLeftTop,
        Self::LipSuckRightBottom,
        Self::LipSuckRightTop,
        Self::LipTightenerLeft,
        Self::LipTightenerRight,
        Self::LipsToward,
        Self::LowerLipDepressorLeft,
        Self::LowerLipDepressorRight,
        Self::MouthLeft,
        Self::MouthRight,
        Self::NoseWrinklerLeft,
        Self::NoseWrinklerRight,
        Self::OuterBrowRaiserLeft,
        Self::OuterBrowRaiserRight,
        Self::UpperLidRaiserLeft,
        Self::UpperLidRaiserRight,
        Self::UpperLipRaiserLeft,
        Self::UpperLipRaiserRight,
        Self::TongueOut,
        Self::TongueLeft,
        Self::TongueRight,
        Self::TongueUp,
        Self::TongueDown,
    ];

    /// The value's place in the arrays that hold one entry for each.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The blend shape of Meta that has the same meaning, or `None` for the tongue shapes that only
    /// Android XR has.
    pub const fn expression(self) -> Option<FaceExpression> {
        match self {
            Self::TongueOut => Some(FaceExpression::TongueOut),
            _ if self.index() < FaceExpression::SHARED => Some(FaceExpression::ALL[self.index()]),
            _ => None,
        }
    }
}
