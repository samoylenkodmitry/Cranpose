//! Which input points for a hand in a frame.

use crate::{
    Controller, HandAim, HandState, InputSource, InteractionProfile, Ray, TrackingState,
    pointer::Aim,
};

// The aim that the pointer takes for one hand, or `None` while nothing points for it.
//
// A controller that the runtime tracks points first. A hand points only when the app asked for
// hand tracking, which `hand` being `Some` says, and only while no controller is in use on its
// side: the two inputs of a side never mix, so a user can hold a controller in one hand and point
// with the other.
//
// A tracked hand points by its aim from `XR_FB_hand_tracking_aim`, with the pinch of its index
// finger as the trigger. A runtime without that extension, such as Android XR's, reports the same
// through the hand interaction profile: the hand's `aim` pose, and a pinch that the profile binds
// to the trigger. The hand interaction profile never counts as a controller, so a hand that
// points through it is a hand in the rules above.
pub(crate) fn pointer_aim(controller: &Controller, hand: Option<&HandState>) -> Option<Aim> {
    let by_hands = controller.profile.is_some_and(InteractionProfile::is_hand);
    if !by_hands && let Some(aim) = of_controller(controller, InputSource::Controller) {
        return Some(aim);
    }
    let hand = hand?;
    match hand
        .aim
        .filter(|_| hand.tracking_state == TrackingState::Tracking)
    {
        Some(aim) => of_hand(&aim),
        None if by_hands => of_controller(controller, InputSource::Hands),
        None => None,
    }
}

// The aim that a controller's actions give. The hand interaction profile has no thumbstick.
fn of_controller(controller: &Controller, source: InputSource) -> Option<Aim> {
    let aim = controller.aim?;
    Some(Aim {
        ray: Ray::of_aim(&aim.pose),
        pose: aim.pose,
        trigger: controller.trigger.value,
        stick: match source {
            InputSource::Controller => controller.thumbstick.y,
            InputSource::Hands => 0.0,
        },
        source,
    })
}

// The hand's aim. A hand that makes the system's gesture, the palm turned to the face with a
// pinch that opens the menu, points at nothing: its pinch belongs to the system, and a press
// from it would click the app's panel behind the menu.
fn of_hand(aim: &HandAim) -> Option<Aim> {
    if aim.system_gesture || aim.menu_pressed {
        return None;
    }
    Some(Aim {
        ray: aim.ray(),
        pose: aim.pose,
        trigger: aim.index.strength,
        stick: 0.0,
        source: InputSource::Hands,
    })
}

#[cfg(test)]
#[path = "tests/pointer_aim_tests.rs"]
mod tests;
