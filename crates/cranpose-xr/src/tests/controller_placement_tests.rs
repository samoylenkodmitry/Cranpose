// Where a model that has its origin at a controller's grip lies in the scene.

use super::*;
use crate::{eye_camera::scene_direction, pose::turn_degrees};

fn near(a: Vec3, b: Vec3) {
    assert!((a - b).length() < 1e-2, "{a:?} is not {b:?}");
}

// Where the room puts a point of the model, which is `p` meters from the grip, in the scene's
// units: through the grip's pose into the room, then into the scene's axes and millimeters.
fn expected(grip: &Pose, p: [f32; 3]) -> Vec3 {
    let moved = grip.rotate(p);
    let room = std::array::from_fn(|axis| grip.position[axis] + moved[axis]);
    scene_direction(room) * UNITS_PER_METER
}

#[test]
fn a_controller_at_the_origin_leaves_the_models_meters_as_millimeters_in_the_scenes_axes() {
    // The model has y up and z toward the user, and the scene has y down and z away
    let map = scene_from_grip(&Pose::IDENTITY);
    near(
        map.transform_point(Vec3::new(0.01, 0.02, 0.03)),
        Vec3::new(10.0, -20.0, -30.0),
    );
}

#[test]
fn the_grip_origin_lies_where_the_grip_is() {
    let grip = Pose::at([-0.25, 1.1, -0.4]);
    near(
        scene_from_grip(&grip).transform_point(Vec3::ZERO),
        Vec3::new(-250.0, -1100.0, 400.0),
    );
}

#[test]
fn a_point_ahead_of_the_grip_along_its_forward_axis_lies_ahead_in_the_room() {
    // The grip's -z is its forward axis. A turn of 90 degrees to the left takes it to the room's -x.
    let grip = Pose {
        position: [0.0, 1.0, -0.5],
        orientation: turn_degrees(0.0, 90.0, 0.0),
    };
    let tip = scene_from_grip(&grip).transform_point(Vec3::new(0.0, 0.0, -0.1));
    near(tip, expected(&grip, [0.0, 0.0, -0.1]));
    // The room's -x is the scene's -x, 100 mm along from the grip at x = 0
    near(tip, Vec3::new(-100.0, -1000.0, 500.0));
}

#[test]
fn every_point_of_a_tilted_model_lies_where_the_grip_puts_it() {
    let grip = Pose {
        position: [0.3, 0.9, -0.2],
        orientation: turn_degrees(35.0, -50.0, 20.0),
    };
    let map = scene_from_grip(&grip);
    for p in [
        [0.0, 0.0, 0.0],
        [0.05, 0.0, 0.0],
        [0.0, 0.05, 0.0],
        [0.0, 0.0, 0.05],
        [-0.03, 0.04, -0.07],
    ] {
        near(map.transform_point(Vec3::from_array(p)), expected(&grip, p));
    }
}

#[test]
fn the_map_keeps_the_models_size_and_its_handedness() {
    let grip = Pose {
        position: [0.1, 1.2, -0.3],
        orientation: turn_degrees(10.0, 70.0, -15.0),
    };
    let map = scene_from_grip(&grip);
    // A meter of the model is a thousand units of the scene
    assert!((map.length_scale() - UNITS_PER_METER).abs() < 1e-2);
    // The model's axes keep a right-handed frame in the scene, whose axes are right-handed too: x
    // cross y is z
    let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        .map(|axis| map.transform_vector(Vec3::from_array(axis)) * (1.0 / UNITS_PER_METER));
    near(axes[0].cross(axes[1]), axes[2]);
}
