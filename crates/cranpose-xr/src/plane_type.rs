//! A summary of the direction that a plane faces.

/// A simple summary of the normal vector of a [`Plane`](crate::Plane). Jetpack XR's `PlaneType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PlaneType {
    /// A horizontal plane that faces downward, such as a ceiling. Jetpack XR's
    /// `PlaneType.HORIZONTAL_DOWNWARD_FACING`.
    HorizontalDownwardFacing,
    /// A horizontal plane that faces upward, such as a floor or a tabletop. Jetpack XR's
    /// `PlaneType.HORIZONTAL_UPWARD_FACING`.
    HorizontalUpwardFacing,
    /// A vertical plane, such as a wall. Jetpack XR's `PlaneType.VERTICAL`.
    Vertical,
}
