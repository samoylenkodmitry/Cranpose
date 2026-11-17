//! The app's preferences for the room around its panels.

use cranpose::{MutableState, mutableStateOf};
use cranpose_core::CompositionLocal;

use crate::locals::with_locals;

/// What the app prefers of the room around its panels. Jetpack XR's `SpatialEnvironment`, which
/// an app reads through [`local_spatial_environment`].
///
/// A preference takes effect only while [`SpatialCapabilities::is_passthrough_control_enabled`]
/// is on. [`current_passthrough_opacity`](Self::current_passthrough_opacity) tells what the room
/// shows.
///
/// [`SpatialCapabilities::is_passthrough_control_enabled`]: crate::SpatialCapabilities::is_passthrough_control_enabled
#[derive(Clone, Copy, PartialEq)]
pub struct SpatialEnvironment {
    preferred: MutableState<Option<f32>>,
    current: MutableState<f32>,
}

impl SpatialEnvironment {
    // An environment with no preference, which shows no room.
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    pub(crate) fn new() -> Self {
        Self {
            preferred: mutableStateOf(None),
            current: mutableStateOf(0.0),
        }
    }

    /// The opacity of the passthrough view of the room that the app asked for, from 0 to 1, or
    /// `None` while it has no preference. Jetpack XR's `preferredPassthroughOpacity`.
    pub fn preferred_passthrough_opacity(&self) -> Option<f32> {
        self.preferred.get()
    }

    /// Asks for passthrough at `opacity`: 0 shows only the app's own background, 1 shows the room
    /// behind the panels, and a value between dims the room. A value outside 0 to 1 becomes the
    /// nearest end. `None` leaves the choice to the system, which shows the app's background.
    /// Jetpack XR's `preferredPassthroughOpacity` setter.
    ///
    /// A runtime that blends the layers' alpha with the room has no dimming: it shows the room
    /// in full for any opacity above 0.
    pub fn set_preferred_passthrough_opacity(&self, opacity: Option<f32>) {
        self.preferred
            .set(opacity.map(|opacity| opacity.clamp(0.0, 1.0)));
    }

    /// The opacity of the room as the headset shows it now, from 0 to 1. Jetpack XR's
    /// `currentPassthroughOpacity`. It differs from the preference while the headset cannot
    /// follow it.
    pub fn current_passthrough_opacity(&self) -> f32 {
        self.current.get()
    }

    // The preference as the backend reads it in a frame, outside the composition.
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    pub(crate) fn preference(&self) -> Option<f32> {
        self.preferred.get_non_reactive()
    }

    // Tells the app what the room shows now. A value that stays does not recompose.
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    pub(crate) fn show(&self, opacity: f32) {
        // Any change of the value counts, so the comparison is of the exact bits
        if self.current.get_non_reactive().to_bits() != opacity.to_bits() {
            self.current.set(opacity);
        }
    }
}

/// The composition local that holds the app's [`SpatialEnvironment`], or `None` outside a
/// headset. Jetpack XR reaches it through its session's scene.
///
/// ```ignore
/// let environment = local_spatial_environment().current();
/// if let Some(environment) = environment {
///     environment.set_preferred_passthrough_opacity(Some(1.0));
/// }
/// ```
pub fn local_spatial_environment() -> CompositionLocal<Option<SpatialEnvironment>> {
    with_locals(|locals| locals.environment.clone())
}

#[cfg(test)]
#[path = "tests/spatial_environment_tests.rs"]
mod tests;
