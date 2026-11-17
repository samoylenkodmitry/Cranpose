use std::ops::Mul;

/// A rotation as a unit quaternion `x*i + y*j + z*k + w`.
///
/// `q` and `-q` are the same rotation. A device that turns past half a turn
/// can therefore send `w` with either sign; interpolate with the dot product
/// of two quaternions in mind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quaternion {
    /// The i component.
    pub x: f32,
    /// The j component.
    pub y: f32,
    /// The k component.
    pub z: f32,
    /// The scalar component.
    pub w: f32,
}

impl Quaternion {
    /// No rotation.
    pub const IDENTITY: Self = Self::new(0.0, 0.0, 0.0, 1.0);

    /// A quaternion from its components.
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    /// The rotation that undoes this one.
    pub const fn conjugate(self) -> Self {
        Self::new(-self.x, -self.y, -self.z, self.w)
    }

    /// The length of the four components.
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt()
    }

    /// This quaternion with length one, or the identity when its length is
    /// zero or not finite.
    pub fn normalized(self) -> Self {
        let length = self.length();
        if length.is_finite() && length > 0.0 {
            Self::new(
                self.x / length,
                self.y / length,
                self.z / length,
                self.w / length,
            )
        } else {
            Self::IDENTITY
        }
    }

    /// Turns the vector `v` by this rotation.
    pub fn rotate(self, v: [f32; 3]) -> [f32; 3] {
        let q = [self.x, self.y, self.z];
        let t = cross(q, v).map(|component| 2.0 * component);
        let u = cross(q, t);
        [
            v[0] + self.w * t[0] + u[0],
            v[1] + self.w * t[1] + u[1],
            v[2] + self.w * t[2] + u[2],
        ]
    }

    /// The rotation of Android's rotation vector sensors, from the first
    /// three values of `SensorEvent.values` and the fourth when the sensor
    /// gives it. Without a fourth value the scalar part is whatever makes
    /// the length one.
    pub fn from_rotation_vector(x: f32, y: f32, z: f32, w: Option<f32>) -> Self {
        let w = w
            .filter(|w| w.is_finite())
            .unwrap_or_else(|| (1.0 - x * x - y * y - z * z).max(0.0).sqrt());
        Self::new(x, y, z, w).normalized()
    }

    /// The rotation from a Core Motion attitude, which Apple's platforms
    /// state as the rotation from the reference frame to the device frame.
    /// The result is its opposite.
    pub fn from_core_motion(x: f64, y: f64, z: f64, w: f64) -> Self {
        Self::new(-x as f32, -y as f32, -z as f32, w as f32).normalized()
    }

    /// The rotation of the web's `deviceorientation` event from its `alpha`,
    /// `beta` and `gamma` angles in degrees: turn about z by `alpha`, then
    /// about the new x by `beta`, then about the new y by `gamma`.
    pub fn from_device_orientation(alpha: f64, beta: f64, gamma: f64) -> Self {
        let (sin_beta, cos_beta) = (beta.to_radians() / 2.0).sin_cos();
        let (sin_gamma, cos_gamma) = (gamma.to_radians() / 2.0).sin_cos();
        let (sin_alpha, cos_alpha) = (alpha.to_radians() / 2.0).sin_cos();
        Self::new(
            (sin_beta * cos_gamma * cos_alpha - cos_beta * sin_gamma * sin_alpha) as f32,
            (cos_beta * sin_gamma * cos_alpha + sin_beta * cos_gamma * sin_alpha) as f32,
            (cos_beta * cos_gamma * sin_alpha + sin_beta * sin_gamma * cos_alpha) as f32,
            (cos_beta * cos_gamma * cos_alpha - sin_beta * sin_gamma * sin_alpha) as f32,
        )
        .normalized()
    }
}

// `self * other` turns a vector by `other` first, then by `self`.
impl Mul for Quaternion {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        Self::new(
            self.w * other.x + self.x * other.w + self.y * other.z - self.z * other.y,
            self.w * other.y - self.x * other.z + self.y * other.w + self.z * other.x,
            self.w * other.z + self.x * other.y - self.y * other.x + self.z * other.w,
            self.w * other.w - self.x * other.x - self.y * other.y - self.z * other.z,
        )
    }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
