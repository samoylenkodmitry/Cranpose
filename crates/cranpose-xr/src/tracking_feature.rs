//! The kinds of tracking an app asks for.

/// A kind of tracking that an app asks for in its build script, with `Use::hand_tracking()`,
/// `Use::body_tracking()`, `Use::face_tracking()` or `Use::eye_tracking()` of
/// `cranpose::capabilities`. An app that asks for none enables no extension, creates no tracker,
/// makes no call in a frame and adds no Android permission.
///
/// Jetpack XR asks at run time, with `Session.configure(Config(handTracking = ...))`. OpenXR fixes
/// the extensions when the instance is made, before the content composes, so Cranpose reads the
/// build's declaration instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TrackingFeature {
    /// The joints of both hands, and where a hand points. A hand with no controller in it
    /// points at the panels and presses with a pinch. `XR_EXT_hand_tracking` and
    /// `XR_FB_hand_tracking_aim`.
    Hands,
    /// The joints of the body. `XR_FB_body_tracking` and `XR_META_body_tracking_full_body`.
    Body,
    /// The weights of the blend shapes of the face. `XR_FB_face_tracking2` and
    /// `XR_ANDROID_face_tracking`.
    Face,
    /// Where the eyes look. `XR_EXT_eye_gaze_interaction` and `XR_FB_eye_tracking_social`.
    Eyes,
}

impl TrackingFeature {
    /// Every feature.
    pub const ALL: [Self; 4] = [Self::Hands, Self::Body, Self::Face, Self::Eyes];

    // The feature's place in an array that holds one entry for each feature.
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Hands => 0,
            Self::Body => 1,
            Self::Face => 2,
            Self::Eyes => 3,
        }
    }
}
