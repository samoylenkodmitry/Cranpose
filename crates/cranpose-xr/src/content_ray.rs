//! A ray in a volume's content space.

use crate::{Pose, Ray, subspace_modifier::METERS_PER_DP};

// A ray in a volume's content space, which has its origin at the box's center, x to the right, y
// down and z away from the viewer, and a dp for a unit. The direction is in dp for each meter of
// the room's ray, so a point of the ray at a distance in meters lies at
// `origin + direction * distance`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(
    not(any(test, headset)),
    expect(dead_code, reason = "the headset backend's pointer makes them")
)]
pub(crate) struct ContentRay {
    pub(crate) origin: [f32; 3],
    pub(crate) direction: [f32; 3],
}

#[cfg_attr(
    not(any(test, headset)),
    expect(dead_code, reason = "the headset backend's pointer makes them")
)]
impl ContentRay {
    // The room's `ray` in the content space of the volume at `pose`.
    pub(crate) fn of(ray: &Ray, pose: &Pose) -> Self {
        let in_content = |room: [f32; 3]| {
            let [x, y, z] = pose.unrotate(room);
            [x, -y, -z]
        };
        let from_center = in_content([
            ray.origin[0] - pose.position[0],
            ray.origin[1] - pose.position[1],
            ray.origin[2] - pose.position[2],
        ]);
        Self {
            origin: from_center.map(|meters| meters / METERS_PER_DP),
            direction: in_content(ray.direction).map(|unit| unit / METERS_PER_DP),
        }
    }

    // The direction as a unit vector.
    pub(crate) fn unit_direction(&self) -> [f32; 3] {
        let [x, y, z] = self.direction;
        let length = (x * x + y * y + z * z).sqrt();
        self.direction.map(|part| part / length)
    }

    // The point at `distance` meters along the ray.
    pub(crate) fn at(&self, distance: f32) -> [f32; 3] {
        std::array::from_fn(|axis| self.origin[axis] + self.direction[axis] * distance)
    }
}
