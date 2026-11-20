//! What the runtime offers of anchors and how far each has come.

use crate::TrackingAvailability;

/// What the runtime offers of anchors, and how far each part has come. An app reads it through
/// [`Anchors::capabilities`](crate::Anchors::capabilities). Jetpack XR reports the same through the
/// result of `Session.configure`.
///
/// An app that does not declare `Use::anchors()` in its build gets
/// [`Offered`](TrackingAvailability::Offered) or [`Unavailable`](TrackingAvailability::Unavailable):
/// the backend enables no extension and makes no call for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct AnchorCapabilities {
    /// Whether the app can create anchors. [`Active`](TrackingAvailability::Active) once the
    /// runtime's extension runs.
    pub anchors: TrackingAvailability,
    /// Whether the app can keep anchors for its next start and load them. The runtime's storage
    /// can ask for a permission of its own, so this can lag behind [`anchors`](Self::anchors) or
    /// differ from it.
    pub persistence: TrackingAvailability,
    /// How many anchors the runtime holds at a time, or `None` while it names no limit.
    pub max_anchors: Option<u32>,
}
