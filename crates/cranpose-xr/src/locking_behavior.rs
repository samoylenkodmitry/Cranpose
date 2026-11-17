//! How a user subspace stays with the user.

/// How the content of a [`UserSubspace`](crate::UserSubspace) follows the user's head. Jetpack
/// XR's `LockingBehavior`.
///
/// Jetpack XR's `soft` follows the head's movement continuously. It is not available here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum LockingBehavior {
    /// The content stands where the user's head put it once: in front of the head when the
    /// `UserSubspace` first composes, and again in front of the head after the runtime recenters
    /// the room. It does not follow later movement of the head. Jetpack XR's `static`.
    #[default]
    Static,
}
