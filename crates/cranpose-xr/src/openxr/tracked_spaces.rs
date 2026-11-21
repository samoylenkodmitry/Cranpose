//! The tracked spaces that the panels of a following subspace ride in, as a frame locates them.
//!
//! A panel that follows every move of the head or of a controller gets its pose in the room from
//! the pose that the frame located for the target. Its layers go to the compositor in the target's
//! own space, at that pose relative to the space. The compositor places such a layer with the
//! newest pose that it has of the space at the time the frame shows, so the panel holds its place
//! against the head or the controller.

use openxr as xr;

use crate::{Pose, space::TrackedSpace};

// A tracked space and its pose in the room in the frame that ends.
#[derive(Clone, Copy)]
pub(crate) struct LocatedSpace {
    pub(crate) space: xr::sys::Space,
    pub(crate) pose: Pose,
}

impl LocatedSpace {
    // `pose`, given in the room, as a pose in this space.
    pub(crate) fn relative(&self, pose: &Pose) -> Pose {
        self.pose.inverse().then(pose)
    }
}

// The head's space and the aim space of each controller, left then right, with their poses in the
// frame that ends. A space that the frame did not locate is `None`. So is the aim of a controller
// that a script plays: the runtime's space does not follow the script.
#[derive(Clone, Copy, Default)]
pub(crate) struct TrackedSpaces {
    pub(crate) view: Option<LocatedSpace>,
    pub(crate) aims: [Option<LocatedSpace>; 2],
}

impl TrackedSpaces {
    // The space `tracked` as the frame located it.
    pub(crate) fn located(&self, tracked: TrackedSpace) -> Option<LocatedSpace> {
        match tracked {
            TrackedSpace::View => self.view,
            TrackedSpace::Aim(hand) => self.aims[hand.index()],
        }
    }
}
