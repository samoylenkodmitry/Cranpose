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
    // The camera's near plane, in meters
    pub(crate) near: f32,
    // The near plane that the layer's depth names, in meters: farther than the camera's by the
    // share that `depth_scale` gives, so the compositor reads each depth that much farther
    pub(crate) depth_near: f32,
}

// How much farther than the scene the layer's depth puts it: one plus the turn between the eye's
// axis and the middle of its vertical field of view, in radians, times the tangent of the field's
// farther vertical edge.
//
// A Quest 3's compositor tests a panel's depth as if it measured along the middle of the eye's
// vertical field of view, 5.5 degrees below the eye's axis, while the projection layer's depth
// measures along the axis, as OpenXR says. A panel below the axis then tests farther than it
// stands, by about the turn times its distance below the axis. On a Quest 3, 45 cm ahead, a key 6
// mm behind a panel hid it from 11 cm below the axis down, and a key 24 mm behind from 24 cm. A
// depth measured along the turned axis showed nearly all of them. Pitching the layer's views to
// that middle changed nothing, so the compositor turns from whatever view the layer names. Over a field of view the error stays
// below the turn times the tangent of the field's edge times the distance, so a depth read that
// much farther covers it: 14% on a Quest 3. A field of view that is as high as it is low gives
// no turn and no margin. A 3D object that stands nearer than a panel by more than the share
// still covers it.
pub(crate) fn depth_scale(view: &FieldOfView) -> f32 {
    let turn = ((view.angle_up + view.angle_down) / 2.0).abs();
    let edge = view.angle_up.tan().max(-view.angle_down.tan());
    1.0 + turn * edge
}

// The part of an eye's view that an image shows: the tangents of its left and its top edge, and
// its pixels for a unit of tangent. The image's size gives its right and its bottom edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ImageRegion {
    pub(crate) left: f32,
    pub(crate) top: f32,
    pub(crate) focal_length: f32,
}

impl ImageRegion {
    // The region of an image of `size` pixels that holds the whole of `view`, with the view's
    // center in the image's middle, or `None` while the view is empty or not finite. The image
    // reaches past the view on the side where it has pixels to spare, because the camera has one
    // focal length for both directions.
    pub(crate) fn whole(view: &FieldOfView, size: (u32, u32)) -> Option<Self> {
        let (left, right) = (view.angle_left.tan(), view.angle_right.tan());
        let (up, down) = (view.angle_up.tan(), view.angle_down.tan());
        let (width, height) = (size.0 as f32, size.1 as f32);
        let focal_length = (width / (right - left)).min(height / (up - down));
        let finite = [left, right, up, down].iter().all(|edge| edge.is_finite());
        if !(finite && focal_length.is_finite() && focal_length > 0.0) {
            return None;
        }
        Some(Self {
            left: (left + right) / 2.0 - width / 2.0 / focal_length,
            top: (up + down) / 2.0 + height / 2.0 / focal_length,
            focal_length,
        })
    }

    // The field of view of an image of `size` pixels.
    pub(crate) fn field_of_view(&self, size: (u32, u32)) -> FieldOfView {
        FieldOfView {
            angle_left: self.left.atan(),
            angle_right: (self.left + size.0 as f32 / self.focal_length).atan(),
            angle_up: self.top.atan(),
            angle_down: (self.top - size.1 as f32 / self.focal_length).atan(),
        }
    }
}

impl EyeCamera {
    // The camera of `eye` for an image of `size` pixels that shows the eye's whole view, or `None`
    // while the eye's field of view is empty or not finite. The headset draws through the regions
    // that `projection_crop.rs` gives, which this is one of.
    #[cfg(test)]
    pub(crate) fn of(eye: &RenderViewpoint, size: (u32, u32), first_row: FirstRow) -> Option<Self> {
        let region = ImageRegion::whole(&eye.field_of_view, size)?;
        Some(Self::of_region(eye, (region, size), size, first_row))
    }

    // The camera of `eye` for an image of `image.1` pixels that shows `image.0`, the region of the
    // eye's view. The scene lays out in a box of `scene` pixels and draws into the image from the
    // box's top left corner, so the camera puts the region's first row and first column there. A
    // scene's camera has its image center in the middle of its box: the camera shears x and y by z
    // so that the center of its box shows the tangents that lie there.
    pub(crate) fn of_region(
        eye: &RenderViewpoint,
        (region, image): (ImageRegion, (u32, u32)),
        scene: (u32, u32),
        first_row: FirstRow,
    ) -> Self {
        let focal_length = region.focal_length;
        let field_of_view = region.field_of_view(image);
        let (half_width, half_height) = (
            scene.0 as f32 / 2.0 / focal_length,
            scene.1 as f32 / 2.0 / focal_length,
        );
        // The image's first row is its top row, or its bottom row for a GL swapchain
        let first = match first_row {
            FirstRow::Top => region.top - half_height,
            FirstRow::Bottom => field_of_view.angle_down.tan() + half_height,
        };
        let center = [region.left + half_width, first];
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
        let near = camera.near() / UNITS_PER_METER;
        Self {
            near,
            depth_near: near * depth_scale(&eye.field_of_view),
            camera,
            field_of_view,
        }
    }

    // The matrix, by rows, that takes a point of the room in meters to the clip space of an image
    // of `size` pixels, as the scene's camera shows it. The depth that it gives is the scene's
    // own: `1 - near / z` for a point at distance `z` along the camera's axis.
    pub(crate) fn clip_from_room(&self, size: (u32, u32)) -> [[f32; 4]; 4] {
        let view = self.camera.view.rows();
        let scale = [UNITS_PER_METER, -UNITS_PER_METER, -UNITS_PER_METER, 1.0];
        let row = |index: usize, factor: f32| -> [f32; 4] {
            std::array::from_fn(|column| view[index][column] * scale[column] * factor)
        };
        let focal = self.camera.focal_length;
        let near = self.camera.near();
        let depth = row(2, 1.0);
        let w = row(3, 1.0);
        [
            row(0, 2.0 * focal / size.0 as f32),
            row(1, -2.0 * focal / size.1 as f32),
            std::array::from_fn(|column| depth[column] - near * w[column]),
            depth,
        ]
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
