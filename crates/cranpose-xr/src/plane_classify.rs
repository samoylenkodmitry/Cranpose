//! What a runtime's plane is, from its normal and its label.

use crate::{PlaneLabel, PlaneType};

// How far from the vertical a plane's normal may point for the plane to count as horizontal: the
// cosine of 30 degrees.
const HORIZONTAL: f32 = 0.866;

// The direction that a plane faces, from its unit normal in the room, where +Y is up. A normal
// within 30 degrees of the vertical is a horizontal plane that faces up or down. Every other plane
// counts as vertical.
pub(crate) fn type_of_normal(normal: [f32; 3]) -> PlaneType {
    if normal[1] >= HORIZONTAL {
        PlaneType::HorizontalUpwardFacing
    } else if normal[1] <= -HORIZONTAL {
        PlaneType::HorizontalDownwardFacing
    } else {
        PlaneType::Vertical
    }
}

// The label of a plane in the semantic labels of Meta's scene model, which come as one string of
// names that a comma separates. A plane has one name in practice. The first name that Jetpack XR's
// labels cover decides. Every other kind of surface, such as a couch or a door frame, is unknown.
pub(crate) fn label_of_meta(labels: &str) -> PlaneLabel {
    labels
        .split(',')
        .find_map(|name| match name.trim() {
            "FLOOR" => Some(PlaneLabel::Floor),
            "CEILING" => Some(PlaneLabel::Ceiling),
            "WALL_FACE" | "INVISIBLE_WALL_FACE" => Some(PlaneLabel::Wall),
            "TABLE" | "DESK" => Some(PlaneLabel::Table),
            _ => None,
        })
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "tests/plane_classify_tests.rs"]
mod tests;
