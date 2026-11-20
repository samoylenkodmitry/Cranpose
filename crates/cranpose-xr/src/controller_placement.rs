//! Where a controller's model stands in the scene that draws the volumes.

use cranpose::space::{Mat4, Vec3};

use crate::{Pose, eye_camera::UNITS_PER_METER, volume_transform::scene_from_volume};

// The map from a model's own space to the scene, for a model whose origin is a controller's grip
// at `grip` in the room.
//
// The runtime's models are glTF files in meters with x to the right, y up and z toward the user.
// The scene's content space has millimeters, y down and z away, and `scene_from_volume` carries
// the content space of a volume at a pose to the scene. A half turn about x takes the glTF axes
// to the content's, so the two half turns cancel in the room: a point of the model that lies `p`
// from the origin lies where `grip` puts `p`.
pub(crate) fn scene_from_grip(grip: &Pose) -> Mat4 {
    Mat4::scaling(Vec3::new(
        UNITS_PER_METER,
        -UNITS_PER_METER,
        -UNITS_PER_METER,
    ))
    .then(&scene_from_volume(grip))
}

#[cfg(test)]
#[path = "tests/controller_placement_tests.rs"]
mod tests;
