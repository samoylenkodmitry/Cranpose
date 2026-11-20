//! The extension that supplies the face's weights.

/// The OpenXR extension that supplies the weights of the face. A runtime that offers both gives
/// the weights of Meta's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FaceTrackingProvider {
    /// `XR_FB_face_tracking2`, which a Meta Quest offers.
    Meta,
    /// `XR_ANDROID_face_tracking`, which Android XR offers.
    Android,
}
