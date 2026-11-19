//! The regions of the face that carry a confidence.

/// A region of the face that has a confidence. Jetpack XR's `FaceConfidenceRegion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FaceConfidenceRegion {
    /// The lower region of the face. OpenXR's `XR_FACE_CONFIDENCE_REGIONS_LOWER_ANDROID`.
    Lower,
    /// The left upper region of the face.
    LeftUpper,
    /// The right upper region of the face.
    RightUpper,
}

impl FaceConfidenceRegion {
    /// How many regions there are.
    pub const COUNT: usize = 3;

    /// Every region, in the order of the arrays that hold one confidence for each.
    pub const ALL: [Self; Self::COUNT] = [Self::Lower, Self::LeftUpper, Self::RightUpper];

    /// The region's place in the arrays that hold one entry for each.
    pub const fn index(self) -> usize {
        self as usize
    }
}
