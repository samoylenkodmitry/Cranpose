//! Which planes a movable panel may snap to by their direction.

#[cfg(any(test, headset))]
use crate::PlaneType;

/// The direction of the planes that a movable panel may anchor to. Jetpack Compose XR's
/// `PlaneOrientation`, which `MovePolicy.anchor` takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PlaneOrientation {
    /// Every direction. Jetpack XR's `PlaneOrientation.Any`.
    Any,
    /// Planes that face up or down, such as floors, tabletops and ceilings. Jetpack XR's
    /// `PlaneOrientation.Horizontal`.
    Horizontal,
    /// Planes that stand upright, such as walls. Jetpack XR's `PlaneOrientation.Vertical`.
    Vertical,
}

impl PlaneOrientation {
    // Whether a plane of `plane_type` has this orientation.
    #[cfg(any(test, headset))]
    pub(crate) const fn admits(self, plane_type: PlaneType) -> bool {
        matches!(
            (self, plane_type),
            (Self::Any, _)
                | (
                    Self::Horizontal,
                    PlaneType::HorizontalDownwardFacing | PlaneType::HorizontalUpwardFacing
                )
                | (Self::Vertical, PlaneType::Vertical)
        )
    }
}
