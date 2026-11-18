//! The math of carrying a movable panel with a controller.

use crate::{
    Pose, Ray,
    pose::turn_degrees,
    space::PanelSlot,
    subspace_modifier::METERS_PER_DP,
    vector::{cross, dot, sub},
};

// The space between the bottom edge of a panel and its handle, in meters.
const HANDLE_GAP: f32 = 0.02;

// The radius of a handle, which is a sphere, in meters.
pub(crate) const HANDLE_RADIUS: f32 = 0.02;

// How far from the handle a ray still grabs it, in meters. The margin stays under the gap, so
// the grab does not reach the panel's bottom edge. A press on that edge still presses.
const HANDLE_MARGIN: f32 = 0.015;

// The nearest and the farthest a carried panel gets from the controller, in meters.
const MIN_DISTANCE: f32 = 0.3;
const MAX_DISTANCE: f32 = 5.0;

// A panel that rolls less than this far from level turns level, in degrees.
const LEVEL_SNAP: f32 = 4.0;

// The center of the handle under `slot`'s panel: below the panel's bottom edge by the gap and the
// handle's radius.
pub(crate) fn handle_center(slot: &PanelSlot) -> [f32; 3] {
    let height = slot.size()[1] * METERS_PER_DP;
    let below = [0.0, -(height / 2.0 + HANDLE_GAP + HANDLE_RADIUS), 0.0];
    slot.pose.then(&Pose::at(below)).position
}

// The distance along `ray` to the handle under `slot`'s panel, if the ray comes within the margin
// of it. The ray meets the sphere from the panel's front, from its back or from any other side. A
// ray that starts inside the margin meets it at distance 0.
pub(crate) fn handle_hit(ray: &Ray, slot: &PanelSlot) -> Option<f32> {
    let reach = HANDLE_RADIUS + HANDLE_MARGIN;
    let from_center = sub(ray.origin, handle_center(slot));
    let along = dot(from_center, ray.direction);
    let discriminant = along * along - (dot(from_center, from_center) - reach * reach);
    if discriminant < 0.0 {
        return None;
    }
    let root = discriminant.sqrt();
    (root >= along).then(|| (-along - root).max(0.0))
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
