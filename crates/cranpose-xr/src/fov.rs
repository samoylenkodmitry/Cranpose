//! The field of view of a view.

/// A view's field of view as four angles in radians from the direction the view looks in, as
/// OpenXR's `XrFovf`. The angles to the left and downward are negative for a view whose picture
/// surrounds its direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fov {
    /// The angle of the picture's left edge. It is below zero for a picture that surrounds the
    /// view's direction.
    pub angle_left: f32,
    /// The angle of the picture's right edge.
    pub angle_right: f32,
    /// The angle of the picture's top edge.
    pub angle_up: f32,
    /// The angle of the picture's bottom edge. It is below zero for a picture that surrounds the
    /// view's direction.
    pub angle_down: f32,
}

impl Fov {
    /// A field of view whose picture surrounds the view's direction, `horizontal` radians wide
    /// and `vertical` radians high.
    pub const fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            angle_left: -horizontal / 2.0,
            angle_right: horizontal / 2.0,
            angle_up: vertical / 2.0,
            angle_down: -vertical / 2.0,
        }
    }
}
