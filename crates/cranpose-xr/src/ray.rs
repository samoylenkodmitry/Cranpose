//! Where a controller's ray meets a panel.

use crate::Pose;

/// A ray in the room: its start in meters and its unit direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    /// Where the ray starts, in meters.
    pub origin: [f32; 3],
    /// The ray's unit direction.
    pub direction: [f32; 3],
}

impl Ray {
    /// The ray of a controller's aim pose, which points along the pose's -z.
    pub fn of_aim(aim: &Pose) -> Self {
        Self {
            origin: aim.position,
            direction: aim.rotate([0.0, 0.0, -1.0]),
        }
    }

    /// Where the ray meets a flat panel of `size` meters centered on `panel`, which faces its
    /// +z. Either face of the panel stops the ray.
    pub fn hit_panel(&self, panel: &Pose, size: [f32; 2]) -> Option<PanelHit> {
        let start = panel.unrotate([
            self.origin[0] - panel.position[0],
            self.origin[1] - panel.position[1],
            self.origin[2] - panel.position[2],
        ]);
        let direction = panel.unrotate(self.direction);
        if direction[2].abs() < 1e-6 {
            return None;
        }
        let distance = -start[2] / direction[2];
        if distance <= 0.0 {
            return None;
        }
        let x = start[0] + distance * direction[0];
        let y = start[1] + distance * direction[1];
        let (u, v) = (x / size[0] + 0.5, 0.5 - y / size[1]);
        ((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)).then_some(PanelHit {
            distance,
            u,
            v,
        })
    }
}

/// A ray's hit on a panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelHit {
    /// How far along the ray the hit is, in meters.
    pub distance: f32,
    /// The hit's place across the panel, from 0 at its left edge to 1 at its right edge.
    pub u: f32,
    /// The hit's place down the panel, from 0 at its top edge to 1 at its bottom edge.
    pub v: f32,
}

#[cfg(test)]
#[path = "tests/ray_tests.rs"]
mod tests;
