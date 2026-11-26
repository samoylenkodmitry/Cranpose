//! The part of each eye's view that the projection layer of the volumes covers.
//!
//! A projection layer over the whole view stores every pixel of both eyes each frame, while the
//! volumes often fill a small part of it, such as a page of controls a meter ahead. The layer
//! covers instead the box that holds the content of every shown volume and every controller's
//! model, as each eye sees it, with a margin. Its images keep the pixels for a unit of tangent of
//! the whole view, so the content draws as sharp as before. A volume whose content may reach
//! anywhere, a box that reaches behind the eye and the pictures of other layers drawn into the
//! projection take the whole view.
//!
//! The images change their size in steps, and only when the content needs more room or needs much
//! less for a while: a new size makes new swapchains, which the content's size changes too often
//! for. The place of the covered part follows the content each frame at no cost.

use crate::{FieldOfView, Pose, RenderViewpoint, eye_camera::ImageRegion};

// The steps of an image's size: a twelfth of the whole view's width and height
const STEPS: u32 = 12;
// The room that a new size leaves beyond what the content needs, as a share of the need
const HEADROOM: f32 = 1.15;
// A size that keeps less than this share of the whole view's pixels for itself is not worth a crop
const WORTH: f32 = 0.85;
// The share of the current pixels below which the need must fall to shrink the images
const SHRINK_BELOW: f32 = 0.6;
// The frames for which the need must stay that small before the images shrink: a quarter of a
// second at 120 Hz
const SHRINK_AFTER: u32 = 30;
// The margin around the content's box, in pixels of the image
const MARGIN_PIXELS: f32 = 4.0;
// A corner nearer the eye's plane than this, in meters, leaves the crop to the whole view
const NEAREST: f32 = 0.05;

// A box in the room, in meters: its middle and the half of each of its three edges as a vector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RoomBox {
    pub(crate) middle: [f32; 3],
    pub(crate) half_edges: [[f32; 3]; 3],
}

impl RoomBox {
    // The box of `extent`, the half sizes in dp of a box about a volume's center, for a volume at
    // `pose`. A dp is a millimeter, and the content's axes turn into the room's by the pose.
    pub(crate) fn of_volume(pose: &Pose, extent: [f32; 3]) -> Self {
        let edge = |axis: [f32; 3]| pose.rotate(axis.map(|value| value / 1000.0));
        Self {
            middle: pose.position,
            half_edges: [
                edge([extent[0], 0.0, 0.0]),
                edge([0.0, extent[1], 0.0]),
                edge([0.0, 0.0, extent[2]]),
            ],
        }
    }

    // A cube of `half` meters each way about `middle`, along the room's axes.
    pub(crate) fn cube(middle: [f32; 3], half: f32) -> Self {
        Self {
            middle,
            half_edges: [[half, 0.0, 0.0], [0.0, half, 0.0], [0.0, 0.0, half]],
        }
    }

    fn corners(&self) -> impl Iterator<Item = [f32; 3]> + '_ {
        (0..8).map(move |corner: u32| {
            let mut point = self.middle;
            for (axis, edge) in self.half_edges.iter().enumerate() {
                let sign = if corner & (1 << axis) == 0 { -1.0 } else { 1.0 };
                for (value, part) in point.iter_mut().zip(edge) {
                    *value += sign * part;
                }
            }
            point
        })
    }
}

// What the projection layer must show of the frame: the boxes of its content, or the whole view.
pub(crate) enum Content<'a> {
    Boxes(&'a [RoomBox]),
    Whole,
}

// The tangents of a rectangle of an eye's view: right and up are positive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tangents {
    pub(crate) left: f32,
    pub(crate) right: f32,
    pub(crate) up: f32,
    pub(crate) down: f32,
}

impl Tangents {
    fn of(view: &FieldOfView) -> Self {
        Self {
            left: view.angle_left.tan(),
            right: view.angle_right.tan(),
            up: view.angle_up.tan(),
            down: view.angle_down.tan(),
        }
    }
}

// The part of `eye`'s view that `boxes` cover, within the view, or `None` when a box reaches
// beside or behind the eye, where the whole view must show.
pub(crate) fn seen_part(eye: &RenderViewpoint, boxes: &[RoomBox]) -> Option<Tangents> {
    let view = Tangents::of(&eye.field_of_view);
    let mut seen: Option<Tangents> = None;
    for corner in boxes.iter().flat_map(RoomBox::corners) {
        let offset = std::array::from_fn(|axis| corner[axis] - eye.pose.position[axis]);
        let [x, y, z] = eye.pose.unrotate(offset);
        let ahead = -z;
        if ahead < NEAREST {
            return None;
        }
        let (across, up) = (
            (x / ahead).clamp(view.left, view.right),
            (y / ahead).clamp(view.down, view.up),
        );
        let part = seen.get_or_insert(Tangents {
            left: across,
            right: across,
            up,
            down: up,
        });
        part.left = part.left.min(across);
        part.right = part.right.max(across);
        part.up = part.up.max(up);
        part.down = part.down.min(up);
    }
    seen
}

