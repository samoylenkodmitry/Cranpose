//! How far an anchor has come in being kept for the next start.

use crate::Uuid;

/// Where an [`Anchor`](crate::Anchor) stands in being kept for the app's next start. Jetpack XR's
/// `Anchor.persist` is a suspending call that returns the identifier. This is its state as
/// the app reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnchorPersistence {
    /// Nothing asked the runtime to keep the anchor.
    #[default]
    NotPersisted,
    /// [`Anchor::persist`](crate::Anchor::persist) asked, and the runtime has not finished.
    Persisting,
    /// The runtime keeps the anchor under this identifier. A later start loads it with
    /// [`Anchors::load`](crate::Anchors::load).
    Persisted(Uuid),
    /// The runtime refused to keep the anchor, or the app cannot persist anchors: its build does
    /// not declare `Use::anchors()`, the runtime has no way to keep them or the user refused the
    /// permission.
    Failed,
}
