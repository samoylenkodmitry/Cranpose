// Where a record sits in its parent's layer.

use cranpose_ui::Point;
use cranpose_ui_graphics::ProjectiveTransform;

/// Where a record sits in its parent's layer: at a point, where layout
/// placed a record that nothing transforms, or by a transform. A transform
/// that only moves a record is a point, so two equal placements compare
/// equal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Placement {
    /// At the point, past the parent's content offset.
    At(Point),
    /// By the transform, which takes the record's space to its parent's.
    By(ProjectiveTransform),
}
