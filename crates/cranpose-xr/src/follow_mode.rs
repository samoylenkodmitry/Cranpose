//! What a following subspace follows, and how.

use crate::{Anchor, Hand, SoftFollowMode};

/// The dimensions of the target's pose that a following subspace tracks. Jetpack XR's
/// `TrackedDimensions`.
///
/// Jetpack XR's `TrackedDimensions` holds six flags, one for each axis of place and turn, and
/// defaults to `TrackedDimensions.All`. Here two sets of them are available. Other sets of
/// Jetpack XR's flags, such as `TrackedDimensions.RotationOnly`, are not available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TrackedDimensions {
    /// All six dimensions: the origin has the target's whole pose, so the content turns with the
    /// target about every axis. A panel at a controller then holds its place against the
    /// controller as the hand tilts and rolls. Jetpack XR's `TrackedDimensions.All`.
    All,
    /// The target's place on its three axes and its turn about the vertical axis. Its pitch and
    /// roll are not tracked, so the content stands upright. A ball that sits 10 cm above a
    /// controller stays straight above it when the hand tilts. Jetpack XR spells it
    /// `TrackedDimensions(isXTracked = true, isYTracked = true, isZTracked = true,
    /// isYawTracked = true)`.
    ///
    /// The turn is the heading of the target's forward axis seen from above. A target whose
    /// forward axis is within about 6 degrees of vertical has no heading, so the content keeps the
    /// heading that it had then.
    TranslationAndYaw,
}

/// How content follows its target, and which dimensions of the target's pose it tracks. Jetpack
/// XR's `FollowMode`, whose `snap`, `tight` and `soft` take a [`TrackedDimensions`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FollowMode {
    /// The content stands where the target put it once: where the target is when the
    /// [`FollowingSubspace`](crate::FollowingSubspace) first composes, or when the runtime first
    /// tracks the target after that. The runtime's recenter of the room places it again. It does
    /// not follow later movement. Jetpack XR's `FollowMode.snap(dimensions)`.
    Snap(TrackedDimensions),
    /// The content follows the target in every frame, with no lag: its origin has the target's
    /// pose in the tracked dimensions as the frame shows it. Jetpack XR's
    /// `FollowMode.tight(dimensions)`.
    ///
    /// The follow costs no composition. It moves the panels' poses in the headset's frame loop.
    /// With [`TrackedDimensions::All`] the headset hands the panels of a view or a controller to
    /// the compositor in the space of the head or of the controller's aim. The compositor then
    /// places them with the newest pose that it has of the head or the controller, at the time
    /// the frame shows.
    Tight(TrackedDimensions),
    /// The content holds still while the target moves a little, and glides after it once it moved
    /// far enough for long enough, as [`SoftFollowMode`] says. Jetpack XR's `FollowMode.soft`.
    ///
    /// The follow costs no composition. It moves the panels' poses in the headset's frame loop, in
    /// the room's space.
    Soft(SoftFollowMode),
}

impl FollowMode {
    /// The soft follow of `dimensions` with Jetpack XR's defaults: a half life of 175 ms, a start
    /// delay of 200 ms and start thresholds of 0.2 m, 16 degrees of pitch, 24 degrees of yaw and 20
    /// degrees of roll. Jetpack XR's `FollowMode.soft(dimensions)`.
    pub const fn soft(dimensions: TrackedDimensions) -> Self {
        Self::Soft(SoftFollowMode::new(dimensions))
    }

    /// The dimensions of the target's pose that the mode tracks.
    pub const fn dimensions(&self) -> TrackedDimensions {
        match self {
            Self::Snap(dimensions) | Self::Tight(dimensions) => *dimensions,
            Self::Soft(soft) => soft.dimensions,
        }
    }
}

/// What a [`FollowingSubspace`](crate::FollowingSubspace) follows. Jetpack XR's `FollowTarget`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FollowTarget {
    /// The user's view, which the head's pose gives. The content is head-locked as `mode` says.
    /// Its origin stands a meter ahead of the head, facing it: at the head's height with
    /// [`TrackedDimensions::TranslationAndYaw`], or along the head's gaze with
    /// [`TrackedDimensions::All`]. Jetpack XR's `FollowTarget.view`.
    View(FollowMode),
    /// The controller in a hand, as `mode` says. Jetpack XR has no controller target: its
    /// targets are the head and anchors.
    ///
    /// The subspace's origin is where the hand's grip pose is
    /// ([`Controller::grip`](crate::Controller::grip)), turned as the controller's aim pose
    /// ([`Controller::aim`](crate::Controller::aim)) is turned. The aim's forward axis is the ray,
    /// its x axis points to the right of the controller and its y axis up from the controller's
    /// top. A panel with no turn of its own then faces back along the ray, toward a user who holds
    /// the controller in front. The grip pose gives no steady turn: its forward axis runs along
    /// the controller's handle, which points steeply upward in the usual hold.
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
    /// The subspace's origin is the anchor's pose in the tracked dimensions. With
    /// [`TrackedDimensions::All`] an anchor on a wall holds content that lies against it. Until
    /// the runtime places the anchor, the subspace's panels do not show and take no hits: an
    /// anchor that the app created has its place at once, and one that it loaded from storage
    /// waits for the runtime to recognize the room. An anchor that is detached or lost leaves the
    /// content where it was.
    Anchor(Anchor, FollowMode),
}
