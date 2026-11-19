//! Whether the runtime tracks a hand, a face or an eye.

/// Whether the runtime tracks something now. Jetpack XR's `TrackingState`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TrackingState {
    /// The runtime tracks it, and the values of the frame are current.
    Tracking,
    /// The runtime runs a tracker but has no current values, as while a hand is out of view of the
    /// cameras. The values keep what the frame before had.
    Paused,
    /// No tracker runs: the app did not ask for it, the user has not granted the permission or the
    /// runtime offers none.
    #[default]
    Stopped,
}
