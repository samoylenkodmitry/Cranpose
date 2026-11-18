//! Where a space is in the app's room and how it moves.

use openxr as xr;

use super::session::pose_of;
use crate::TrackedPose;

// Where `space` is in `base` at `time` and how it moves, or `None` while the runtime does not
// track it.
pub(crate) fn tracked(space: &xr::Space, base: &xr::Space, time: xr::Time) -> Option<TrackedPose> {
    let (location, velocity) = space.relate(base, time).ok()?;
    // A pose and its velocities count only when the runtime says they are valid
    let valid = xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID;
    if !location.location_flags.contains(valid) {
        return None;
    }
    let vector = |v: xr::Vector3f| [v.x, v.y, v.z];
    Some(TrackedPose {
        pose: pose_of(&location.pose),
        linear_velocity: velocity
            .velocity_flags
            .contains(xr::SpaceVelocityFlags::LINEAR_VALID)
            .then(|| vector(velocity.linear_velocity)),
        angular_velocity: velocity
            .velocity_flags
            .contains(xr::SpaceVelocityFlags::ANGULAR_VALID)
            .then(|| vector(velocity.angular_velocity)),
    })
}
