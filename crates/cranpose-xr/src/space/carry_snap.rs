//! Where a carried panel comes to rest on a plane of the room.

use super::{Entry, PanelMove, Space};
#[cfg(any(test, headset))]
use super::{PanelKind, Ride};
use crate::Pose;
#[cfg(any(test, headset))]
use crate::{METERS_PER_DP, MovePolicy, SpatialMoveEventType};

// The pose in the room that a step of a carry gives its panel: where the carry puts it, or where
// that comes to rest on a plane when the panel's policy snaps and the step moves it.
#[cfg(any(test, headset))]
pub(super) fn snapped(space: &Space, entry: &Entry, step: &PanelMove) -> Pose {
    // A panel that rides a controller stays at the hand: a plane of the room does not hold it
    if step.event_type != SpatialMoveEventType::Moving || entry.slot.ride != Ride::Free {
        return step.pose;
    }
    let Some(filter) = entry.policy.as_ref().and_then(MovePolicy::snap_filter) else {
        return step.pose;
    };
    let [width, height] = entry.slot.size().map(|dp| dp * METERS_PER_DP);
    let depth = match entry.slot.kind {
        PanelKind::Volume { depth, .. } => depth * METERS_PER_DP,
        PanelKind::Content | PanelKind::Surface(_) => 0.0,
    };
    space
        .inner
        .spatial
        .get()
        .and_then(|store| store.snap(filter, &step.pose, [width, height, depth]))
        .unwrap_or(step.pose)
}

// Without a headset no plane exists to snap to, so the carry gives its panel the pose it
// carries.
#[cfg(not(any(test, headset)))]
pub(super) fn snapped(_space: &Space, _entry: &Entry, step: &PanelMove) -> Pose {
    step.pose
}
