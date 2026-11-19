use cranpose::space::Vec3;

use super::*;
use crate::pose::turn_degrees;

fn near(a: Vec3, b: Vec3) {
    assert!((a - b).length() < 1e-3, "{a:?} is not {b:?}");
}

#[test]
fn a_volume_at_the_origin_leaves_its_content_in_the_scenes_axes() {
    let content = Vec3::new(100.0, 50.0, 200.0);
    near(
        scene_from_volume(&Pose::IDENTITY).transform_point(content),
        content,
    );
}

#[test]
fn a_volume_a_meter_ahead_moves_its_content_a_thousand_units_away() {
    // The room's -z is ahead, and the scene's z is away
    let volume = scene_from_volume(&Pose::at([0.0, 0.0, -1.0]));
    near(
        volume.transform_point(Vec3::ZERO),
        Vec3::new(0.0, 0.0, 1000.0),
    );
    near(
        volume.transform_point(Vec3::new(10.0, 20.0, 30.0)),
        Vec3::new(10.0, 20.0, 1030.0),
    );
}

#[test]
fn a_volume_above_the_floor_lifts_its_content_up_the_scenes_negative_y() {
    let volume = scene_from_volume(&Pose::at([0.0, 1.5, 0.0]));
    near(
        volume.transform_point(Vec3::ZERO),
        Vec3::new(0.0, -1500.0, 0.0),
    );
}

#[test]
fn a_turned_volume_turns_its_content_about_its_own_center() {
    // A quarter turn to the left takes the volume's way away, which is the room's -z, to -x
    let pose = Pose {
        position: [0.0, 0.0, -2.0],
        orientation: turn_degrees(0.0, 90.0, 0.0),
    };
    let volume = scene_from_volume(&pose);
    let center = Vec3::new(0.0, 0.0, 2000.0);
    near(volume.transform_point(Vec3::ZERO), center);
    near(
        volume.transform_point(Vec3::new(0.0, 0.0, 100.0)),
        center + Vec3::new(-100.0, 0.0, 0.0),
    );
    // Right stays a right-handed turn of the axes: the content's right goes away from the viewer
    near(
        volume.transform_point(Vec3::new(100.0, 0.0, 0.0)),
        center + Vec3::new(0.0, 0.0, 100.0),
    );
    // Down stays down
    near(
        volume.transform_point(Vec3::new(0.0, 100.0, 0.0)),
        center + Vec3::new(0.0, 100.0, 0.0),
    );
}

#[test]
fn a_direction_of_the_content_turns_with_the_volume() {
    let pose = Pose {
        position: [3.0, 2.0, 1.0],
        orientation: turn_degrees(0.0, 90.0, 0.0),
    };
    // A light that travels away from the viewer travels along the scene's -x
    near(
        scene_from_volume(&pose).transform_vector(Vec3::new(0.0, 0.0, 1.0)),
        Vec3::new(-1.0, 0.0, 0.0),
    );
}
