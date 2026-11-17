use super::*;

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5)
}

#[test]
fn a_yaw_of_ninety_degrees_turns_forward_to_the_left() {
    let pose = Pose {
        position: [0.0; 3],
        orientation: turn_degrees(0.0, 90.0, 0.0),
    };
    assert!(close(pose.rotate([0.0, 0.0, -1.0]), [-1.0, 0.0, 0.0]));
    assert!(close(pose.unrotate([-1.0, 0.0, 0.0]), [0.0, 0.0, -1.0]));
}

#[test]
fn a_positive_pitch_turns_forward_up() {
    let pose = Pose {
        position: [0.0; 3],
        orientation: turn_degrees(30.0, 0.0, 0.0),
    };
    let forward = pose.rotate([0.0, 0.0, -1.0]);
    assert!(forward[1] > 0.49 && forward[1] < 0.51);
}

#[test]
fn a_child_pose_moves_with_its_parent_turn() {
    let parent = Pose {
        position: [1.0, 0.0, 0.0],
        orientation: turn_degrees(0.0, 90.0, 0.0),
    };
    let child = parent.then(&Pose::at([0.0, 0.0, -2.0]));
    assert!(close(child.position, [-1.0, 0.0, 0.0]));
    assert!(close(child.rotate([0.0, 0.0, -1.0]), [-1.0, 0.0, 0.0]));
}
