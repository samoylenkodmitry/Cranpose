//! Where a volume's content stands in the scene that draws every volume of a headset.
//!
//! A volume's content has its own space: its origin is the middle of the volume's box, x runs to
//! the right, y down and z away from the viewer, and a unit is a dp. The scene is the room in the
//! same kind of units, so one scene holds all volumes and its depth buffer hides a farther
//! volume behind a nearer one.

use cranpose::space::Mat4;

use crate::{
    Pose,
    eye_camera::{UNITS_PER_METER, scene_direction},
};

// The map from the content of a volume at `pose` in the room to the scene. A volume's content
// moves with the volume's pose, so the pose moves the content without composing it again.
pub(crate) fn scene_from_volume(pose: &Pose) -> Mat4 {
    // The content's axes turn into the room's, then by the pose, then into the scene's. The turn
    // between the two conventions is the same half turn about x each time, so it leaves the
    // volume's own turn as it is.
    let axis = |content: [f32; 3]| {
        let room = pose.rotate([content[0], -content[1], -content[2]]);
        scene_direction(room)
    };
    let origin = scene_direction(pose.position) * UNITS_PER_METER;
    Mat4::from_axes(
        axis([1.0, 0.0, 0.0]),
        axis([0.0, 1.0, 0.0]),
        axis([0.0, 0.0, 1.0]),
        origin,
    )
}

#[cfg(test)]
#[path = "tests/volume_transform_tests.rs"]
mod tests;
