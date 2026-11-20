//! The camera that draws one eye of the headset into a scene of Cranpose's 3D.
//!
//! A scene's camera is a pinhole whose image center is the middle of its box, and a unit of its
//! space is a dp, which is a millimeter in a subspace. Its space has x to the right, y down and
//! z away from the viewer, where OpenXR's room has x to the right, y up and z toward the viewer.
//! An eye's display is off center: the left eye reaches farther to the left than to the right.
//! The camera shears x and y by z, which moves the image center by the same amount at every
//! depth. The shear keeps the pinhole camera and the scene's box.

use cranpose::space::{Camera, Mat4, Vec3};

use crate::{FieldOfView, Pose, RenderViewpoint, vector::dot};

// Scene units in a meter: a dp is a millimeter.
pub(crate) const UNITS_PER_METER: f32 = 1000.0;

// Which end of an image its first row is. A GL swapchain's first row is its bottom one, and the
// layer cannot say so for a projection, so the picture is drawn upside down. A Vulkan image
// starts with its top row, which is how wgpu draws it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FirstRow {
    Top,
    Bottom,
}

// How one eye sees the scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EyeCamera {
    // The camera that draws the eye's image
    pub(crate) camera: Camera,
    // The field of view that the image covers, which the layer submits with the image. It holds
    // the eye's own and reaches farther on the side where the image has pixels to spare, because
    // the camera has one focal length for both directions.
    pub(crate) field_of_view: FieldOfView,
    // The camera's near plane, in meters, which the layer's depth says
    pub(crate) near: f32,
}

impl EyeCamera {
    // The camera of `eye` for an image of `size` pixels, or `None` while the eye's field of view
    // is empty or not finite.
    pub(crate) fn of(eye: &RenderViewpoint, size: (u32, u32), first_row: FirstRow) -> Option<Self> {
        let view = &eye.field_of_view;
        let (left, right) = (view.angle_left.tan(), view.angle_right.tan());
        let (up, down) = (view.angle_up.tan(), view.angle_down.tan());
        let (width, height) = (size.0 as f32, size.1 as f32);
        let focal_length = (width / (right - left)).min(height / (up - down));
        let finite = [left, right, up, down].iter().all(|edge| edge.is_finite());
        if !(finite && focal_length.is_finite() && focal_length > 0.0) {
            return None;
        }
        // The view's center, in tangents. The image center sits there.
        let center = [(left + right) / 2.0, (up + down) / 2.0];
        let half = [width / 2.0 / focal_length, height / 2.0 / focal_length];
        let [right_axis, down_axis, forward_axis] = camera_axes(&eye.pose);
        let to_scene = |axis: [f32; 3]| {
            let scene = scene_direction(axis);
            [
                scene.x,
                scene.y,
                scene.z,
                -UNITS_PER_METER * dot(axis, eye.pose.position),
            ]
        };
        let forward = to_scene(forward_axis);
        let sheared = |axis: [f32; 3], shear: f32| {
            let row = to_scene(axis);
            std::array::from_fn(|column| row[column] + shear * forward[column])
        };
        let x = sheared(right_axis, -center[0]);
        let mut y = sheared(down_axis, center[1]);
        if first_row == FirstRow::Bottom {
            y = y.map(|value| -value);
        }
        let camera = Camera {
            view: Mat4::from_rows([x, y, forward, [0.0, 0.0, 0.0, 1.0]]),
            focal_length,
        };
        Some(Self {
            near: camera.near() / UNITS_PER_METER,
            camera,
            field_of_view: FieldOfView {
                angle_left: (center[0] - half[0]).atan(),
                angle_right: (center[0] + half[0]).atan(),
                angle_up: (center[1] + half[1]).atan(),
                angle_down: (center[1] - half[1]).atan(),
            },
        })
    }
}

// The camera's right, down and forward axes in the room.
fn camera_axes(pose: &Pose) -> [[f32; 3]; 3] {
    let right = pose.rotate([1.0, 0.0, 0.0]);
    let up = pose.rotate([0.0, 1.0, 0.0]);
    let forward = pose.rotate([0.0, 0.0, -1.0]);
    [right, up.map(|v| -v), forward]
}

// A direction of the room in the scene's axes.
pub(crate) fn scene_direction(direction: [f32; 3]) -> Vec3 {
    Vec3::new(direction[0], -direction[1], -direction[2])
}

#[cfg(test)]
#[path = "tests/eye_camera_tests.rs"]
mod tests;
