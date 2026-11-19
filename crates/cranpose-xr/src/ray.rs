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
        let (distance, [x, y]) = self.plane_point(panel)?;
        let (u, v) = (x / size[0] + 0.5, 0.5 - y / size[1]);
        ((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)).then_some(PanelHit {
            distance,
            u,
            v,
        })
    }

    /// Where the ray first meets a box of `size` meters (width, height and depth) centered on
    /// `pose`. The hit's `u` and `v` are the entry point's place across the box's front face,
    /// seen from the box's +z, from 0 to 1, whichever face the ray enters by. A ray that starts
    /// inside the box hits it at distance 0.
    pub fn hit_box(&self, pose: &Pose, size: [f32; 3]) -> Option<PanelHit> {
        let start = pose.unrotate([
            self.origin[0] - pose.position[0],
            self.origin[1] - pose.position[1],
            self.origin[2] - pose.position[2],
        ]);
        let direction = pose.unrotate(self.direction);
        // The stretch of the ray inside each pair of the box's parallel faces, which the ray
        // shares with all three pairs where it is inside the box
        let (mut enters, mut leaves) = (0.0_f32, f32::INFINITY);
        for axis in 0..3 {
            let half = size[axis] / 2.0;
            if direction[axis].abs() < 1e-9 {
                if start[axis].abs() > half {
                    return None;
                }
                continue;
            }
            let (first, second) = (
                (-half - start[axis]) / direction[axis],
                (half - start[axis]) / direction[axis],
            );
            enters = enters.max(first.min(second));
            leaves = leaves.min(first.max(second));
            if enters > leaves {
                return None;
            }
        }
        let x = start[0] + enters * direction[0];
        let y = start[1] + enters * direction[1];
        Some(PanelHit {
            distance: enters,
            u: (x / size[0] + 0.5).clamp(0.0, 1.0),
            v: (0.5 - y / size[1]).clamp(0.0, 1.0),
        })
    }

    // Where the ray meets the plane of a panel centered on `panel`, which faces its +z: how far
    // along the ray, and the place from the panel's center to the right and up, in meters. The
    // place may lie outside the panel. A ray parallel to the plane, or one that points away from
    // it, meets it nowhere.
    pub(crate) fn plane_point(&self, panel: &Pose) -> Option<(f32, [f32; 2])> {
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
        Some((
            distance,
            [
                start[0] + distance * direction[0],
                start[1] + distance * direction[1],
            ],
        ))
    }

    /// Where the ray meets a panel of `size` meters bent around a cylinder of `radius` meters.
    /// `panel` is the pose of the panel's center, which faces its +z, and the cylinder's upright
    /// axis stands `radius` meters from the center along the panel's +z. The first size is the
    /// panel's arc length, and `u` runs along the arc. Either side of the panel stops the ray.
    /// A radius that is not positive or not finite makes the panel flat.
    pub fn hit_cylinder(&self, panel: &Pose, size: [f32; 2], radius: f32) -> Option<PanelHit> {
        if !(radius.is_finite() && radius > 0.0) {
            return self.hit_panel(panel, size);
        }
        let start = panel.unrotate([
            self.origin[0] - panel.position[0],
            self.origin[1] - panel.position[1],
            self.origin[2] - panel.position[2],
        ]);
        let direction = panel.unrotate(self.direction);
        // The ray's distance from the axis, in the plane across the axis, equals the radius
        let from_axis = [start[0], start[2] - radius];
        let a = direction[0] * direction[0] + direction[2] * direction[2];
        let b = 2.0 * (from_axis[0] * direction[0] + from_axis[1] * direction[2]);
        let c = from_axis[0] * from_axis[0] + from_axis[1] * from_axis[1] - radius * radius;
        let discriminant = b * b - 4.0 * a * c;
        if a < 1e-12 || discriminant < 0.0 {
            return None;
        }
        let root = discriminant.sqrt();
        [(-b - root) / (2.0 * a), (-b + root) / (2.0 * a)]
            .into_iter()
            .filter(|distance| *distance > 0.0)
            .find_map(|distance| {
                let x = start[0] + distance * direction[0];
                let y = start[1] + distance * direction[1];
                let z = start[2] + distance * direction[2];
                let arc = x.atan2(radius - z) * radius;
                let (u, v) = (arc / size[0] + 0.5, 0.5 - y / size[1]);
                ((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)).then_some(PanelHit {
                    distance,
                    u,
                    v,
                })
            })
    }
}

// A ray as shown: from its origin along its direction for `length` meters.
#[cfg(any(test, headset))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Beam {
    pub(crate) ray: Ray,
    pub(crate) length: f32,
}

/// A ray's hit on a panel or a volume.
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
