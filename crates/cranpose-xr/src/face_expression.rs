//! The blend shapes of Meta's face tracking.

use crate::FaceBlendShapeType;

/// A blend shape of `XR_FB_face_tracking2`, with Meta's names. Each is a weight from 0 to 1. A
/// [`MetaFaceState`](crate::MetaFaceState) holds one weight for each, at the shape's
/// [`index`](Self::index). Jetpack XR's [`FaceBlendShapeType`](crate::FaceBlendShapeType) names the
/// 63 shapes that both runtimes share, and [`blend_shape`](Self::blend_shape) maps to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FaceExpression {
    /// OpenXR's `XR_FACE_EXPRESSION2_BROW_LOWERER_L_FB`.
    BrowLowererL,
    /// OpenXR's `XR_FACE_EXPRESSION2_BROW_LOWERER_R_FB`.
    BrowLowererR,
    /// OpenXR's `XR_FACE_EXPRESSION2_CHEEK_PUFF_L_FB`.
    CheekPuffL,
    /// OpenXR's `XR_FACE_EXPRESSION2_CHEEK_PUFF_R_FB`.
    CheekPuffR,
    /// OpenXR's `XR_FACE_EXPRESSION2_CHEEK_RAISER_L_FB`.
    CheekRaiserL,
    /// OpenXR's `XR_FACE_EXPRESSION2_CHEEK_RAISER_R_FB`.
    CheekRaiserR,
    /// OpenXR's `XR_FACE_EXPRESSION2_CHEEK_SUCK_L_FB`.
    CheekSuckL,
    /// OpenXR's `XR_FACE_EXPRESSION2_CHEEK_SUCK_R_FB`.
    CheekSuckR,
    /// OpenXR's `XR_FACE_EXPRESSION2_CHIN_RAISER_B_FB`.
    ChinRaiserB,
    /// OpenXR's `XR_FACE_EXPRESSION2_CHIN_RAISER_T_FB`.
    ChinRaiserT,
    /// OpenXR's `XR_FACE_EXPRESSION2_DIMPLER_L_FB`.
    DimplerL,
    /// OpenXR's `XR_FACE_EXPRESSION2_DIMPLER_R_FB`.
    DimplerR,
    /// OpenXR's `XR_FACE_EXPRESSION2_EYES_CLOSED_L_FB`.
    EyesClosedL,
    /// OpenXR's `XR_FACE_EXPRESSION2_EYES_CLOSED_R_FB`.
    EyesClosedR,
    /// OpenXR's `XR_FACE_EXPRESSION2_EYES_LOOK_DOWN_L_FB`.
    EyesLookDownL,
    /// OpenXR's `XR_FACE_EXPRESSION2_EYES_LOOK_DOWN_R_FB`.
    EyesLookDownR,
    /// OpenXR's `XR_FACE_EXPRESSION2_EYES_LOOK_LEFT_L_FB`.
    EyesLookLeftL,
    /// OpenXR's `XR_FACE_EXPRESSION2_EYES_LOOK_LEFT_R_FB`.
    EyesLookLeftR,
    /// OpenXR's `XR_FACE_EXPRESSION2_EYES_LOOK_RIGHT_L_FB`.
    EyesLookRightL,
    /// OpenXR's `XR_FACE_EXPRESSION2_EYES_LOOK_RIGHT_R_FB`.
    EyesLookRightR,
    /// OpenXR's `XR_FACE_EXPRESSION2_EYES_LOOK_UP_L_FB`.
    EyesLookUpL,
    /// OpenXR's `XR_FACE_EXPRESSION2_EYES_LOOK_UP_R_FB`.
    EyesLookUpR,
    /// OpenXR's `XR_FACE_EXPRESSION2_INNER_BROW_RAISER_L_FB`.
    InnerBrowRaiserL,
    /// OpenXR's `XR_FACE_EXPRESSION2_INNER_BROW_RAISER_R_FB`.
    InnerBrowRaiserR,
    /// OpenXR's `XR_FACE_EXPRESSION2_JAW_DROP_FB`.
    JawDrop,
    /// OpenXR's `XR_FACE_EXPRESSION2_JAW_SIDEWAYS_LEFT_FB`.
    JawSidewaysLeft,
    /// OpenXR's `XR_FACE_EXPRESSION2_JAW_SIDEWAYS_RIGHT_FB`.
    JawSidewaysRight,
    /// OpenXR's `XR_FACE_EXPRESSION2_JAW_THRUST_FB`.
    JawThrust,
    /// OpenXR's `XR_FACE_EXPRESSION2_LID_TIGHTENER_L_FB`.
    LidTightenerL,
    /// OpenXR's `XR_FACE_EXPRESSION2_LID_TIGHTENER_R_FB`.
    LidTightenerR,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_CORNER_DEPRESSOR_L_FB`.
    LipCornerDepressorL,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_CORNER_DEPRESSOR_R_FB`.
    LipCornerDepressorR,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_CORNER_PULLER_L_FB`.
    LipCornerPullerL,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_CORNER_PULLER_R_FB`.
    LipCornerPullerR,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_FUNNELER_LB_FB`.
    LipFunnelerLb,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_FUNNELER_LT_FB`.
    LipFunnelerLt,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_FUNNELER_RB_FB`.
    LipFunnelerRb,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_FUNNELER_RT_FB`.
    LipFunnelerRt,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_PRESSOR_L_FB`.
    LipPressorL,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_PRESSOR_R_FB`.
    LipPressorR,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_PUCKER_L_FB`.
    LipPuckerL,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_PUCKER_R_FB`.
    LipPuckerR,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_STRETCHER_L_FB`.
    LipStretcherL,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_STRETCHER_R_FB`.
    LipStretcherR,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_SUCK_LB_FB`.
    LipSuckLb,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_SUCK_LT_FB`.
    LipSuckLt,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_SUCK_RB_FB`.
    LipSuckRb,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_SUCK_RT_FB`.
    LipSuckRt,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_TIGHTENER_L_FB`.
    LipTightenerL,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIP_TIGHTENER_R_FB`.
    LipTightenerR,
    /// OpenXR's `XR_FACE_EXPRESSION2_LIPS_TOWARD_FB`.
    LipsToward,
    /// OpenXR's `XR_FACE_EXPRESSION2_LOWER_LIP_DEPRESSOR_L_FB`.
    LowerLipDepressorL,
    /// OpenXR's `XR_FACE_EXPRESSION2_LOWER_LIP_DEPRESSOR_R_FB`.
    LowerLipDepressorR,
    /// OpenXR's `XR_FACE_EXPRESSION2_MOUTH_LEFT_FB`.
    MouthLeft,
    /// OpenXR's `XR_FACE_EXPRESSION2_MOUTH_RIGHT_FB`.
    MouthRight,
    /// OpenXR's `XR_FACE_EXPRESSION2_NOSE_WRINKLER_L_FB`.
    NoseWrinklerL,
    /// OpenXR's `XR_FACE_EXPRESSION2_NOSE_WRINKLER_R_FB`.
    NoseWrinklerR,
    /// OpenXR's `XR_FACE_EXPRESSION2_OUTER_BROW_RAISER_L_FB`.
    OuterBrowRaiserL,
    /// OpenXR's `XR_FACE_EXPRESSION2_OUTER_BROW_RAISER_R_FB`.
    OuterBrowRaiserR,
    /// OpenXR's `XR_FACE_EXPRESSION2_UPPER_LID_RAISER_L_FB`.
    UpperLidRaiserL,
    /// OpenXR's `XR_FACE_EXPRESSION2_UPPER_LID_RAISER_R_FB`.
    UpperLidRaiserR,
    /// OpenXR's `XR_FACE_EXPRESSION2_UPPER_LIP_RAISER_L_FB`.
    UpperLipRaiserL,
    /// OpenXR's `XR_FACE_EXPRESSION2_UPPER_LIP_RAISER_R_FB`.
    UpperLipRaiserR,
    /// OpenXR's `XR_FACE_EXPRESSION2_TONGUE_TIP_INTERDENTAL_FB`.
    TongueTipInterdental,
    /// OpenXR's `XR_FACE_EXPRESSION2_TONGUE_TIP_ALVEOLAR_FB`.
    TongueTipAlveolar,
    /// OpenXR's `XR_FACE_EXPRESSION2_TONGUE_FRONT_DORSAL_PALATE_FB`.
    TongueFrontDorsalPalate,
    /// OpenXR's `XR_FACE_EXPRESSION2_TONGUE_MID_DORSAL_PALATE_FB`.
    TongueMidDorsalPalate,
    /// OpenXR's `XR_FACE_EXPRESSION2_TONGUE_BACK_DORSAL_VELAR_FB`.
    TongueBackDorsalVelar,
    /// OpenXR's `XR_FACE_EXPRESSION2_TONGUE_OUT_FB`.
    TongueOut,
    /// OpenXR's `XR_FACE_EXPRESSION2_TONGUE_RETREAT_FB`.
    TongueRetreat,
}

