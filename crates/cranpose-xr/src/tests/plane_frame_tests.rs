use super::*;
use crate::pose::turn_degrees;

fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
}

// The pose of an entity of Meta's scene model: its +Z axis is the normal.
fn entity(position: [f32; 3], pitch: f32, yaw: f32) -> Pose {
    Pose {
        position,
        orientation: turn_degrees(pitch, yaw, 0.0),
    }
}

#[test]
fn a_floor_whose_z_axis_points_up_keeps_its_frame_and_faces_up() {
    // Turning an entity by -90 degrees about X puts its +Z axis along the room's +Y axis
    let space = entity([0.0, -1.5, 0.0], -90.0, 0.0);
    let frame = PlaneFrame::of(&space, [-1.0, -2.0], [2.0, 4.0], PlaneLabel::Floor);
    assert_eq!(frame.plane_type, PlaneType::HorizontalUpwardFacing);
    let center = frame.center_pose(&space);
    assert!(
        near(center.rotate([0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]),
        "{center:?}"
    );
    assert_eq!(frame.extents, [2.0, 4.0]);
}

#[test]
fn a_floor_whose_z_axis_points_down_turns_half_a_turn_to_face_up() {
    let space = entity([0.0, -1.5, 0.0], 90.0, 0.0);
    let frame = PlaneFrame::of(&space, [-1.0, -2.0], [2.0, 4.0], PlaneLabel::Floor);
    assert_eq!(frame.plane_type, PlaneType::HorizontalUpwardFacing);
    let center = frame.center_pose(&space);
    assert!(
        near(center.rotate([0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]),
        "{center:?}"
    );
}

#[test]
fn a_ceiling_faces_down_whichever_way_the_runtime_points_its_z_axis() {
    for pitch in [-90.0, 90.0] {
        let space = entity([0.0, 1.0, 0.0], pitch, 0.0);
        let frame = PlaneFrame::of(&space, [-1.0, -1.0], [2.0, 2.0], PlaneLabel::Ceiling);
        assert_eq!(
            frame.plane_type,
            PlaneType::HorizontalDownwardFacing,
            "{pitch}"
        );
        let normal = frame.center_pose(&space).rotate([0.0, 1.0, 0.0]);
        assert!(near(normal, [0.0, -1.0, 0.0]), "{pitch}: {normal:?}");
    }
}

#[test]
fn a_wall_keeps_its_normal_and_its_type_with_no_label_to_correct_it() {
    // An entity that stands on the wall at z = -3, its +Z axis toward the room
    let space = entity([0.0, 1.0, -3.0], 0.0, 0.0);
    let frame = PlaneFrame::of(&space, [-1.0, -1.0], [2.0, 2.0], PlaneLabel::Wall);
    assert_eq!(frame.plane_type, PlaneType::Vertical);
    let normal = frame.center_pose(&space).rotate([0.0, 1.0, 0.0]);
    assert!(near(normal, [0.0, 0.0, 1.0]), "{normal:?}");
}

#[test]
fn the_center_is_the_middle_of_the_rectangle_and_the_polygon_lies_around_it() {
    let space = entity([0.0, 0.0, 0.0], -90.0, 0.0);
    let frame = PlaneFrame::of(&space, [1.0, 2.0], [4.0, 2.0], PlaneLabel::Table);
    // The rectangle's middle is at (3, 3) in the entity's X-Y plane. Turned to lie flat, the entity's
    // Y axis points along -Z of the room, so the middle is 3 meters toward -Z
    let center = frame.center_pose(&space);
    assert!(near(center.position, [3.0, 0.0, -3.0]), "{center:?}");
    // The corner at the minimum is half an extent from the middle along each plane axis
    let corner = frame.vertex([1.0, 2.0]);
    assert!(
        (corner[0] + 2.0).abs() < 1e-5 && (corner[1] - 1.0).abs() < 1e-5,
        "{corner:?}"
    );
}
