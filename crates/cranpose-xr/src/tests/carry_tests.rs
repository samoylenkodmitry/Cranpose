use super::*;
use crate::space::{Mobility, PanelKind, Shape};

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4)
}

fn turned(position: [f32; 3], pitch: f32, yaw: f32, roll: f32) -> Pose {
    Pose {
        position,
        orientation: turn_degrees(pitch, yaw, roll),
    }
}

// A panel of 800 by 500 dp, which is 0.8 by 0.5 meters, two meters ahead.
fn slot() -> PanelSlot {
    PanelSlot {
        id: 1,
        kind: PanelKind::Content,
        atlas: [0.0, 0.0, 800.0, 500.0],
        pose: Pose::at([0.0, 0.0, -2.0]),
        mobility: Mobility::Movable,
        shape: Shape::Flat,
    }
}

#[test]
fn a_carried_panel_follows_a_translated_aim() {
    let panel = Pose::at([0.0, 0.0, -1.5]);
    let carry = Carry::grab(&Pose::IDENTITY, &panel);
    let moved = carry.pose(&Pose::at([0.5, 0.2, 0.1]));
    assert!(close(moved.position, [0.5, 0.2, -1.4]));
    assert!(close(moved.rotate([0.0, 0.0, 1.0]), [0.0, 0.0, 1.0]));
}

#[test]
fn a_yaw_of_the_aim_turns_the_panel_about_the_aim() {
    let aim = Pose::at([1.0, 1.0, 0.0]);
    let panel = Pose::at([1.0, 1.0, -1.5]);
    let carry = Carry::grab(&aim, &panel);
    let turned_aim = turned([1.0, 1.0, 0.0], 0.0, 90.0, 0.0);
    let moved = carry.pose(&turned_aim);
    // A yaw of 90 degrees turns forward to the left.
    assert!(close(moved.position, [-0.5, 1.0, 0.0]));
    assert!(close(moved.rotate([0.0, 0.0, 1.0]), [1.0, 0.0, 0.0]));
}

#[test]
fn a_slight_roll_snaps_the_panel_level() {
    // The panel's normal is the aim's z, so a roll of the aim is a roll of the panel.
    let carry = Carry::grab(&Pose::IDENTITY, &Pose::at([0.0, 0.0, -1.5]));
    let axes = |roll: f32| {
        let pose = carry.pose(&turned([0.0; 3], 10.0, 30.0, roll));
        (pose.rotate([1.0, 0.0, 0.0]), pose.rotate([0.0, 0.0, 1.0]))
    };
    let (level_x, level_normal) = axes(0.0);
    let (x, normal) = axes(3.0);
    assert!(x[1].abs() < 1e-4, "the panel's +x is horizontal");
    assert!(close(x, level_x) && close(normal, level_normal));
    let (x, _) = axes(10.0);
    assert!(x[1].abs() > 0.1, "a roll of 10 degrees stays");
}

#[test]
fn a_push_moves_the_panel_away_within_limits() {
    let mut carry = Carry::grab(&Pose::IDENTITY, &Pose::at([0.0, 0.0, -1.0]));
    let mut distance = |push: f32| {
        carry.push(push);
        carry.pose(&Pose::IDENTITY).position[2]
    };
    assert!(close([distance(0.5), 0.0, 0.0], [-1.5, 0.0, 0.0]));
    assert!(close([distance(-10.0), 0.0, 0.0], [-0.3, 0.0, 0.0]));
    // A push back out of the near limit takes effect at once
    assert!(close([distance(0.2), 0.0, 0.0], [-0.5, 0.0, 0.0]));
    assert!(close([distance(10.0), 0.0, 0.0], [-5.0, 0.0, 0.0]));
}

#[test]
fn the_handle_catches_a_ray_under_the_panel_and_misses_a_far_one() {
    let slot = slot();
    let (pose, size) = handle_pose(&slot);
    assert!(close(pose.position, [0.0, -0.277, -2.0]));
    assert_eq!(size, [0.12, 0.014]);
    let ahead = |x: f32, y: f32| Ray {
        origin: [x, y, 0.0],
        direction: [0.0, 0.0, -1.0],
    };
    let hit = handle_hit(&ahead(0.0, -0.3), &slot).expect("just under the bar");
    assert!((hit - 2.0).abs() < 1e-4);
    assert_eq!(
        handle_hit(&ahead(0.0, 0.0), &slot),
        None,
        "the panel itself"
    );
    assert_eq!(handle_hit(&ahead(0.0, -0.5), &slot), None);
    assert_eq!(handle_hit(&ahead(1.0, -0.28), &slot), None);
}
