//! The state of the user's face in a frame.

use crate::{
    FaceBlendShapeType, FaceConfidenceRegion, FaceExpression, MetaFaceState, TrackingState,
};

/// The user's face in one frame, with Jetpack XR's names. Jetpack XR's `Face.State`. The weights
/// come from `XR_ANDROID_face_tracking`, or from `XR_FB_face_tracking2`, which has 63 of the same
/// blend shapes and the tongue's. A shape that Meta lacks, such as `TongueLeft`, stays 0 there.
/// [`XrFrame::meta_face`](crate::XrFrame::meta_face) has every weight of Meta's with its own
/// names.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceState {
    /// Whether the runtime tracks the face.
    pub tracking_state: TrackingState,
    /// Whether the weights are valid.
    pub is_valid: bool,
    /// The weights from 0 to 1, at the [`index`](FaceBlendShapeType::index) of each
    /// [`FaceBlendShapeType`]. Jetpack XR's `blendShapes` map.
    pub blend_shapes: [f32; FaceBlendShapeType::COUNT],
    /// How far the runtime trusts each region of the face, from 0 to 1, at the
    /// [`index`](FaceConfidenceRegion::index) of each [`FaceConfidenceRegion`]. Meta has one
    /// confidence for the whole upper face, which both upper regions take.
    pub confidence_regions: [f32; FaceConfidenceRegion::COUNT],
}

impl Default for FaceState {
    fn default() -> Self {
        Self {
            tracking_state: TrackingState::Stopped,
            is_valid: false,
            blend_shapes: [0.0; FaceBlendShapeType::COUNT],
            confidence_regions: [0.0; FaceConfidenceRegion::COUNT],
        }
    }
}

impl FaceState {
    /// The weight of `blend_shape`.
    pub const fn weight(&self, blend_shape: FaceBlendShapeType) -> f32 {
        self.blend_shapes[blend_shape.index()]
    }

    // The Jetpack XR shape of the weights that Meta's tracker gave. The face is tracked while the
    // weights are valid and paused while they are not.
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    pub(crate) fn from_meta(meta: &MetaFaceState) -> Self {
        let mut state = Self {
            tracking_state: if meta.is_valid {
                TrackingState::Tracking
            } else {
                TrackingState::Paused
            },
            is_valid: meta.is_valid,
            ..Self::default()
        };
        for expression in FaceExpression::ALL {
            if let Some(shape) = expression.blend_shape() {
                state.blend_shapes[shape.index()] = meta.weight(expression);
            }
        }
        state.confidence_regions[FaceConfidenceRegion::Lower.index()] = meta.lower_face_confidence;
        state.confidence_regions[FaceConfidenceRegion::LeftUpper.index()] =
            meta.upper_face_confidence;
        state.confidence_regions[FaceConfidenceRegion::RightUpper.index()] =
            meta.upper_face_confidence;
        state
    }
}

#[cfg(test)]
#[path = "tests/face_state_tests.rs"]
mod tests;
