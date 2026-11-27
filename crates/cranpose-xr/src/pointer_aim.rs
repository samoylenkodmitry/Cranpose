//! Which input points for a hand in a frame.

use crate::{
    Controller, HandAim, HandJointType, HandState, InputSource, InteractionProfile, Ray,
    TrackingState,
    aim::{Aim, Buttons},
    direct_touch::Fingertip,
};

// The aim that the pointer takes for one hand, or `None` while nothing points for it.
//
// A controller that the runtime tracks points first. A hand points only when the app asked for
// hand tracking, which `hand` being `Some` says, and only while no controller is in use on its
// side: the two inputs of a side never mix, so a user can hold a controller in one hand and point
// with the other.
//
// A hand points only while the cameras track it. A runtime keeps the hand interaction profile's
// aim valid while no hand is in view: a Quest reports it with no hand tracked, and the pointer
// would show a ray from nowhere.
//
// A tracked hand points by its aim from `XR_FB_hand_tracking_aim`, with the pinch of its index
// finger as the trigger. A runtime that tracks the joints but lacks that extension reports the
// same through the hand interaction profile: the hand's `aim` pose, and a pinch that the profile
// binds to the trigger. The hand interaction profile never counts as a controller, so a hand that
// points through it is a hand in the rules above.
//
// A hand's aim carries the tip of its index finger while the cameras see that joint, for the
// direct touch of the panels. A joint that the runtime only estimates touches nothing: a finger
// that the cameras do not see could press a panel where the finger is not.
pub(crate) fn pointer_aim(controller: &Controller, hand: Option<&HandState>) -> Option<Aim> {
    let by_hands = controller.profile.is_some_and(InteractionProfile::is_hand);
    if !by_hands && let Some(aim) = of_controller(controller, InputSource::Controller) {
        return Some(aim);
    }
    let hand = hand?;
    if hand.tracking_state != TrackingState::Tracking {
        return None;
    }
    let aim = match hand.aim {
        Some(aim) => of_hand(&aim),
        None if by_hands => of_controller(controller, InputSource::Hands),
        None => None,
    }?;
    let tip = hand.joint(HandJointType::IndexTip);
    Some(Aim {
        tip: (tip.valid && tip.tracked).then_some(Fingertip {
            position: tip.pose.position,
            radius: tip.radius,
        }),
        ..aim
    })
}

// The aim that a controller's actions give. The hand interaction profile has no thumbstick and no
// other button: its grasp is the squeeze, which presses nothing.
fn of_controller(controller: &Controller, source: InputSource) -> Option<Aim> {
    let aim = controller.aim?;
    let (stick, buttons) = match source {
        InputSource::Controller => (
            [controller.thumbstick.x, controller.thumbstick.y],
            Buttons {
                squeeze: controller.squeeze.value,
                primary: controller.primary.pressed,
                secondary: controller.secondary.pressed,
                stick_click: controller.thumbstick.pressed,
                menu: controller.menu,
            },
        ),
        InputSource::Hands => ([0.0; 2], Buttons::default()),
    };
    Some(Aim {
        ray: Ray::of_aim(&aim.pose),
        pose: aim.pose,
        trigger: controller.trigger.value,
        stick,
        buttons,
        source,
        tip: None,
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
        stick: [0.0; 2],
        buttons: Buttons::default(),
        source: InputSource::Hands,
        tip: None,
    })
}

#[cfg(test)]
#[path = "tests/pointer_aim_tests.rs"]
mod tests;
