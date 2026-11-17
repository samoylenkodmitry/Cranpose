use std::{f32::consts::FRAC_1_SQRT_2, ops::Mul};

use super::sample::DisplayRotation;

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

    /// The rotation from a Core Motion attitude. Apple does not state the
    /// direction of its quaternion. This takes it as the rotation from the
    /// reference frame to the device frame and returns the opposite. On a
    /// tilted device, `rotation.conjugate().rotate([0.0, 0.0, -1.0])` must
    /// equal the `gravity` of the same `CMDeviceMotion`.
    pub fn from_core_motion(x: f64, y: f64, z: f64, w: f64) -> Self {
        Self::new(narrow(-x), narrow(-y), narrow(-z), narrow(w)).normalized()
    }

    /// The rotation of the screen frame instead of the device frame, for a
    /// display turned by `rotation`. The screen frame has x to the right of
    /// the picture and y to its top. In the natural orientation the result
    /// equals `self`.
    pub fn in_screen_frame(self, rotation: DisplayRotation) -> Self {
        // The screen axes in device coordinates are the device axes turned
        // about z by minus the rotation of the display.
        let screen_from_device = match rotation {
            DisplayRotation::Rotation0 => Self::IDENTITY,
            DisplayRotation::Rotation90 => Self::new(0.0, 0.0, -FRAC_1_SQRT_2, FRAC_1_SQRT_2),
            DisplayRotation::Rotation180 => Self::new(0.0, 0.0, -1.0, 0.0),
            DisplayRotation::Rotation270 => Self::new(0.0, 0.0, FRAC_1_SQRT_2, FRAC_1_SQRT_2),
        };
        self * screen_from_device
    }

    /// The rotation of the web's `deviceorientation` event from its `alpha`,
    /// `beta` and `gamma` angles in degrees: turn about z by `alpha`, then
    /// about the new x by `beta`, then about the new y by `gamma`.
    pub fn from_device_orientation(alpha: f64, beta: f64, gamma: f64) -> Self {
        let (sin_beta, cos_beta) = (beta.to_radians() / 2.0).sin_cos();
        let (sin_gamma, cos_gamma) = (gamma.to_radians() / 2.0).sin_cos();
        let (sin_alpha, cos_alpha) = (alpha.to_radians() / 2.0).sin_cos();
        Self::new(
            narrow(sin_beta * cos_gamma * cos_alpha - cos_beta * sin_gamma * sin_alpha),
            narrow(cos_beta * sin_gamma * cos_alpha + sin_beta * cos_gamma * sin_alpha),
            narrow(cos_beta * cos_gamma * sin_alpha + sin_beta * sin_gamma * cos_alpha),
            narrow(cos_beta * cos_gamma * cos_alpha - sin_beta * sin_gamma * sin_alpha),
        )
        .normalized()
    }
}

// A component of a unit quaternion as the nearest `f32`: the precision a
// sample keeps. A component lies within -1 and 1, so the value stays finite.
fn narrow(component: f64) -> f32 {
    // Only the lint budget's run turns this lint on, so an `expect` would
    // go unmet in every other build.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a component within -1 and 1 loses only the digits `f32` does not hold"
    )]
    let narrow = component as f32;
    narrow
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
