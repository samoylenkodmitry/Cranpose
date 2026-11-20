//! What a following subspace follows, and how.

use crate::{Anchor, Hand};

/// How content follows its target. Jetpack XR's `FollowMode`.
///
/// Jetpack XR's `FollowMode.soft`, which lags behind the target, is not available here.
///
/// Jetpack XR's `snap` and `tight` take a `TrackedDimensions` that chooses the tracked axes, and
/// track all six by default. Here the dimensions are fixed: the target's place on its three axes
/// and its turn about the vertical axis. Its pitch and roll are not tracked, so the content
/// stands upright. A ball that sits 10 cm above a controller stays above it when the hand tilts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FollowMode {
    /// The content stands where the target put it once: where the target is when the
    /// [`FollowingSubspace`](crate::FollowingSubspace) first composes, or when the runtime first
    /// tracks the target after that. The runtime's recenter of the room places it again. It does
    /// not follow later movement. Jetpack XR's `FollowMode.snap`.
    Snap,
    /// The content follows the target in every frame, with no lag: its origin has the target's
    /// place and heading as the frame shows them. Jetpack XR's `FollowMode.tight`.
    ///
    /// The follow costs no composition. It moves the panels' poses in the headset's frame loop.
    Tight,
}

/// What a [`FollowingSubspace`](crate::FollowingSubspace) follows. Jetpack XR's `FollowTarget`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FollowTarget {
    /// The user's view, which the head's pose gives. The content is head-locked as `mode` says.
    /// Its origin stands a meter ahead of the head, at the head's height and facing it. Jetpack
    /// XR's `FollowTarget.view`.
    View(FollowMode),
    /// The controller in a hand, as `mode` says. Jetpack XR has no controller target: its
    /// targets are the head and anchors.
    ///
    /// The subspace's origin is where the hand's grip pose is
    /// ([`Controller::grip`](crate::Controller::grip)). It turns about the vertical axis with the
    /// heading of the controller's aim pose ([`Controller::aim`](crate::Controller::aim)), which
    /// is the direction of the ray seen from above. The grip pose has no steady heading: its
    /// forward axis runs along the controller's handle, which points steeply upward in the usual
    /// hold. A ray that points straight up or down has no heading. The content keeps the heading
    /// that it had then.
    ///
    /// While the runtime does not track the controller, the subspace's panels do not show and
    /// take no hits. An app that has no input focus gets no controller poses, so they do not show
    /// then either. A `Snap` subspace that placed its panels once shows them from then on.
    ///
    /// A controller's own ray ignores the panels of a subspace that follows that controller, so
    /// the left ray passes through a ball above the left controller. The other hand's ray hits
    /// and presses the ball as it does any panel.
    Controller(Hand, FollowMode),
    /// An anchor, as `mode` says. The content stands in the place that the runtime holds still
    /// for the anchor, so it stays on a table or a wall while the runtime learns more of the room.
    /// Jetpack XR's `FollowTarget.anchor`.
    ///
    /// The subspace's origin is the anchor's pose with all six degrees of freedom, so an anchor on
    /// a wall holds content that lies against it. The view and the controller give only a place and
    /// a heading. Until the runtime places the anchor, the subspace's panels do not show and take
    /// no hits: an anchor that the app created has its place at once, and one that it loaded from
    /// storage waits for the runtime to recognize the room. An anchor that is detached or lost
    /// leaves the content where it was.
    Anchor(Anchor, FollowMode),
}
