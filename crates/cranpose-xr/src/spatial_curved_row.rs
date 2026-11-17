//! A row of panels on a curve around the viewer.

use cranpose::prelude::*;
use cranpose_core::{CompositionLocal, CompositionLocalProvider};

use crate::{
    METERS_PER_DP, Pose, SubspaceModifier,
    locals::with_locals,
    pose::turn_degrees,
    space::{SUBSPACE_ORIGIN, Shape, local_space},
    subspace::FlatRow,
};

/// A row of panels on a curve that bends around the viewer. Jetpack XR's `SpatialCurvedRow`.
///
/// `curve_radius` is in dp, and a dp is a millimeter, so a radius of 1000 centers the curve on a
/// viewer who stands a meter from the row's middle, as a subspace places the viewer. A radius
/// that is not positive or not finite makes the row straight.
///
/// In a headset each panel inside the row is a cylinder layer of the headset's compositor with
/// the row's radius, and a controller's ray follows its curve. Its horizontal offset is a
/// distance along the curve from the row's middle, and it turns to face the viewer. A runtime
/// without cylinder layers shows the panels flat. Outside a headset the row shows its panels
/// side by side.
#[composable]
pub fn SpatialCurvedRow(
    modifier: SubspaceModifier,
    curve_radius: f32,
    content: impl FnMut() + 'static,
) {
    if local_space().current().is_none() {
        FlatRow(content);
        return;
    }
    let curve = Curve {
        origin: SUBSPACE_ORIGIN.then(&modifier.pose()),
        shape: if curve_radius.is_finite() && curve_radius > 0.0 {
            Shape::Curved {
                radius: curve_radius * METERS_PER_DP,
            }
        } else {
            Shape::Flat
        },
    };
    CompositionLocalProvider([local_curve().provides(Some(curve))], content);
}

// The curve that the panels of a row stand on.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Curve {
    // The row's middle, where its curve faces the viewer.
    origin: Pose,
    shape: Shape,
}

impl Curve {
    // The pose and shape of a panel that `modifier` places in the row. The panel's offset to
    // the right is a distance along the curve. Its other offsets and its turn apply at that
    // place.
    pub(crate) fn place(&self, modifier: &SubspaceModifier) -> (Pose, Shape) {
        let own = modifier.pose();
        let Shape::Curved { radius } = self.shape else {
            return (self.origin.then(&own), Shape::Flat);
        };
        let angle = own.position[0] / radius;
        let on_curve = Pose {
            position: [radius * angle.sin(), 0.0, radius * (1.0 - angle.cos())],
            // A panel to the right turns its face to the left, toward the viewer
            orientation: turn_degrees(0.0, -angle.to_degrees(), 0.0),
        };
        let at_place = Pose {
            position: [0.0, own.position[1], own.position[2]],
            orientation: own.orientation,
        };
        (self.origin.then(&on_curve).then(&at_place), self.shape)
    }
}

// The curve of the row that holds the content, or `None` outside a row.
pub(crate) fn local_curve() -> CompositionLocal<Option<Curve>> {
    with_locals(|locals| locals.curve.clone())
}

#[cfg(test)]
#[path = "tests/spatial_curved_row_tests.rs"]
mod tests;
