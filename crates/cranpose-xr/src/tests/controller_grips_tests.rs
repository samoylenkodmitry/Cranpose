// Which controllers show a model, and where they stand.

use super::*;
use crate::{TrackedPose, pose::turn_degrees};

fn at(position: [f32; 3]) -> Option<TrackedPose> {
    Some(TrackedPose {
        pose: Pose {
            position,
            orientation: turn_degrees(0.0, 30.0, 0.0),
        },
        linear_velocity: None,
        angular_velocity: None,
    })
}

// A controller that the runtime tracks, in the hand that the profile names.
fn tracked(profile: Option<InteractionProfile>, position: [f32; 3]) -> Controller {
    Controller {
        profile,
        grip: at(position),
        aim: at(position),
        ..Controller::default()
    }
}

fn touch(position: [f32; 3]) -> Controller {
    tracked(Some(InteractionProfile::MetaTouchPlus), position)
}

#[test]
fn a_tracked_controller_shows_its_model_at_its_grip_pose() {
    let left = touch([-0.2, 1.0, -0.3]);
    let grips = shown([&left, &Controller::default()]);
    assert_eq!(grips[0], left.grip.map(|grip| grip.pose));
    assert_eq!(grips[1], None);
}

#[test]
fn the_grip_and_not_the_aim_places_the_model() {
    let mut controller = touch([0.0, 1.0, 0.0]);
    controller.aim = at([0.0, 1.0, -0.5]);
    let [left, _] = shown([&controller, &Controller::default()]);
    assert_eq!(left.map(|pose| pose.position), Some([0.0, 1.0, 0.0]));
}

#[test]
fn a_tracked_hand_with_no_controller_shows_no_model_whatever_the_runtime_keeps_valid() {
    for profile in [
        InteractionProfile::ExtHand,
        InteractionProfile::MicrosoftHand,
    ] {
        let hand = tracked(Some(profile), [0.1, 1.0, -0.3]);
        assert_eq!(shown([&hand, &hand]), [None, None], "{profile:?}");
    }
}

#[test]
fn each_hand_decides_for_itself() {
    let controller = touch([0.2, 1.0, -0.3]);
    let hand = tracked(Some(InteractionProfile::ExtHand), [-0.2, 1.0, -0.3]);
    let grips = shown([&hand, &controller]);
    assert_eq!(grips[0], None);
    assert!(grips[1].is_some());
}

#[test]
fn a_controller_that_the_runtime_does_not_track_shows_no_model() {
    let lost = Controller {
        profile: Some(InteractionProfile::MetaTouchPlus),
        grip: None,
        aim: at([0.0; 3]),
        ..Controller::default()
    };
    assert_eq!(shown([&lost, &lost]), [None, None]);
}

#[test]
fn a_controller_with_no_reported_profile_still_shows_a_model_while_its_grip_is_tracked() {
    let sleepy = tracked(None, [0.0, 1.0, 0.0]);
    assert!(shown([&sleepy, &sleepy])[0].is_some());
}

#[test]
fn a_controller_that_lies_still_is_in_use() {
    // The state of the pointer decides nothing: no trigger, no stick, no movement
    let resting = touch([0.0, 0.7, -0.4]);
    assert_eq!(resting.trigger.value, 0.0);
    assert!(shown([&resting, &resting])[0].is_some());
}
