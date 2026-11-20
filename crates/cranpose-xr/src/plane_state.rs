//! Where a plane of the room is and what it is.

use crate::{PlaneLabel, Pose, TrackingState};

/// The state of a [`Plane`](crate::Plane): a finite rectangle in the room. Jetpack XR's
/// `Plane.State`, which also gives the plane's vertices and the plane that subsumed it.
/// [`Plane::with_vertices`](crate::Plane::with_vertices) and
/// [`Plane::subsumed_by`](crate::Plane::subsumed_by) give those.
///
/// The pose is in the app's room, the OpenXR LOCAL space that [`XrFrame`](crate::XrFrame)
/// describes, where Jetpack XR's pose is in its perception space.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct PlaneState {
    /// The center of the plane's bounding rectangle. The +Y axis of the pose is the plane's
    /// normal, which points into the room. Jetpack XR's `centerPose`.
    pub center_pose: Pose,
    /// The size of the bounding rectangle in meters along the X and the Z axes of the
    /// [`center_pose`](Self::center_pose). Jetpack XR's `extents`.
    pub extents: [f32; 2],
    /// What the plane is.
    pub label: PlaneLabel,
    /// Whether the runtime tracks the plane now.
    pub tracking_state: TrackingState,
}
