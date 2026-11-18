//! The pose of the flat quad that shows a beam.

use crate::{
    Pose,
    pose::turn_of_axes,
    ray::Beam,
    vector::{add, cross, dot, normalize, perpendicular, scale, sub},
};

// The pose of a quad that shows `beam` to a viewer at `head`. The quad lies in its local XY
// plane and faces its +Z. Its +Y runs along the beam. Its +Z is the way from the beam's middle
// to the head with the part along the beam removed, so the flat quad turns its face to the
// viewer. Its center is the beam's middle.
pub(crate) fn quad_pose(beam: &Beam, head: [f32; 3]) -> Pose {
    let along = beam.ray.direction;
    let middle = add(beam.ray.origin, scale(along, beam.length / 2.0));
    let to_head = sub(head, middle);
    let across = perpendicular(to_head, along);
    // A head on the beam's line leaves no side to face: any direction across the beam does
    let facing = if dot(across, across) > 1e-8 {
        normalize(across)
    } else {
        let smallest = (0..3).fold(0, |best, axis| {
            if along[axis].abs() < along[best].abs() {
                axis
            } else {
                best
            }
        });
        let mut axis = [0.0; 3];
        axis[smallest] = 1.0;
        normalize(perpendicular(axis, along))
    };
    Pose {
        position: middle,
        orientation: turn_of_axes(cross(along, facing), along, facing),
    }
}

#[cfg(test)]
#[path = "tests/beam_pose_tests.rs"]
mod tests;
