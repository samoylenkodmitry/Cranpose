//! The pose of a flat quad that turns its face to the viewer from any side.

use crate::{
    Pose,
    pose::turn_of_axes,
    vector::{cross, dot, normalize, scale, sub},
};

// The pose of a quad centered on `center` that shows its picture to a viewer at `head`. The quad
// lies in its local XY plane and faces its +Z, which points at the head. Its +Y points up as far
// as that facing lets it, so a picture of a lit ball keeps its light on top when the viewer
// walks around.
pub(crate) fn billboard_pose(center: [f32; 3], head: [f32; 3]) -> Pose {
    let to_head = sub(head, center);
    // A head at the center has no side to face: the quad faces the way it faces by default
    let facing = if dot(to_head, to_head) > 1e-12 {
        normalize(to_head)
    } else {
        [0.0, 0.0, 1.0]
    };
    // The way up with the part along the facing removed
    let up = sub([0.0, 1.0, 0.0], scale(facing, facing[1]));
    // A head straight above or below leaves no up: the way ahead does as well
    let top = if dot(up, up) > 1e-8 {
        normalize(up)
    } else {
        normalize(sub([0.0, 0.0, -1.0], scale(facing, -facing[2])))
    };
    Pose {
        position: center,
        orientation: turn_of_axes(cross(top, facing), top, facing),
    }
}

#[cfg(test)]
#[path = "tests/billboard_pose_tests.rs"]
mod tests;
