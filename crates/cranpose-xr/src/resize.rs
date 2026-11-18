//! The math of resizing a panel with a controller.

use crate::{
    Pose, Ray, ResizeLimits,
    handle::{HANDLE_GAP, HANDLE_RADIUS},
    space::PanelSlot,
    subspace_modifier::METERS_PER_DP,
};

// How far the center of a panel's corner handle stands from the corner, to the right and down, in
// meters. The handle's own sphere then touches no part of the panel's rectangle that a press
// could land on.
const CORNER_OFFSET: f32 = HANDLE_GAP + HANDLE_RADIUS;

// The smallest width and height that the pointer gives a panel, in dp, so that a minimum of zero
// leaves a panel that the user can still see and grab.
const MIN_SIZE: f32 = 20.0;

// The largest width and height that the pointer gives a panel, in dp. A ray that grazes the
// panel's plane meets it far away, and a maximum of infinity would take that size to the atlas.
// At 1.5 pixels in a dp the panel's image stays within the 8192 pixels that a GPU texture holds.
const MAX_SIZE: f32 = 4000.0;

// The center of the handle at the bottom right corner of `slot`'s panel.
pub(crate) fn corner_center(slot: &PanelSlot) -> [f32; 3] {
    let [width, height] = slot.size().map(|dp| dp * METERS_PER_DP);
    let corner = [
        width / 2.0 + CORNER_OFFSET,
        -(height / 2.0 + CORNER_OFFSET),
        0.0,
    ];
    slot.pose.then(&Pose::at(corner)).position
}

// Where `ray` meets the plane of a panel at `pose`, in dp to the right and up from the panel's
// center.
fn plane_dp(ray: &Ray, pose: &Pose) -> Option<[f32; 2]> {
    let (_, place) = ray.plane_point(pose)?;
    Some(place.map(|meters| meters / METERS_PER_DP))
}

// A panel held by its corner handle. The panel's center stays where it is: the corner follows the
// ray's meeting with the panel's plane, and the size is twice the corner's distance from the
// center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Resize {
    // The panel's size at the grab, in dp
    start: [f32; 2],
    // Where the ray met the panel's plane at the grab, in dp to the right and up from the center,
    // so that the first step of a grab off the handle's middle does not jump
    grab: [f32; 2],
    limits: ResizeLimits,
}

impl Resize {
    // The grab of `slot`'s corner handle by `ray`, or `None` for a panel that has none.
    pub(crate) fn grab(ray: &Ray, slot: &PanelSlot) -> Option<Self> {
        let limits = slot.resize_limits()?;
        let start = slot.size();
        let handle = [
            start[0] / 2.0 + CORNER_OFFSET / METERS_PER_DP,
            -(start[1] / 2.0 + CORNER_OFFSET / METERS_PER_DP),
        ];
        Some(Self {
            start,
            grab: plane_dp(ray, &slot.pose).unwrap_or(handle),
            limits,
        })
    }

    // The size that `ray` gives the panel at `pose`, in whole dp within the limits, or `None`
    // while the ray is parallel to the panel's plane or points away from it.
    pub(crate) fn size(&self, ray: &Ray, pose: &Pose) -> Option<[f32; 2]> {
        let [x, y] = plane_dp(ray, pose)?;
        let wanted = [
            self.start[0] + 2.0 * (x - self.grab[0]),
            self.start[1] - 2.0 * (y - self.grab[1]),
        ];
        Some(self.limit(wanted))
    }

    // `wanted` within the limits. A minimum wins over a maximum below it, and the ceiling bounds a
    // maximum above it. With the proportions kept, one scale of the start size is limited, so that
    // the size stays on the proportions' line.
    fn limit(&self, wanted: [f32; 2]) -> [f32; 2] {
        let (minimum, maximum) = self.limits.bounds();
        let low = minimum.map(|dp| dp.max(MIN_SIZE));
        let high = [0, 1].map(|axis| maximum[axis].min(MAX_SIZE).max(low[axis]));
        let fit =
            |size: [f32; 2]| [0, 1].map(|axis| size[axis].round().max(low[axis]).min(high[axis]));
        let [width, height] = self.start;
        if !self.limits.keeps_ratio() || width <= 0.0 || height <= 0.0 {
            return fit(wanted);
        }
        // The scale that puts the start's proportions nearest to the wanted size
        let scale = (wanted[0] * width + wanted[1] * height) / (width * width + height * height);
        let smallest = (low[0] / width).max(low[1] / height);
        let largest = (high[0] / width).min(high[1] / height).max(smallest);
        let scale = scale.max(smallest).min(largest);
        fit([width * scale, height * scale])
    }
}

#[cfg(test)]
#[path = "tests/resize_tests.rs"]
mod tests;
