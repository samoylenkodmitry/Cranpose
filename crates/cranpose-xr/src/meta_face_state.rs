//! The weights of Meta's face tracking in a frame.

use crate::{FaceDataSource, FaceExpression};

/// The face in one frame as `XR_FB_face_tracking2` reports it, with Meta's names. It holds every
/// weight that the runtime gives. [`FaceState`](crate::FaceState) holds the Jetpack XR names of
/// those that both Meta and Android XR share.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MetaFaceState {
    /// Whether the weights are valid. They are not while the runtime has no face, as while the
    /// headset is off the head.
    pub is_valid: bool,
    /// Whether the weights of the eyes' looks follow the eyes. They do while the user grants the
    /// eye tracking permission and the headset has eye cameras, and the runtime then gives the
    /// look weights from the eyes.
    pub is_eye_following_blendshapes_valid: bool,
    /// What the runtime estimates the weights from, or `None` while no tracker runs.
    pub data_source: Option<FaceDataSource>,
    /// The weights from 0 to 1, at the [`index`](FaceExpression::index) of each
    /// [`FaceExpression`].
    pub weights: [f32; FaceExpression::COUNT],
    /// How far the runtime trusts the weights of the lower face, from 0 to 1.
    pub lower_face_confidence: f32,
    /// How far the runtime trusts the weights of the upper face, from 0 to 1.
    pub upper_face_confidence: f32,
}

impl Default for MetaFaceState {
    fn default() -> Self {
        Self {
            is_valid: false,
            is_eye_following_blendshapes_valid: false,
            data_source: None,
            weights: [0.0; FaceExpression::COUNT],
            lower_face_confidence: 0.0,
            upper_face_confidence: 0.0,
        }
    }
}

impl MetaFaceState {
    /// The weight of `expression`.
    pub const fn weight(&self, expression: FaceExpression) -> f32 {
        self.weights[expression.index()]
    }
}