// The size of the layer's images, which changes in steps as the content needs.
#[derive(Default)]
pub(crate) struct LayerSize {
    current: Option<(u32, u32)>,
    // The frames in a row whose need was small enough to shrink the images
    small: u32,
}

impl LayerSize {
    // The size of the images for a frame that needs `need` pixels, or the whole view's `whole`
    // pixels for `None`. A need beyond the current size grows the images at once, with room to
    // spare. A need below a share of them shrinks them once it has held for a while.
    pub(crate) fn next(&mut self, need: Option<(u32, u32)>, whole: (u32, u32)) -> (u32, u32) {
        let step = (whole.0.div_ceil(STEPS), whole.1.div_ceil(STEPS));
        let fitted = need.map(|(width, height)| {
            let up = |value: u32, step: u32, limit: u32| {
                ((value as f32 * HEADROOM).ceil() as u32)
                    .div_ceil(step)
                    .max(1)
                    .saturating_mul(step)
                    .min(limit)
            };
            (up(width, step.0, whole.0), up(height, step.1, whole.1))
        });
        let area = |(width, height): (u32, u32)| u64::from(width) * u64::from(height);
        let wanted = match fitted {
            Some(fitted) if (area(fitted) as f32) < WORTH * area(whole) as f32 => fitted,
            _ => whole,
        };
        let current = *self.current.get_or_insert(wanted);
        let grows = wanted.0 > current.0 || wanted.1 > current.1;
        let small = (area(wanted) as f32) < SHRINK_BELOW * area(current) as f32;
        self.small = if small { self.small + 1 } else { 0 };
        if grows || self.small >= SHRINK_AFTER {
            let next = if grows {
                (wanted.0.max(current.0), wanted.1.max(current.1))
            } else {
                wanted
            };
            self.current = Some(next);
            self.small = 0;
        }
        self.current.unwrap_or(wanted)
    }
}

// The pixels that `part` takes at `focal_length` pixels for a unit of tangent, with the margin.
pub(crate) fn pixels_of(part: &Tangents, focal_length: f32) -> (u32, u32) {
    let pixels = |span: f32| (span * focal_length + 2.0 * MARGIN_PIXELS).ceil().max(1.0) as u32;
    (pixels(part.right - part.left), pixels(part.up - part.down))
}

// The region of an image of `size` pixels about the middle of `part`, at the focal length of the
// whole view's `whole` region.
pub(crate) fn region_about(part: &Tangents, whole: &ImageRegion, size: (u32, u32)) -> ImageRegion {
    let focal_length = whole.focal_length;
    let (middle_x, middle_y) = ((part.left + part.right) / 2.0, (part.up + part.down) / 2.0);
    ImageRegion {
        left: middle_x - size.0 as f32 / 2.0 / focal_length,
        top: middle_y + size.1 as f32 / 2.0 / focal_length,
        focal_length,
    }
}

// What each eye's image shows for `content`: the images' size and each eye's region of its view,
// for eyes whose whole view takes images of `whole` pixels. `None` while an eye's view is empty.
pub(crate) fn crop(
    size: &mut LayerSize,
    eyes: [&RenderViewpoint; 2],
    content: Content<'_>,
    whole: (u32, u32),
) -> Option<((u32, u32), [ImageRegion; 2])> {
    let regions = [
        ImageRegion::whole(&eyes[0].field_of_view, whole)?,
        ImageRegion::whole(&eyes[1].field_of_view, whole)?,
    ];
    let parts = match content {
        Content::Boxes(boxes) => match (seen_part(eyes[0], boxes), seen_part(eyes[1], boxes)) {
            (Some(left), Some(right)) => Some([left, right]),
            _ => None,
        },
        Content::Whole => None,
    };
    let need = parts.map(|parts| {
        let [left, right] = [0, 1].map(|eye| pixels_of(&parts[eye], regions[eye].focal_length));
        (left.0.max(right.0), left.1.max(right.1))
    });
    let images = size.next(need, whole);
    match parts {
        Some(parts) if images != whole => Some((
            images,
            [0, 1].map(|eye| region_about(&parts[eye], &regions[eye], images)),
        )),
        _ => Some((whole, regions)),
    }
}

#[cfg(test)]
#[path = "tests/projection_crop_tests.rs"]
mod tests;