impl FaceExpression {
    /// How many joints or shapes there are.
    pub const COUNT: usize = 70;

    // The shapes from the first one on that Meta and Android XR share, in the same order
    pub(crate) const SHARED: usize = 63;

    /// Every value, in the order of the OpenXR enum that numbers them.
    pub const ALL: [Self; 70] = [
        Self::BrowLowererL,
        Self::BrowLowererR,
        Self::CheekPuffL,
        Self::CheekPuffR,
        Self::CheekRaiserL,
        Self::CheekRaiserR,
        Self::CheekSuckL,
        Self::CheekSuckR,
        Self::ChinRaiserB,
        Self::ChinRaiserT,
        Self::DimplerL,
        Self::DimplerR,
        Self::EyesClosedL,
        Self::EyesClosedR,
        Self::EyesLookDownL,
        Self::EyesLookDownR,
        Self::EyesLookLeftL,
        Self::EyesLookLeftR,
        Self::EyesLookRightL,
        Self::EyesLookRightR,
        Self::EyesLookUpL,
        Self::EyesLookUpR,
        Self::InnerBrowRaiserL,
        Self::InnerBrowRaiserR,
        Self::JawDrop,
        Self::JawSidewaysLeft,
        Self::JawSidewaysRight,
        Self::JawThrust,
        Self::LidTightenerL,
        Self::LidTightenerR,
        Self::LipCornerDepressorL,
        Self::LipCornerDepressorR,
        Self::LipCornerPullerL,
        Self::LipCornerPullerR,
        Self::LipFunnelerLb,
        Self::LipFunnelerLt,
        Self::LipFunnelerRb,
        Self::LipFunnelerRt,
        Self::LipPressorL,
        Self::LipPressorR,
        Self::LipPuckerL,
        Self::LipPuckerR,
        Self::LipStretcherL,
        Self::LipStretcherR,
        Self::LipSuckLb,
        Self::LipSuckLt,
        Self::LipSuckRb,
        Self::LipSuckRt,
        Self::LipTightenerL,
        Self::LipTightenerR,
        Self::LipsToward,
        Self::LowerLipDepressorL,
        Self::LowerLipDepressorR,
        Self::MouthLeft,
        Self::MouthRight,
        Self::NoseWrinklerL,
        Self::NoseWrinklerR,
        Self::OuterBrowRaiserL,
        Self::OuterBrowRaiserR,
        Self::UpperLidRaiserL,
        Self::UpperLidRaiserR,
        Self::UpperLipRaiserL,
        Self::UpperLipRaiserR,
        Self::TongueTipInterdental,
        Self::TongueTipAlveolar,
        Self::TongueFrontDorsalPalate,
        Self::TongueMidDorsalPalate,
        Self::TongueBackDorsalVelar,
        Self::TongueOut,
        Self::TongueRetreat,
    ];

    /// The value's place in the arrays that hold one entry for each.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The blend shape of Jetpack XR that has the same meaning, or `None` for the tongue shapes that
    /// only Meta has.
    pub const fn blend_shape(self) -> Option<FaceBlendShapeType> {
        match self {
            Self::TongueOut => Some(FaceBlendShapeType::TongueOut),
            _ if self.index() < Self::SHARED => Some(FaceBlendShapeType::ALL[self.index()]),
            _ => None,
        }
    }
}
