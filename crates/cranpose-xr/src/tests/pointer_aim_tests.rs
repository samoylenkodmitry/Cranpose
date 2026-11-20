use super::*;
use crate::{Pinch, Pose, TrackedPose, Trigger};

// A controller that the runtime tracks, pulled by `trigger`, at `x` meters.
fn controller(x: f32, trigger: f32) -> Controller {
    Controller {
        profile: Some(InteractionProfile::MetaTouchPlus),
        aim: Some(TrackedPose {
            pose: Pose::at([x, 0.0, 0.0]),
            linear_velocity: None,
            angular_velocity: None,
        }),
        trigger: Trigger {
            value: trigger,
            touched: true,
        },
        ..Controller::default()
    }
}

// The hand interaction profile's hand: the runtime reports its pointer as a controller's actions.
fn profile_hand(x: f32, pinch: f32) -> Controller {
    Controller {
        profile: Some(InteractionProfile::ExtHand),
        ..controller(x, pinch)
    }
}

// A hand that the cameras track, with its aim at `x` meters and its index finger pinching by
// `strength`.
fn hand(x: f32, strength: f32) -> HandState {
    HandState {
        tracking_state: TrackingState::Tracking,
        aim: Some(HandAim {
            pose: Pose::at([x, 0.0, 0.0]),
            index: Pinch {
                strength,
                pinching: strength > 0.8,
            },
            ..HandAim::default()
        }),
        ..HandState::default()
    }
}

#[test]
fn a_tracked_hand_with_no_controller_points_by_its_aim_and_presses_with_its_index_finger() {
    let aim = pointer_aim(&Controller::default(), Some(&hand(0.25, 0.7))).expect("the hand points");
    assert_eq!(aim.source, InputSource::Hands);
    assert_eq!(aim.pose, Pose::at([0.25, 0.0, 0.0]));
    assert_eq!(aim.ray.origin, [0.25, 0.0, 0.0]);
    assert_eq!(aim.ray.direction, [0.0, 0.0, -1.0]);
    assert_eq!(aim.trigger, 0.7);
    assert_eq!(aim.stick, 0.0);
}

#[test]
fn only_the_index_finger_presses() {
    let mut state = hand(0.0, 0.0);
    if let Some(aim) = &mut state.aim {
        aim.middle = Pinch {
            strength: 1.0,
            pinching: true,
        };
        aim.ring.strength = 1.0;
        aim.little.strength = 1.0;
    }
    let aim = pointer_aim(&Controller::default(), Some(&state)).expect("the hand points");
    assert_eq!(aim.trigger, 0.0);
}

#[test]
fn an_app_that_did_not_ask_for_hand_tracking_has_no_hand_pointer() {
    assert_eq!(pointer_aim(&Controller::default(), None), None);
    // The hand interaction profile reports a hand through the controller's actions. It stays
    // quiet as well
    assert_eq!(pointer_aim(&profile_hand(0.0, 1.0), None), None);
}

#[test]
fn a_controller_points_whether_or_not_the_app_asked_for_hands() {
    for hand in [None, Some(&HandState::default())] {
        let aim = pointer_aim(&controller(0.5, 0.9), hand).expect("the controller points");
        assert_eq!(aim.source, InputSource::Controller);
        assert_eq!(aim.trigger, 0.9);
    }
}

#[test]
fn a_controller_in_use_takes_the_side_from_the_hand() {
    // The cameras still track the hand that holds the controller
    let aim = pointer_aim(&controller(0.5, 0.0), Some(&hand(0.0, 1.0))).expect("it points");
    assert_eq!(aim.source, InputSource::Controller);
    assert_eq!(aim.pose, Pose::at([0.5, 0.0, 0.0]));
    assert_eq!(aim.trigger, 0.0);
}

#[test]
fn a_hand_that_is_not_tracked_points_at_nothing() {
    for tracking_state in [TrackingState::Paused, TrackingState::Stopped] {
        let state = HandState {
            tracking_state,
            ..hand(0.0, 1.0)
        };
        assert_eq!(pointer_aim(&Controller::default(), Some(&state)), None);
    }
    // A hand that the runtime gives no aim for has only its joints
    let joints_only = HandState {
        tracking_state: TrackingState::Tracking,
        ..HandState::default()
    };
    assert_eq!(
        pointer_aim(&Controller::default(), Some(&joints_only)),
        None
    );
}

#[test]
fn a_hand_that_makes_the_system_gesture_points_at_nothing() {
    for (system_gesture, menu_pressed) in [(true, false), (false, true), (true, true)] {
        let mut state = hand(0.0, 1.0);
        if let Some(aim) = &mut state.aim {
            aim.system_gesture = system_gesture;
            aim.menu_pressed = menu_pressed;
        }
        assert_eq!(pointer_aim(&Controller::default(), Some(&state)), None);
        // The hand interaction profile reports the same hand, and its pinch stays out of reach
        assert_eq!(pointer_aim(&profile_hand(0.0, 1.0), Some(&state)), None);
    }
}

#[test]
fn a_runtime_without_the_aim_extension_points_through_the_hand_interaction_profile() {
    let aim =
        pointer_aim(&profile_hand(0.3, 0.8), Some(&HandState::default())).expect("the hand points");
    assert_eq!(aim.source, InputSource::Hands);
    assert_eq!(aim.pose, Pose::at([0.3, 0.0, 0.0]));
    assert_eq!(aim.trigger, 0.8);
    assert_eq!(aim.stick, 0.0);
    // The same runtime tracks the joints of the hand, which add no aim
    let joints_only = HandState {
        tracking_state: TrackingState::Tracking,
        ..HandState::default()
    };
    assert_eq!(
        pointer_aim(&profile_hand(0.3, 0.8), Some(&joints_only)).map(|aim| aim.source),
        Some(InputSource::Hands)
    );
}

#[test]
fn the_aim_extension_takes_the_hand_before_the_hand_interaction_profile() {
    let aim = pointer_aim(&profile_hand(0.3, 0.8), Some(&hand(-0.2, 0.1))).expect("it points");
    assert_eq!(aim.source, InputSource::Hands);
    assert_eq!(aim.pose, Pose::at([-0.2, 0.0, 0.0]));
    assert_eq!(aim.trigger, 0.1);
}

#[test]
fn only_a_controllers_thumbstick_pushes() {
    let mut touch = controller(0.0, 0.0);
    touch.thumbstick.y = 1.0;
    let aim = pointer_aim(&touch, Some(&HandState::default())).expect("it points");
    assert_eq!(aim.stick, 1.0);
    let hand_profile = Controller {
        thumbstick: touch.thumbstick,
        ..profile_hand(0.0, 0.0)
    };
    let aim = pointer_aim(&hand_profile, Some(&HandState::default())).expect("it points");
    assert_eq!(aim.stick, 0.0);
    assert_eq!(
        pointer_aim(&Controller::default(), Some(&hand(0.0, 0.0))).map(|aim| aim.stick),
        Some(0.0)
    );
}
