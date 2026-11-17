//! What a move of a movable composable reports.

use crate::Pose;

/// The phase of a move: Jetpack XR's `SpatialMoveEventType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpatialMoveEventType {
    /// The user grabbed the content with a controller.
    Start,
    /// The user carries the content. Each frame in which its pose changes reports one.
    Moving,
    /// The user let the content go, or the runtime stopped tracking the controller that held it.
    End,
}

/// A step of a move that the user makes with a controller: Jetpack XR's `SpatialMoveEvent`.
///
/// The poses are relative to the origin of the subspace that holds the content, in meters, and
/// are the ones that `SubspaceModifier::with_pose` takes. A move has no scale or size.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct SpatialMoveEvent {
    /// The phase of the move.
    pub event_type: SpatialMoveEventType,
    /// The content's new pose.
    pub pose: Pose,
    /// The content's pose in the event before. In a [`Start`](SpatialMoveEventType::Start) event
    /// and in an [`End`](SpatialMoveEventType::End) event it equals `pose`.
    pub previous_pose: Pose,
}
