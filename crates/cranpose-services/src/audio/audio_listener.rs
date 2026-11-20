//! Where the listener's head is.

/// The pose of the listener's head: where it is and how it is turned.
///
/// The axes are OpenXR's: x to the right of the head, y up and z behind it, so the head looks
/// along -z. A position is in meters. An orientation is a unit quaternion, x, y, z, w. They match
/// the fields of a headset's pose, so an XR app copies its head pose into one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioListener {
    /// The place of the head, in meters.
    pub position: [f32; 3],
    /// The turn of the head as a unit quaternion: x, y, z, w.
    pub orientation: [f32; 4],
}

impl Default for AudioListener {
    fn default() -> Self {
        Self::AT_ORIGIN
    }
}

impl AudioListener {
    /// A head at the origin that looks along -z.
    pub const AT_ORIGIN: Self = Self {
        position: [0.0; 3],
        orientation: [0.0, 0.0, 0.0, 1.0],
    };

    /// The place of `point` in the head's own axes: x to the right, y up and z behind.
    pub fn local(&self, point: [f32; 3]) -> [f32; 3] {
        let offset = [
            point[0] - self.position[0],
            point[1] - self.position[1],
            point[2] - self.position[2],
        ];
        let [x, y, z, w] = self.orientation;
        // The turn back by the conjugate: v + 2w(u x v) + 2 u x (u x v), with u = -(x, y, z)
        let u = [-x, -y, -z];
        let cross = |a: [f32; 3], b: [f32; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let t = cross(u, offset);
        let t2 = cross(u, t);
        [
            offset[0] + 2.0 * (w * t[0] + t2[0]),
            offset[1] + 2.0 * (w * t[1] + t2[1]),
            offset[2] + 2.0 * (w * t[2] + t2[2]),
        ]
    }

    pub(crate) fn to_words(self) -> [u32; 7] {
        let [px, py, pz] = self.position;
        let [qx, qy, qz, qw] = self.orientation;
        [px, py, pz, qx, qy, qz, qw].map(f32::to_bits)
    }

    pub(crate) fn from_words(words: [u32; 7]) -> Self {
        let [px, py, pz, qx, qy, qz, qw] = words.map(f32::from_bits);
        Self {
            position: [px, py, pz],
            orientation: [qx, qy, qz, qw],
        }
    }
}

#[cfg(test)]
#[path = "tests/audio_listener_tests.rs"]
mod tests;
