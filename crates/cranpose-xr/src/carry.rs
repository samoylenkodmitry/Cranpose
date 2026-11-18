//! The math of carrying a movable panel with a controller.

use crate::{Pose, Ray, pose::turn_degrees, space::PanelSlot, subspace_modifier::METERS_PER_DP};

// The space between the bottom edge of a panel and its handle, in meters.
const HANDLE_GAP: f32 = 0.02;

// The width and the height of a handle, in meters.
const HANDLE_SIZE: [f32; 2] = [0.12, 0.014];

// How far from the handle a ray still grabs it, in meters.
const HANDLE_MARGIN: f32 = 0.03;

// The nearest and the farthest a carried panel gets from the controller, in meters.
const MIN_DISTANCE: f32 = 0.3;
const MAX_DISTANCE: f32 = 5.0;

// A panel that rolls less than this far from level turns level, in degrees.
const LEVEL_SNAP: f32 = 4.0;

// The center pose and the size in meters of the handle under `slot`'s panel.
pub(crate) fn handle_pose(slot: &PanelSlot) -> (Pose, [f32; 2]) {
    let height = slot.size()[1] * METERS_PER_DP;
    let below = Pose::at([0.0, -(height + HANDLE_SIZE[1]) / 2.0 - HANDLE_GAP, 0.0]);
    (slot.pose.then(&below), HANDLE_SIZE)
}

// The distance along `ray` to the handle under `slot`'s panel, if the ray comes within the margin
// of it.
pub(crate) fn handle_hit(ray: &Ray, slot: &PanelSlot) -> Option<f32> {
    let (pose, size) = handle_pose(slot);
    let grab = size.map(|side| side + 2.0 * HANDLE_MARGIN);
    ray.hit_panel(&pose, grab).map(|hit| hit.distance)
}

// A panel held by a controller: the panel's pose in the controller's frame at the grab.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Carry {
    relative: Pose,
    // How far the thumbstick moved the panel away along the aim, in meters
    push: f32,
}

impl Carry {
    pub(crate) fn grab(aim: &Pose, panel: &Pose) -> Self {
        Self {
            relative: aim.inverse().then(panel),
            push: 0.0,
        }
    }

    // Moves the panel `meters` farther along the aim, between MIN_DISTANCE and MAX_DISTANCE
    // from the controller.
    pub(crate) fn push(&mut self, meters: f32) {
        let ahead = -self.relative.position[2];
        self.push = (self.push + meters).clamp(
            (MIN_DISTANCE - ahead).min(0.0),
            (MAX_DISTANCE - ahead).max(0.0),
        );
    }

    // How far the panel is from the controller along the aim, in meters.
    pub(crate) fn distance(&self) -> f32 {
        -self.relative.position[2] + self.push
    }

    pub(crate) fn pose(&self, aim: &Pose) -> Pose {
        let [x, y, z] = self.relative.position;
        let pushed = [x, y, (z - self.push).min(z.max(0.0))];
        let length = pushed.iter().map(|v| v * v).sum::<f32>().sqrt();
        let position = if length < 1e-6 {
            [0.0, 0.0, -MIN_DISTANCE]
        } else {
            let scale = length.clamp(MIN_DISTANCE, MAX_DISTANCE) / length;
            pushed.map(|v| v * scale)
        };
        let held = Pose {
            position,
            orientation: self.relative.orientation,
        };
        level(aim.then(&held))
    }
}

fn level(pose: Pose) -> Pose {
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let normal = pose.rotate([0.0, 0.0, 1.0]);
    let across = (normal[0] * normal[0] + normal[2] * normal[2]).sqrt();
    // A panel that faces straight up or down has no level.
    if across < 1e-3 {
        return pose;
    }
    let flat = [normal[2] / across, 0.0, -normal[0] / across];
    let up = cross(normal, flat);
    let x = pose.rotate([1.0, 0.0, 0.0]);
    let roll = dot(x, up).atan2(dot(x, flat)).to_degrees();
    if roll.abs() >= LEVEL_SNAP {
        return pose;
    }
    pose.then(&Pose {
        position: [0.0; 3],
        orientation: turn_degrees(0.0, 0.0, -roll),
    })
}

#[cfg(test)]
#[path = "tests/carry_tests.rs"]
mod tests;
