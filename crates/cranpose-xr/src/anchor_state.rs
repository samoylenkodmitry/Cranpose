//! Where an anchor is and whether the runtime holds it.

use crate::{Pose, TrackingState};

/// The state of an [`Anchor`](crate::Anchor): where it is and whether the runtime tracks it.
/// Jetpack XR's `Anchor.State`.
///
/// The pose is in the app's room, the OpenXR LOCAL space that [`XrFrame`](crate::XrFrame)
/// describes, where Jetpack XR's pose is in its perception space.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct AnchorState {
    /// Where the anchor is in the app's room. It stays at the last place the runtime gave while the
    /// tracking state is not [`Tracking`](TrackingState::Tracking). An anchor that the app loads
    /// from storage has the identity pose until the runtime places it for the first time.
    pub pose: Pose,
    /// Whether the runtime tracks the anchor now. A new anchor is
    /// [`Paused`](TrackingState::Paused) until the runtime holds it. A runtime that refuses it, an
    /// anchor that the app detached and an anchor whose resources the runtime lost are
    /// [`Stopped`](TrackingState::Stopped).
    pub tracking_state: TrackingState,
}
