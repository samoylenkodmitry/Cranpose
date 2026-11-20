//! The controllers that show a model in a frame, at their grips.

use crate::{Controller, InteractionProfile, Pose};

// The grip pose of each hand's controller that shows a model, left then right, or `None` for a
// hand that holds no controller in use.
//
// A controller is in use while the runtime tracks its grip and the hand's interaction profile is not
// that of a hand. A runtime keeps the hand profile's poses valid while no hand is in view. A tracked
// hand with no controller holds nothing to draw. A hand profile therefore shows no model. The state
// of the pointer decides nothing here: a controller that lies still is in use.
pub(crate) fn shown(controllers: [&Controller; 2]) -> [Option<Pose>; 2] {
    controllers.map(|controller| {
        if controller.profile.is_some_and(InteractionProfile::is_hand) {
            return None;
        }
        controller.grip.map(|grip| grip.pose)
    })
}

#[cfg(test)]
#[path = "tests/controller_grips_tests.rs"]
mod tests;
