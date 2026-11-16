// An eye of a viewer and the camera it sees the scene through.
//
// The scene is built in meters in WebXR's frame: x right, y up, z toward the viewer, the floor
// at y = 0. A Cranpose scene's camera looks along z with y down, so `cran` turns a WebXR point
// half a turn about x. It also scales meters to millimeters: a camera cuts away what lies
// nearer than a 64th of its focal length in pixels, which is several meters in meter units.
//
// A Cranpose camera is a pinhole with its center in the middle of its box. A headset eye's
// view is off center: the camera shears x and y by z, which moves the image center by the same
// amount at every depth and keeps the eye's box and its clip.

use cranpose_ui::{
    space::{Camera, Mat4, Vec3},
    Size,
};

pub(super) const EYE_HEIGHT: f32 = 1.6;
pub(super) const UNITS_PER_METER: f32 = 1000.0;
const IPD: f32 = 0.064;
// A Quest 3 left eye: its view reaches farther outward than toward the nose.
const LEFT_FOV: [f32; 4] = [-1.20, 0.95, 0.95, -1.15];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EyeView {
    pub(super) position: [f32; 3],
    pub(super) orientation: [f32; 4],
    pub(super) fov: [f32; 4],
}

// How an eye's view fits its box: the focal length in pixels, and how far the view's center
// lies from the box's middle, in focal lengths.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct EyeLayout {
    pub(super) focal_length: f32,
    pub(super) shear: [f32; 2],
}

impl EyeView {
    pub(super) fn from_page(d: &[f32]) -> Self {
        Self {
            position: [d[0], d[1], d[2]],
            orientation: [d[3], d[4], d[5], d[6]],
            fov: [d[7], d[8], d[9], d[10]],
        }
    }

    pub(super) fn camera(&self, layout: EyeLayout) -> Camera {
        let q = self.orientation;
        let right = turn(rotate(q, [1.0, 0.0, 0.0]));
        let down = turn(rotate(q, [0.0, -1.0, 0.0]));
        let forward = turn(rotate(q, [0.0, 0.0, -1.0]));
        let eye = cran(self.position);
        let [sx, sy] = layout.shear;
        let row = |a: Vec3| [a.x, a.y, a.z, -a.dot(eye)];
        let z = row(forward);
        let sheared = |a: Vec3, s: f32| {
            let r = row(a);
            [
                r[0] + s * z[0],
                r[1] + s * z[1],
                r[2] + s * z[2],
                r[3] + s * z[3],
            ]
        };
        Camera {
            view: Mat4::from_rows([
                sheared(right, sx),
                sheared(down, sy),
                z,
                [0.0, 0.0, 0.0, 1.0],
            ]),
            focal_length: layout.focal_length,
        }
    }

    // The view keeps square pixels and fits `size`, centered.
    pub(super) fn layout(&self, size: Size) -> EyeLayout {
        let [l, r, u, d] = self.fov;
        let focal_length = (size.width / (r - l)).min(size.height / (u - d)).max(1.0);
        // the view's center, from the middle of the image the view fills
        let center_x = -(l + r) * 0.5 * focal_length;
        let center_y = (u + d) * 0.5 * focal_length;
        EyeLayout {
            focal_length,
            shear: [center_x / focal_length, center_y / focal_length],
        }
    }

    // One eye in the middle of a head at eye height, with an even view of `vertical_fov`
    // radians across a box of `aspect`.
    pub(super) fn middle(yaw: f32, pitch: f32, vertical_fov: f32, aspect: f32) -> Self {
        let tan = (vertical_fov * 0.5).tan();
        Self {
            position: [0.0, EYE_HEIGHT, 0.0],
            orientation: yaw_pitch(yaw, pitch),
            fov: [-tan * aspect, tan * aspect, tan, -tan],
        }
    }

    // The two eyes, left then right, of a head at eye height that turned `yaw` radians left and
    // `pitch` up.
    pub(super) fn pair(yaw: f32, pitch: f32) -> [Self; 2] {
        let orientation = yaw_pitch(yaw, pitch);
        let right = rotate(orientation, [IPD * 0.5, 0.0, 0.0]);
        let eye = |side: f32, fov: [f32; 4]| Self {
            position: [
                right[0] * side,
                EYE_HEIGHT + right[1] * side,
                right[2] * side,
            ],
            orientation,
            fov,
        };
        let [l, r, u, d] = LEFT_FOV;
        [eye(-1.0, LEFT_FOV), eye(1.0, [-r, -l, u, d])]
    }
}

// A WebXR point in the scene.
pub(super) fn cran(point: [f32; 3]) -> Vec3 {
    turn(point) * UNITS_PER_METER
}

// A WebXR direction in the scene.
pub(super) fn turn([x, y, z]: [f32; 3]) -> Vec3 {
    Vec3::new(x, -y, -z)
}

fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let (axis, w) = (Vec3::new(q[0], q[1], q[2]), q[3]);
    let v = Vec3::new(v[0], v[1], v[2]);
    let t = axis.cross(v) * 2.0;
    let r = v + t * w + axis.cross(t);
    [r.x, r.y, r.z]
}

// Yaw about y, then pitch about the turned x.
fn yaw_pitch(yaw: f32, pitch: f32) -> [f32; 4] {
    let (sy, cy) = (yaw * 0.5).sin_cos();
    let (sp, cp) = (pitch * 0.5).sin_cos();
    [cy * sp, sy * cp, -sy * sp, cy * cp]
}

#[cfg(test)]
#[path = "tests/eye_view.rs"]
mod tests;
