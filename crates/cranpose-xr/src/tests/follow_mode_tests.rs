use std::time::Duration;

use crate::{
    FollowMode, FollowTarget, Hand, HandJointType, HandState, Pose, Recenter, TrackedDimensions,
    TrackingState,
    follow_origin::Poses,
    following_subspace::tests::{ball, controller, head, same, seconds, upright_at},
    pose::turn_degrees,
    space::Ride,
};

// The ball of a soft subspace of the view with the head at `position`, turned left by `yaw`: a
// meter ahead of the head and 10 cm above it.
fn ahead_of(position: [f32; 3], yaw: f32) -> Pose {
    head(position, yaw).then(&Pose::at([0.0, 0.1, -1.0]))
}

fn millis(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

#[test]
fn a_soft_subspace_of_the_view_holds_still_for_small_moves_and_glides_after_a_turn() {
    let ball = ball(FollowTarget::View(FollowMode::soft(
        TrackedDimensions::TranslationAndYaw,
    )));
    let track = |position: [f32; 3], yaw: f32, time: Duration| {
        ball.space.track(
            &Poses {
                head: Some(head(position, yaw)),
                controllers: [None, None],
                ..Poses::default()
            },
            time,
            None,
        );
        ball.slot().pose
    };
    // The first tracked frame puts the ball in front of the head at once
    let first = ahead_of([0.0, 1.6, 0.0], 0.0);
    assert!(same(&track([0.0, 1.6, 0.0], 0.0, millis(1000)), &first));

    // A turn of 20 degrees and a step of 15 cm stay below the thresholds of 24 degrees and 20 cm
    assert!(same(&track([0.0, 1.6, 0.0], 20.0, millis(1100)), &first));
    assert!(same(&track([0.15, 1.6, 0.0], 0.0, millis(1200)), &first));
    // A pitch is no dimension of an upright follow, so a look down moves nothing
    ball.space.track(
        &Poses {
            head: Some(Pose {
                position: [0.0, 1.6, 0.0],
                orientation: turn_degrees(-60.0, 0.0, 0.0),
            }),
            controllers: [None, None],
            ..Poses::default()
        },
        millis(1300),
        None,
    );
    assert!(same(&ball.slot().pose, &first));

    // A turn of 40 degrees holds the ball for the start delay of 200 ms
    assert!(same(&track([0.0, 1.6, 0.0], 40.0, millis(2000)), &first));
    assert!(same(&track([0.0, 1.6, 0.0], 40.0, millis(2150)), &first));
    // Then the ball covers half of its way in each half life of 175 ms: a frame 175 ms after the
    // last one puts it halfway
    let target = ahead_of([0.0, 1.6, 0.0], 40.0);
    let halfway = track([0.0, 1.6, 0.0], 40.0, millis(2325)).position;
    let middle = [0, 1, 2].map(|axis| (first.position[axis] + target.position[axis]) / 2.0);
    let off = (0..3)
        .map(|axis| (halfway[axis] - middle[axis]).abs())
        .fold(0.0, f32::max);
    assert!(off < 1e-3, "{halfway:?} {middle:?}");

    // Frames of 11 ms bring it to the target, where it stops
    let mut time = 2325;
    while !same(&ball.slot().pose, &target) {
        time += 11;
        assert!(time < 5000, "the ball reached no rest");
        track([0.0, 1.6, 0.0], 40.0, millis(time));
    }
    assert!(time > 2325 + 3 * 175, "{time}");
    // At rest it holds still again for a small turn
    assert!(same(
        &track([0.0, 1.6, 0.0], 50.0, millis(time + 500)),
        &target
    ));
    assert_eq!(ball.slot().tracked, None);
}

#[test]
fn a_soft_subspace_stands_at_its_target_at_once_after_a_recenter_and_after_a_lost_controller() {
    let ball = ball(FollowTarget::View(FollowMode::soft(
        TrackedDimensions::TranslationAndYaw,
    )));
    ball.space.track(
        &Poses {
            head: Some(head([0.0; 3], 0.0)),
            controllers: [None, None],
            ..Poses::default()
        },
        seconds(1),
        None,
    );
    let recenter = Recenter {
        change_time: seconds(2),
        pose_in_previous_space: None,
    };
    ball.space.track(
        &Poses {
            head: Some(head([1.0, 0.0, 0.0], 90.0)),
            controllers: [None, None],
            ..Poses::default()
        },
        seconds(2),
        Some(recenter),
    );
    assert!(same(&ball.slot().pose, &ahead_of([1.0, 0.0, 0.0], 90.0)));

    let ball = self::ball(FollowTarget::Controller(
        Hand::Right,
        FollowMode::soft(TrackedDimensions::TranslationAndYaw),
    ));
    ball.track(
        [None, Some(controller([0.0, 1.0, 0.0], 0.0, 0.0, 0.0))],
        seconds(1),
    );
    assert!(upright_at(&ball.slot(), [0.0, 1.1, 0.0], 0.0));
    ball.track([None, None], seconds(2));
    assert_eq!(ball.slot().ride, Ride::Hidden);
    ball.track(
        [None, Some(controller([3.0, 1.0, 0.0], 0.0, 0.0, 0.0))],
        seconds(3),
    );
    assert_eq!(ball.slot().ride, Ride::On(Hand::Right));
    assert!(upright_at(&ball.slot(), [3.0, 1.1, 0.0], 0.0));
}

// A right hand whose palm joint stands at `palm`, tracked or not.
fn right_hand_with_palm(palm: Pose, tracking: TrackingState, valid: bool) -> [HandState; 2] {
    let mut right = HandState {
        tracking_state: tracking,
        ..HandState::default()
    };
    let joint = &mut right.joints[HandJointType::Palm.index()];
    joint.pose = palm;
    joint.valid = valid;
    joint.tracked = valid;
    [HandState::default(), right]
}

#[test]
fn a_subspace_of_a_hand_joint_follows_the_palm_and_hides_while_the_hand_is_not_tracked() {
    let ball = ball(FollowTarget::Hand(
        Hand::Right,
        HandJointType::Palm,
        FollowMode::Tight(TrackedDimensions::All),
    ));
    let track = |hands: [HandState; 2], time: Duration| {
        ball.space.track(
            &Poses {
                hands,
                ..Poses::default()
            },
            time,
            None,
        );
        ball.slot()
    };
    // No hand yet: the ball shows nowhere
    assert_eq!(
        track([HandState::default(); 2], seconds(1)).ride,
        Ride::Hidden
    );
    // A palm that faces up, its back down: the ball stands 10 cm off the back of the hand, along
    // the joint's +y, and the right hand's own pointer passes through it
    let palm = Pose {
        position: [0.2, 1.0, -0.4],
        orientation: turn_degrees(180.0, 0.0, 0.0),
    };
    let slot = track(
        right_hand_with_palm(palm, TrackingState::Tracking, true),
        seconds(2),
    );
    assert_eq!(slot.ride, Ride::On(Hand::Right));
    assert!(
        same(&slot.pose, &palm.then(&Pose::at([0.0, 0.1, 0.0]))),
        "{slot:?}"
    );
    assert!((slot.pose.position[1] - 0.9).abs() < 1e-4, "{slot:?}");
    // A joint without a valid pose, and a hand that the cameras lost, hide it
    let invalid = right_hand_with_palm(palm, TrackingState::Tracking, false);
    assert_eq!(track(invalid, seconds(3)).ride, Ride::Hidden);
    let lost = right_hand_with_palm(palm, TrackingState::Paused, true);
    assert_eq!(track(lost, seconds(4)).ride, Ride::Hidden);
    // The other hand's joints do not move it
    let moved = Pose::at([-0.3, 1.2, -0.5]);
    let tracked = right_hand_with_palm(moved, TrackingState::Tracking, true);
    assert!(same(
        &track(tracked, seconds(5)).pose,
        &moved.then(&Pose::at([0.0, 0.1, 0.0]))
    ));
    let mut left_only = tracked;
    left_only.swap(0, 1);
    assert_eq!(track(left_only, seconds(6)).ride, Ride::Hidden);
}
