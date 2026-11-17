use super::*;
use crate::{XrFrame, pose::turn_degrees};

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5)
}

#[test]
fn an_eye_is_given_relative_to_a_head_that_stands_and_turns_in_the_room() {
    // The head stands at (1, 2, 3) and turns 90 degrees to the left about the up axis. The turned
    // head's right-hand side points to -z of the room.
    let head = Pose {
        position: [1.0, 2.0, 3.0],
        orientation: turn_degrees(0.0, 90.0, 0.0),
    };
    let right_eye_in_room = Pose::at([1.0, 2.0, 3.0 - 0.032]);
    let eye = RenderViewpoint::seen_from(&head, right_eye_in_room, FieldOfView::default());
    assert_eq!(eye.pose, right_eye_in_room);
    assert!(
        close(eye.local_pose.position, [0.032, 0.0, 0.0]),
        "{:?}",
        eye.local_pose
    );
    // The eye looks along the room's axes, so seen from the head it turns back by the head's turn
    let [x, y, z, w] = eye.local_pose.orientation;
    let half = std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        x.abs() < 1e-5 && z.abs() < 1e-5 && (y + half).abs() < 1e-5 && (w - half).abs() < 1e-5,
        "{x} {y} {z} {w}"
    );
}

#[test]
fn the_interpupillary_distance_is_the_distance_between_the_eyes() {
    let head = Pose::at([0.0, 1.6, 0.0]);
    let eye =
        |x: f32| RenderViewpoint::seen_from(&head, Pose::at([x, 1.6, 0.0]), FieldOfView::default());
    let frame = XrFrame {
        left_eye: Some(eye(-0.032)),
        right_eye: Some(eye(0.032)),
        ..XrFrame::default()
    };
    assert!((frame.interpupillary_distance().expect("both eyes") - 0.064).abs() < 1e-6);
    let blind = XrFrame {
        left_eye: frame.left_eye,
        ..XrFrame::default()
    };
    assert_eq!(blind.interpupillary_distance(), None);
}

#[test]
fn the_refresh_rate_follows_from_the_frame_period() {
    let frame = XrFrame {
        period: std::time::Duration::from_nanos(8_333_333),
        ..XrFrame::default()
    };
    assert!((frame.refresh_rate().expect("a period") - 120.0).abs() < 0.01);
    assert_eq!(XrFrame::default().refresh_rate(), None);
}

#[test]
fn the_floor_height_is_the_height_of_the_stage_in_the_room() {
    let frame = XrFrame {
        stage_pose: Some(Pose::at([0.1, -1.62, 0.3])),
        ..XrFrame::default()
    };
    assert_eq!(frame.floor_height(), Some(-1.62));
    assert_eq!(XrFrame::default().floor_height(), None);
}
