//! What the runtime estimates a face from.

/// What `XR_FB_face_tracking2` estimates the face from. A Meta Quest 3 has no camera that sees
/// the mouth and the eyes, so it gives [`Audio`](Self::Audio). Jetpack XR has no such value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FaceDataSource {
    /// The cameras that see the face. The runtime may also use audio to improve the weights.
    Visual,
    /// The microphone. The runtime estimates the mouth from speech and leaves the eyes and
    /// the brows to a model of the head.
    Audio,
}
