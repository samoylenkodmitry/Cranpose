//! The kind of input that drives a hand's pointer.

/// The kind of input that drives the pointer of one hand. Jetpack XR's `InputEvent.Source`, whose
/// `SOURCE_CONTROLLER` and `SOURCE_HANDS` these are.
///
/// Jetpack XR tags each input event with its source. The [`InputEvent`](crate::InputEvent) of a
/// volume's ray has it. The pointer's events for the composition and for panels reach the app as
/// plain pointer events with no source, so Cranpose also tags the frame:
/// [`XrFrame::pointer`](crate::XrFrame::pointer) tells which input points for a hand. Jetpack XR's
/// `SOURCE_HEAD`, `SOURCE_MOUSE` and `SOURCE_GAZE_AND_GESTURE` have no counterpart here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InputSource {
    /// A controller's aim pose casts the ray, and its trigger presses.
    Controller,
    /// The aim pose of a tracked hand casts the ray, and the pinch of its index finger presses.
    Hands,
}
