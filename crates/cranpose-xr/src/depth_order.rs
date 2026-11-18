//! The order in which panels draw: the farthest from the head first, so that a nearer panel covers
//! a farther one.

use crate::Pose;

// How far the head is from the plane of the panel at `layer`, along the panel's normal. The
// distance to the panel's center would put a small panel at the edge of a large surface behind
// that surface, because the edge lies farther from the head than the center does. The plane's
// distance ignores where on the plane the panel sits, and it does not change when the head
// turns.
//
// A curved panel sorts by the plane at its center. Its arc leaves that plane by about
// `arc_length^2 / (8 * radius)`, which is 2 cm for a panel of 0.4 m on a cylinder of 1 m. A
// measure that follows the arc changes when the head steps aside. The distance to the arc's
// nearest point puts a small curved panel at the edge of a large surface behind that surface.
// The distance to the tangent plane at the head's angle sinks a flat panel 3 cm in front of a
// curved panel under it when the head steps 20 cm aside.
fn plane_distance(head: [f32; 3], layer: &Pose) -> f32 {
    let normal = layer.rotate([0.0, 0.0, 1.0]);
    let from_layer = [
        head[0] - layer.position[0],
        head[1] - layer.position[1],
        head[2] - layer.position[2],
    ];
    (normal[0] * from_layer[0] + normal[1] * from_layer[1] + normal[2] * from_layer[2]).abs()
}

// Sorts `layers` in place from the one whose plane lies farthest from `head` to the one whose
// plane lies nearest. The compositor draws layers in list order, so the last one covers the
// others. The sort neither allocates nor keeps a list between frames. Layers of one distance may
// change their places among themselves.
pub(crate) fn sort_back_to_front<T>(
    layers: &mut [T],
    head: [f32; 3],
    pose_of: impl Fn(&T) -> &Pose,
) {
    layers.sort_unstable_by(|a, b| {
        plane_distance(head, pose_of(b)).total_cmp(&plane_distance(head, pose_of(a)))
    });
}

#[cfg(test)]
#[path = "tests/depth_order_tests.rs"]
mod tests;
