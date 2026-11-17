//! The pose of the flat quad that shows a beam.

use crate::{Pose, ray::Beam};

// The pose of a quad that shows `beam` to a viewer at `head`. The quad lies in its local XY
// plane and faces its +Z. Its +Y runs along the beam. Its +Z is the way from the beam's middle
// to the head with the part along the beam removed, so the flat quad turns its face to the
// viewer. Its center is the beam's middle.
pub(crate) fn quad_pose(beam: &Beam, head: [f32; 3]) -> Pose {
    let along = beam.ray.direction;
    let middle = add(beam.ray.origin, scale(along, beam.length / 2.0));
    let to_head = sub(head, middle);
    let across = sub(to_head, scale(along, dot(to_head, along)));
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
        normalize(sub(axis, scale(along, dot(axis, along))))
    };
    Pose {
        position: middle,
        orientation: turn_of_axes(cross(along, facing), along, facing),
    }
}

// The unit quaternion x, y, z, w of the turn that takes the x, y and z axes to `x`, `y` and
// `z`, which are unit vectors at right angles with `x` cross `y` equal to `z`.
fn turn_of_axes(x: [f32; 3], y: [f32; 3], z: [f32; 3]) -> [f32; 4] {
    // The turn's matrix has these axes as its columns: m[row][column]
    let m = [[x[0], y[0], z[0]], [x[1], y[1], z[1]], [x[2], y[2], z[2]]];
    let trace = m[0][0] + m[1][1] + m[2][2];
    // Each branch divides by the largest of w, x, y and z, which keeps the division exact
    if trace > 0.0 {
        let s = 2.0 * (trace + 1.0).sqrt();
        [
            (m[2][1] - m[1][2]) / s,
            (m[0][2] - m[2][0]) / s,
            (m[1][0] - m[0][1]) / s,
            0.25 * s,
        ]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = 2.0 * (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt();
        [
            0.25 * s,
            (m[0][1] + m[1][0]) / s,
            (m[0][2] + m[2][0]) / s,
            (m[2][1] - m[1][2]) / s,
        ]
    } else if m[1][1] > m[2][2] {
        let s = 2.0 * (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt();
        [
            (m[0][1] + m[1][0]) / s,
            0.25 * s,
            (m[1][2] + m[2][1]) / s,
            (m[0][2] - m[2][0]) / s,
        ]
    } else {
        let s = 2.0 * (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt();
        [
            (m[0][2] + m[2][0]) / s,
            (m[1][2] + m[2][1]) / s,
            0.25 * s,
            (m[1][0] - m[0][1]) / s,
        ]
    }
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f32; 3], factor: f32) -> [f32; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(a: [f32; 3]) -> [f32; 3] {
    scale(a, 1.0 / dot(a, a).sqrt())
}

#[cfg(test)]
#[path = "tests/beam_pose_tests.rs"]
mod tests;
