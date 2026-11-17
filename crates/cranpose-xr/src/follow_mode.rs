//! What a following subspace follows, and how.

/// How content follows its target. Jetpack XR's `FollowMode`.
///
/// Jetpack XR's `soft` and `tight` follow the target's movement continuously. They are not
/// available here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FollowMode {
    /// The content stands where the target put it once: in front of the target when the
    /// [`FollowingSubspace`](crate::FollowingSubspace) first composes, and again after the runtime
    /// recenters the room. It does not follow later movement. Jetpack XR's `FollowMode.snap`.
    ///
    /// Jetpack XR tracks all dimensions of the target by default. This mode tracks the target's
    /// place and its turn about the vertical axis, so the content stands upright.
    Snap,
}

/// What a [`FollowingSubspace`](crate::FollowingSubspace) follows. Jetpack XR's `FollowTarget`.
///
/// Jetpack XR's `FollowTarget.anchor` is not available here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FollowTarget {
    /// The user's view, which the head's pose gives. The content is head-locked as `mode` says.
    /// Jetpack XR's `FollowTarget.view`.
    View(FollowMode),
}
