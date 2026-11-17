//! What the headset lets the app do now.

use cranpose_core::CompositionLocal;

use crate::locals::with_locals;

/// What the headset lets the app do now. Jetpack XR's `SpatialCapabilities`, which an app reads
/// through [`local_spatial_capabilities`] before it shows spatial content or controls the room.
///
/// Outside a headset every capability is off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpatialCapabilities {
    spatial_ui: bool,
    passthrough_control: bool,
}

impl SpatialCapabilities {
    // The capabilities of a headset that shows spatial panels.
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    pub(crate) const fn in_headset(passthrough_control: bool) -> Self {
        Self {
            spatial_ui: true,
            passthrough_control,
        }
    }

    /// Whether the app may create spatial panels. Jetpack XR's `isSpatialUiEnabled`.
    pub const fn is_spatial_ui_enabled(&self) -> bool {
        self.spatial_ui
    }

    /// Whether the app may control the passthrough state of the room around its panels with
    /// [`SpatialEnvironment`](crate::SpatialEnvironment). Jetpack XR's
    /// `isPassthroughControlEnabled`.
    ///
    /// It is on in a headset whose runtime has a passthrough layer, such as the Meta Quest's, or
    /// blends the layers' alpha with the room, such as Android XR, Pico and Magic Leap. A
    /// runtime with neither shows the app's own background.
    pub const fn is_passthrough_control_enabled(&self) -> bool {
        self.passthrough_control
    }
}

/// The composition local that holds the current [`SpatialCapabilities`]. Jetpack XR's
/// `LocalSpatialCapabilities`.
///
/// ```ignore
/// if local_spatial_capabilities().current().is_spatial_ui_enabled() {
///     Subspace(|| { /* panels */ });
/// }
/// ```
pub fn local_spatial_capabilities() -> CompositionLocal<SpatialCapabilities> {
    with_locals(|locals| locals.capabilities.clone())
}
