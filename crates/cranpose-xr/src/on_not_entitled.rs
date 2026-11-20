//! What the app does when the entitlement check fails.

/// What the backend does when the entitlement check finds the viewer not entitled. Jetpack XR has
/// no equivalent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OnNotEntitled {
    /// The backend asks the app to close with [`request_exit`](cranpose::request_exit): the
    /// session ends and the activity finishes. Meta's rules for store apps need an app that fails
    /// the check to quit, so this is the default.
    #[default]
    Quit,
    /// The app goes on. [`EntitlementCheck::entitlement`] tells it `NotEntitled`, so that it
    /// can limit itself, as a demo does. Meta's guide allows this.
    ///
    /// [`EntitlementCheck::entitlement`]: crate::EntitlementCheck::entitlement
    Continue,
}
