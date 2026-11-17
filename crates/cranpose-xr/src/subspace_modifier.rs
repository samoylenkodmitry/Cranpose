//! The size, place and turn of a spatial composable.

use crate::{Pose, pose::turn_degrees, space::Mobility};

/// Meters in one dp. In a subspace one dp is a millimeter, so a panel of 800 by 500 dp is 80 by
/// 50 centimeters.
pub const METERS_PER_DP: f32 = 0.001;

/// The size of a panel that names none, in dp.
pub const DEFAULT_PANEL_SIZE: [f32; 2] = [800.0, 500.0];

/// The size, place and turn of a spatial composable, in dp. Jetpack XR's `SubspaceModifier`.
///
/// ```
/// use cranpose_xr::SubspaceModifier;
///
/// let modifier = SubspaceModifier::new()
///     .size(800.0, 500.0)
///     .offset(0.0, 100.0, 0.0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubspaceModifier {
    size: [Option<f32>; 2],
    offset: [f32; 3],
    orientation: [f32; 4],
    mobility: Mobility,
}

impl Default for SubspaceModifier {
    fn default() -> Self {
        Self::new()
    }
}

impl SubspaceModifier {
    /// No size, offset or turn.
    pub const fn new() -> Self {
        Self {
            size: [None; 2],
            offset: [0.0; 3],
            orientation: [0.0, 0.0, 0.0, 1.0],
            mobility: Mobility::Fixed,
        }
    }

    /// Sets the width, in dp.
    pub const fn width(mut self, width: f32) -> Self {
        self.size[0] = Some(width);
        self
    }

    /// Sets the height, in dp.
    pub const fn height(mut self, height: f32) -> Self {
        self.size[1] = Some(height);
        self
    }

    /// Sets the width and the height, in dp.
    pub const fn size(self, width: f32, height: f32) -> Self {
        self.width(width).height(height)
    }

    /// Moves the content by `x` to the right, `y` up and `z` toward the viewer, in dp.
    pub const fn offset(mut self, x: f32, y: f32, z: f32) -> Self {
        self.offset = [x, y, z];
        self
    }

    /// Turns the content by `pitch` about its horizontal axis, `yaw` about its vertical axis and
    /// `roll` about the axis toward the viewer, in degrees. A positive yaw turns its face to the
    /// right.
    pub fn rotate(mut self, pitch: f32, yaw: f32, roll: f32) -> Self {
        self.orientation = turn_degrees(pitch, yaw, roll);
        self
    }

    /// Lets the user carry the content with a controller. A handle under the content grabs it. The
    /// place that the user gives replaces the place of this modifier until the content leaves
    /// the composition. Jetpack XR's `movable`.
    pub const fn movable(mut self) -> Self {
        self.mobility = Mobility::Movable;
        self
    }

    // Whether the user may carry the content.
    pub(crate) const fn mobility(&self) -> Mobility {
        self.mobility
    }

    /// The size in dp, with `fallback` for what the modifier leaves out.
    pub fn size_or(&self, fallback: [f32; 2]) -> [f32; 2] {
        [
            self.size[0].unwrap_or(fallback[0]),
            self.size[1].unwrap_or(fallback[1]),
        ]
    }

    /// The pose this modifier gives in its parent's frame, in meters.
    pub fn pose(&self) -> Pose {
        Pose {
            position: self.offset.map(|dp| dp * METERS_PER_DP),
            orientation: self.orientation,
        }
    }
}
