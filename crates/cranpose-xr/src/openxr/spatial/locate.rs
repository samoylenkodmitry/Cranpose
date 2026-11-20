//! Where a space of the runtime is in the app's room.

use openxr as xr;

use super::super::session::pose_of;
use crate::{Pose, TrackingState};

// Where `space` is in `base` at `time`: its pose when the runtime gives a valid one, and whether the
// runtime tracks it. A pose that is valid and not tracked is the runtime's last estimate, so the
// space is paused. A runtime that cannot locate the space gives no pose and pauses it.
pub(crate) fn locate_anchor(
    space: &xr::Space,
    base: &xr::Space,
    time: xr::Time,
) -> (Option<Pose>, TrackingState) {
    let Ok(location) = space.locate(base, time) else {
        return (None, TrackingState::Paused);
    };
    let flags = location.location_flags;
    let valid = xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID;
    let tracked =
        xr::SpaceLocationFlags::POSITION_TRACKED | xr::SpaceLocationFlags::ORIENTATION_TRACKED;
    if !flags.contains(valid) {
        return (None, TrackingState::Paused);
    }
    let tracking = if flags.contains(tracked) {
        TrackingState::Tracking
    } else {
        TrackingState::Paused
    };
    (Some(pose_of(&location.pose)), tracking)
}
