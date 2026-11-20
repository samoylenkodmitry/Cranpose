//! What creating or loading an anchor gave.

use crate::Anchor;

/// The result of creating or loading an anchor. Jetpack XR's `AnchorResult`, whose subtypes are
/// `AnchorCreateSuccess`, `AnchorCreateResourcesExhausted` and `AnchorCreateTrackingUnavailable`.
/// Where Jetpack XR throws, this type has a variant.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnchorResult {
    /// The anchor exists. The runtime holds it from the next frame on, and until then its
    /// [`AnchorState`](crate::AnchorState) is [`Paused`](crate::TrackingState::Paused). Jetpack
    /// XR's `AnchorCreateSuccess`.
    Success(Anchor),
    /// The runtime allows no more anchors at a time. The app detaches one it does not use and
    /// tries again. Jetpack XR's `AnchorCreateResourcesExhausted`.
    ResourcesExhausted,
    /// The runtime does not track the head now, so it can place no anchor. The state may come
    /// back. Jetpack XR's `AnchorCreateTrackingUnavailable`.
    TrackingUnavailable,
    /// The app cannot have anchors: its build does not declare `Use::anchors()`, the runtime has
    /// no extension for them or the user refused the permission. Jetpack XR throws an
    /// `IllegalStateException` for a session whose configuration has the feature off.
    /// [`Anchors::capabilities`](crate::Anchors::capabilities) says which.
    Unavailable,
    /// [`Anchors::load`](crate::Anchors::load) found no persisted anchor with that identifier.
    /// Jetpack XR throws `AnchorInvalidUuidException`.
    InvalidUuid,
}

impl AnchorResult {
    /// The anchor of a [`Success`](Self::Success), or `None` for any other result.
    pub fn anchor(&self) -> Option<&Anchor> {
        match self {
            Self::Success(anchor) => Some(anchor),
            _ => None,
        }
    }

    /// The anchor of a [`Success`](Self::Success), or `None` for any other result.
    pub fn into_anchor(self) -> Option<Anchor> {
        match self {
            Self::Success(anchor) => Some(anchor),
            _ => None,
        }
    }
}
