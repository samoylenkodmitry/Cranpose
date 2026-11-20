//! The size, place and turn of a spatial composable.

use crate::{MovePolicy, Pose, ResizeLimits, ResizePolicy, pose::turn_degrees};

/// Meters in one dp. In a subspace one dp is a millimeter, so a panel of 800 by 500 dp is 80 by
/// 50 centimeters.
pub const METERS_PER_DP: f32 = 0.001;

/// The size of a panel that names none, in dp.
pub const DEFAULT_PANEL_SIZE: [f32; 2] = [800.0, 500.0];

/// The width, height and depth of a volume that names none, in dp: a cube of half a meter.
pub const DEFAULT_VOLUME_SIZE: [f32; 3] = [500.0, 500.0, 500.0];

/// The size, place and turn of a spatial composable, in dp. Jetpack XR's `SubspaceModifier`.
///
/// ```
/// use cranpose_xr::SubspaceModifier;
///
/// let modifier = SubspaceModifier::new()
///     .size(800.0, 500.0)
///     .offset(0.0, 100.0, 0.0);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct SubspaceModifier {
    size: [Option<f32>; 2],
    depth: Option<f32>,
    // The place in meters
    offset: [f32; 3],
    orientation: [f32; 4],
    move_policy: Option<MovePolicy>,
    resize: Option<(ResizeLimits, ResizePolicy)>,
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
            depth: None,
            offset: [0.0; 3],
            orientation: [0.0, 0.0, 0.0, 1.0],
            move_policy: None,
            resize: None,
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

    /// Sets the depth, in dp: how far a [`SpatialVolume`](crate::SpatialVolume) reaches back from
    /// its front face. A panel is flat and ignores it. Jetpack XR's `SubspaceModifier.depth`.
    pub const fn depth(mut self, depth: f32) -> Self {
        self.depth = Some(depth);
        self
    }

    /// Moves the content by `x` to the right, `y` up and `z` toward the viewer, in dp.
    pub const fn offset(mut self, x: f32, y: f32, z: f32) -> Self {
        self.offset = [x * METERS_PER_DP, y * METERS_PER_DP, z * METERS_PER_DP];
        self
    }

    /// Turns the content by `pitch` about its horizontal axis, `yaw` about its vertical axis and
    /// `roll` about the axis toward the viewer, in degrees. A positive yaw turns its face to the
    /// right.
    pub fn rotate(mut self, pitch: f32, yaw: f32, roll: f32) -> Self {
        self.orientation = turn_degrees(pitch, yaw, roll);
        self
    }

    /// Places the content at `pose` in its subspace, in meters. It replaces the offset and the
    /// turn of this modifier. A [`SpatialMoveEvent`](crate::SpatialMoveEvent) reports poses in
    /// this frame, so an app that saves the pose of its content restores it with this method.
    pub const fn with_pose(mut self, pose: Pose) -> Self {
        self.offset = pose.position;
        self.orientation = pose.orientation;
        self
    }

    /// Lets the user carry the content with a controller. A handle under the content grabs it. The
    /// place that the user gives replaces the place of this modifier until the content leaves
    /// the composition. Jetpack XR's `movable`.
    pub fn movable(self) -> Self {
        self.movable_with(MovePolicy::default())
    }

    /// Lets the user carry the content as [`movable`](Self::movable) does, and applies `policy`
    /// to the move: a callback that receives the pose of each step, and who sets the content's
    /// pose. Jetpack XR's `movable` with a `movePolicy`.
    pub fn movable_with(mut self, policy: MovePolicy) -> Self {
        self.move_policy = Some(policy);
        self
    }

    // What the content does when the user carries it, or `None` for content that stays.
    pub(crate) fn move_policy(&self) -> Option<&MovePolicy> {
        self.move_policy.as_ref()
    }

    /// Lets the user resize the content with a controller. A handle at the content's bottom right
    /// corner grabs it. The content's center stays where it is, and `policy` receives each size
    /// that the user gives. The app owns the size: the callback sets the state that feeds
    /// [`size`](Self::size). Jetpack XR's `resizable` with its default limits.
    ///
    /// Jetpack XR's `enabled` has no counterpart: an app that does not want the handle leaves the
    /// modifier out. A panel inside a [`SpatialCurvedRow`](crate::SpatialCurvedRow) has no
    /// handle. Only a headset shows it.
    pub fn resizable(self, policy: ResizePolicy) -> Self {
        self.resizable_with(ResizeLimits::new(), policy)
    }

    /// Lets the user resize the content as [`resizable`](Self::resizable) does, within `limits`:
    /// a smallest and a largest size, and whether the proportions stay. Jetpack XR's `resizable`
    /// with its `minimumSize`, `maximumSize` and `maintainAspectRatio`.
    pub fn resizable_with(mut self, limits: ResizeLimits, policy: ResizePolicy) -> Self {
        self.resize = Some((limits, policy));
        self
    }

    // The limits and the callback of a resize that the user makes, or `None` for content whose
    // size stays.
    pub(crate) fn resize(&self) -> Option<(ResizeLimits, &ResizePolicy)> {
        self.resize
            .as_ref()
            .map(|(limits, policy)| (*limits, policy))
    }

    /// The size in dp, with `fallback` for what the modifier leaves out.
    pub fn size_or(&self, fallback: [f32; 2]) -> [f32; 2] {
        [
            self.size[0].unwrap_or(fallback[0]),
            self.size[1].unwrap_or(fallback[1]),
        ]
    }

    /// The width, height and depth in dp, with `fallback` for what the modifier leaves out.
    pub fn size_3d_or(&self, fallback: [f32; 3]) -> [f32; 3] {
        let [width, height] = self.size_or([fallback[0], fallback[1]]);
        [width, height, self.depth.unwrap_or(fallback[2])]
    }

    // The width, height and depth that the modifier sets, in dp, and `None` for a side it leaves
    // out.
    #[cfg(feature = "gltf")]
    pub(crate) fn size_limits(&self) -> [Option<f32>; 3] {
        [self.size[0], self.size[1], self.depth]
    }

    /// The pose this modifier gives in its parent's frame, in meters.
    pub fn pose(&self) -> Pose {
        Pose {
            position: self.offset,
            orientation: self.orientation,
        }
    }
}
