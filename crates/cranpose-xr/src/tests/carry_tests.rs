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
fn the_handle_catches_a_ray_from_any_side_and_misses_a_ray_beside_it() {
    let slot = slot();
    // The panel is 0.5 m high and 2 m ahead: its bottom edge is at y = -0.25, and the handle's
    // center is 4 cm under it.
    let center = handle_center(&slot);
    assert!(close(center, [0.0, -0.29, -2.0]));
    let ray = |origin: [f32; 3], direction: [f32; 3]| Ray { origin, direction };
    // From the front, looking at the panel
    let hit = handle_hit(&ray([0.0, -0.29, 0.0], [0.0, 0.0, -1.0]), &slot).expect("front");
    assert!(
        (hit - (2.0 - 0.035)).abs() < 1e-4,
        "the grab sphere's near side"
    );
    // From behind the panel, looking back at it
    let hit = handle_hit(&ray([0.0, -0.29, -4.0], [0.0, 0.0, 1.0]), &slot).expect("behind");
    assert!((hit - (2.0 - 0.035)).abs() < 1e-4);
    // From below, from the side and from above at a slant
    assert!(handle_hit(&ray([0.0, -1.29, -2.0], [0.0, 1.0, 0.0]), &slot).is_some());
    assert!(handle_hit(&ray([1.0, -0.29, -2.0], [-1.0, 0.0, 0.0]), &slot).is_some());
    let slant = [0.0, -0.6, -0.8];
    assert!(handle_hit(&ray([0.0, 0.31, -1.2], slant), &slot).is_some());
    // A ray inside the margin meets the handle at once
    assert_eq!(
        handle_hit(&ray([0.0, -0.29, -2.0], [0.0, 0.0, -1.0]), &slot),
        Some(0.0)
    );
    // Misses: the panel itself, beside the margin, far to the side, and a handle behind the ray
    assert_eq!(
        handle_hit(&ray([0.0, 0.0, 0.0], [0.0, 0.0, -1.0]), &slot),
        None,
        "the panel itself"
    );
    assert_eq!(
        handle_hit(&ray([0.0, -0.29 - 0.04, 0.0], [0.0, 0.0, -1.0]), &slot),
        None,
        "4 cm beside the center, past the 3.5 cm grab"
    );
    assert_eq!(
        handle_hit(&ray([1.0, -0.29, 0.0], [0.0, 0.0, -1.0]), &slot),
        None
    );
    assert_eq!(
        handle_hit(&ray([0.0, -0.29, -4.0], [0.0, 0.0, -1.0]), &slot),
        None,
        "the handle lies behind the ray"
    );
}
