//! What a resize of a resizable composable reports.

/// The phase of a resize: Jetpack XR's `SpatialResizeEventType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpatialResizeEventType {
    /// The user grabbed the content's corner handle with a controller.
    Start,
    /// The user resizes the content. Each frame in which the size changes reports one.
    Resizing,
    /// The user let the handle go, or the runtime stopped tracking the controller that held it.
    End,
}

/// A step of a resize that the user makes with a controller: Jetpack XR's `SpatialResizeEvent`.
///
/// The size is in dp, the unit of [`SubspaceModifier::size`](crate::SubspaceModifier::size), and
/// is whole dp. Jetpack XR reports it in integer pixels. The content's center stays where it is,
/// so a resize has no pose.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct SpatialResizeEvent {
    /// The phase of the resize.
    pub event_type: SpatialResizeEventType,
    /// The width and the height that the user gives the content. In a
    /// [`Start`](SpatialResizeEventType::Start) event it is the content's size at the grab. In an
    /// [`End`](SpatialResizeEventType::End) event it is the size of the last step.
    pub size: [f32; 2],
}
