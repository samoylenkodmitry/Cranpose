//! A place and a turn in the room.

/// A place in meters and a turn as a unit quaternion, in OpenXR's axes: x to the right, y up and
/// z toward the viewer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// The place, in meters.
    pub position: [f32; 3],
    /// The turn as a unit quaternion: x, y, z, w.
    pub orientation: [f32; 4],
}

impl Pose {
    /// No offset and no turn.
    pub const IDENTITY: Self = Self {
        position: [0.0; 3],
        orientation: [0.0, 0.0, 0.0, 1.0],
    };

    /// The pose at `position` with no turn.
    pub const fn at(position: [f32; 3]) -> Self {
        Self {
            position,
            orientation: [0.0, 0.0, 0.0, 1.0],
        }
    }

    /// Turns the direction `v` by this pose's turn.
    pub fn rotate(&self, v: [f32; 3]) -> [f32; 3] {
        rotate(self.orientation, v)
    }

    /// Turns the direction `v` back by this pose's turn.
    pub fn unrotate(&self, v: [f32; 3]) -> [f32; 3] {
        let [x, y, z, w] = self.orientation;
        rotate([-x, -y, -z, w], v)
    }

    /// The pose `child`, given in this pose's frame, in the frame this pose is given in.
    pub fn then(&self, child: &Self) -> Self {
        let moved = self.rotate(child.position);
        Self {
            position: [
                self.position[0] + moved[0],
                self.position[1] + moved[1],
                self.position[2] + moved[2],
            ],
            orientation: multiply(self.orientation, child.orientation),
        }
    }
}

// The turn by `pitch` about x, then `yaw` about y, then `roll` about z, as seen from the turned
// frame, all in degrees: a head that turns left by yaw, then nods by pitch.
pub(crate) fn turn_degrees(pitch: f32, yaw: f32, roll: f32) -> [f32; 4] {
    let about = |axis: [f32; 3], degrees: f32| {
        let (sin, cos) = (degrees.to_radians() / 2.0).sin_cos();
        [axis[0] * sin, axis[1] * sin, axis[2] * sin, cos]
    };
    multiply(
        multiply(about([0.0, 1.0, 0.0], yaw), about([1.0, 0.0, 0.0], pitch)),
        about([0.0, 0.0, 1.0], roll),
    )
}

fn multiply(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let u = [q[0], q[1], q[2]];
    let w = q[3];
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    // v + 2w(u x v) + 2 u x (u x v)
    let t = cross(u, v);
    let t2 = cross(u, t);
    [
        v[0] + 2.0 * (w * t[0] + t2[0]),
        v[1] + 2.0 * (w * t[1] + t2[1]),
        v[2] + 2.0 * (w * t[2] + t2[2]),
    ]
}

#[cfg(test)]
#[path = "tests/pose_tests.rs"]
mod tests;
