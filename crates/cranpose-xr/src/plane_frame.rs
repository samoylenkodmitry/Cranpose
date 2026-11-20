//! A plane of Meta's scene model in the frame that Jetpack XR gives a plane.

use crate::{PlaneLabel, PlaneType, Pose, plane_classify::type_of_normal};

const HALF_TURN_ABOUT_X: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
const QUARTER_TURN_ABOUT_X: [f32; 4] = [
    std::f32::consts::FRAC_1_SQRT_2,
    0.0,
    0.0,
    std::f32::consts::FRAC_1_SQRT_2,
];

// How a spatial entity of Meta's scene model maps to a plane in Jetpack XR's frame.
//
// Meta's entity has its rectangle in the X-Y plane of its space, and its Z axis is the normal.
// Jetpack XR's plane has +Y as the normal, +X along the width and +Z along the depth. A quarter
// turn about X takes the one to the other: Y becomes the normal, and Z is the old Y reversed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlaneFrame {
    // The pose of the plane's center in the entity's space
    pub(crate) local: Pose,
    // The size of the rectangle along the new X and Z axes
    pub(crate) extents: [f32; 2],
    // The center of the rectangle in the entity's X-Y plane, which the polygon's vertices lie around
    center: [f32; 2],
    // Whether the frame turned half a turn about X so that the normal points up or down as the label
    // says
    flipped: bool,
    pub(crate) plane_type: PlaneType,
}

impl PlaneFrame {
    // The frame of an entity whose space is at `space` in the room, with the rectangle `offset` and
    // `extent` in its X-Y plane and `label`. A runtime's sense of the normal's sign is a convention
    // that the label corrects: a floor or a table faces up, and a ceiling faces down. A wall has no
    // such rule, and a snap treats both of its sides alike.
    pub(crate) fn of(space: &Pose, offset: [f32; 2], extent: [f32; 2], label: PlaneLabel) -> Self {
        let center = [offset[0] + extent[0] / 2.0, offset[1] + extent[1] / 2.0];
        let mut local = Pose {
            position: [center[0], center[1], 0.0],
            orientation: QUARTER_TURN_ABOUT_X,
        };
        let normal = space.then(&local).rotate([0.0, 1.0, 0.0]);
        let mut plane_type = type_of_normal(normal);
        let wanted = match label {
            PlaneLabel::Ceiling => Some(PlaneType::HorizontalDownwardFacing),
            PlaneLabel::Floor | PlaneLabel::Table => Some(PlaneType::HorizontalUpwardFacing),
            PlaneLabel::Unknown | PlaneLabel::Wall => None,
        };
        let horizontal = plane_type != PlaneType::Vertical;
        let flipped = horizontal
            && wanted.is_some_and(|wanted| wanted != plane_type && flips_to(wanted, plane_type));
        if flipped {
            local = local.then(&Pose {
                position: [0.0; 3],
                orientation: HALF_TURN_ABOUT_X,
            });
            plane_type = wanted.unwrap_or(plane_type);
        }
        Self {
            local,
            extents: extent,
            center,
            flipped,
            plane_type,
        }
    }

    // The pose of the plane's center in the room, given where the entity's space is.
    pub(crate) fn center_pose(&self, space: &Pose) -> Pose {
        space.then(&self.local)
    }

    // A vertex of the entity's boundary, given in its X-Y plane, along the plane's X and Z axes.
    pub(crate) fn vertex(&self, vertex: [f32; 2]) -> [f32; 2] {
        let across = vertex[0] - self.center[0];
        let depth = -(vertex[1] - self.center[1]);
        [across, if self.flipped { -depth } else { depth }]
    }
}

// Whether a plane that faces `have` turns to face `wanted` by a half turn: only the two horizontal
// directions swap.
fn flips_to(wanted: PlaneType, have: PlaneType) -> bool {
    matches!(
        (wanted, have),
        (
            PlaneType::HorizontalDownwardFacing,
            PlaneType::HorizontalUpwardFacing
        ) | (
            PlaneType::HorizontalUpwardFacing,
            PlaneType::HorizontalDownwardFacing
        )
    )
}

#[cfg(test)]
#[path = "tests/plane_frame_tests.rs"]
mod tests;
