//! What the entitlement check of the Meta Horizon Store found.

/// Whether the viewer owns the app, as the entitlement check of the Meta Horizon Store tells.
/// Meta's Platform SDK answers it. Jetpack XR has no equivalent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Entitlement {
    /// The check runs. The app has no answer yet. A check that gets no answer for 30 seconds
    /// counts as [`NotEntitled`](Self::NotEntitled).
    #[default]
    Checking,
    /// The viewer owns the app.
    Entitled,
    /// The viewer does not own the app, or the check failed. A failure is the Platform SDK that
    /// did not initialize, an app id that the store does not know, a build that lacks the
    /// `meta-entitlement` service or no answer in time. Meta's guide counts each as a failed
    /// check.
    NotEntitled,
}
