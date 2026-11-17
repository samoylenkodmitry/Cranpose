//! The edges of what a display shows.

/// The field of view of one eye's display: the angle of each edge from the view's forward axis,
/// in radians. Jetpack XR's `FieldOfView`.
///
/// The angles keep OpenXR's signs, which Jetpack XR's documentation leaves open: the left and
/// bottom edges lie at negative angles and the right and top edges at positive ones. A host builds
/// an asymmetric projection from them with no conversion.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FieldOfView {
    /// The angle of the left edge, negative for an ordinary view.
    pub angle_left: f32,
    /// The angle of the right edge, positive for an ordinary view.
    pub angle_right: f32,
    /// The angle of the top edge, positive for an ordinary view.
    pub angle_up: f32,
    /// The angle of the bottom edge, negative for an ordinary view.
    pub angle_down: f32,
}

impl FieldOfView {
    /// A field of view that surrounds the view's forward axis evenly, `horizontal` radians wide
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
