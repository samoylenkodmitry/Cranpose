//! The light on a model until the content sets another.

use cranpose::space::{Light, Vec3};

// From the upper left over the viewer's shoulder, in the axes of a volume's content and of the
// scene: x to the right, y down and z away from the viewer.
pub(crate) const DEFAULT_LIGHT: Light = Light {
    direction: Vec3::new(0.45, 0.6, 0.65),
    ambient: 0.35,
    shadows: false,
    size: 0.0,
};
