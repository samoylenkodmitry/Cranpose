// A scene for tests that put a panel on a mesh: its camera looks straight at
// the plane z = 0 from 600 units in front, with `middle` in the middle of the
// view, so a unit there spans one pixel.

use cranpose_ui::space::{Camera, Scene3d, Vec3};

pub(crate) fn facing(middle: Vec3) -> Scene3d {
    Scene3d::new(Camera::look_at(
        middle + Vec3::new(0.0, 0.0, -600.0),
        middle,
        Vec3::new(0.0, 1.0, 0.0),
        600.0,
    ))
}
