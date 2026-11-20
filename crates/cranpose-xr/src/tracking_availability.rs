//! How far a tracking feature has come in a session.

/// How far a [`TrackingFeature`](crate::TrackingFeature) has come in the session. The anchors and
/// the planes of the room report their progress in the same terms
/// ([`AnchorCapabilities`](crate::AnchorCapabilities), [`Planes::availability`](crate::Planes::availability)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TrackingAvailability {
    /// The runtime offers no way to track it: it lists no extension for it, or the headset reports
    /// no sensor for it, as a Quest 3 has no eye cameras.
    #[default]
    Unavailable,
    /// The runtime lists an extension for it, and the app did not ask for it. Nothing is enabled,
    /// so the headset may still lack the sensor. The app learns that when it asks.
    Offered,
    /// The app asked for it, and no tracker runs yet because the user has not granted its
    /// permission. The system's prompt shows, or the runtime has not answered.
    AwaitingPermission,
    /// The app asked for it, and the user refused the permission. Only the app's page in the
    /// system settings can allow it now.
    PermissionRefused,
    /// The app asked for it, and a tracker runs. The frame's values are current.
    Active,
}
