//! The order in which flat layers draw: the farthest from the head first, so that a nearer layer
//! covers a farther one.

use crate::Pose;

// How far the head is from the plane of the layer at `layer`, along the layer's normal. The
// distance to the layer's center would put a small panel at the edge of a large surface behind
// that surface, because the edge lies farther from the head than the center does. The plane's
// distance ignores where on the plane the layer sits, and it does not change when the head
// turns.
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
// others. The sort neither allocates nor keeps a list between frames. Layers of one distance,
// such as the two eyes of a stereo surface, may change their places among themselves.
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
