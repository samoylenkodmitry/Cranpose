//! The answers of `CranposeMetaEntitlement`, which the Java side numbers and the Rust side names.

/// Where the entitlement check of the Meta Horizon Store stands. Meta has no equivalent in
/// Jetpack Compose or Jetpack XR.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(all(
        feature = "meta-entitlement",
        feature = "renderer-wgpu",
        target_os = "android"
    )),
    expect(
        unreachable_pub,
        reason = "only an Android build with the entitlement bridge exports the answer"
    )
)]
pub enum MetaEntitlement {
    /// The application's build carries no `CranposeMetaEntitlement`: its Gradle build lists no
    /// `meta-entitlement` service, or the Platform SDK failed to load.
    Unavailable,
    /// `begin_meta_entitlement` has not run in this process.
    NotStarted,
    /// The check runs. The Platform SDK answers within a few seconds.
    Checking,
    /// The viewer owns the application.
    Entitled,
    /// The viewer does not own the application, or the check failed: the SDK did not
    /// initialize, the app id is wrong or an exception ended the check. Meta's guide counts every
    /// failure as a failed check.
    NotEntitled,
}

impl MetaEntitlement {
    // The answer of a call to the Java side: no number means the call itself failed, as it does
    // when the application's build carries no `CranposeMetaEntitlement`.
    pub(crate) const fn from_answer(answer: Option<i32>) -> Self {
        match answer {
            Some(state) => Self::from_state(state),
            None => Self::Unavailable,
        }
    }

    // The answer `CranposeMetaEntitlement.cranposeMetaEntitlementState` gives: 0 not started,
    // 1 running, 2 entitled, 3 not entitled.
    pub(crate) const fn from_state(state: i32) -> Self {
        match state {
            1 => Self::Checking,
            2 => Self::Entitled,
            3 => Self::NotEntitled,
            _ => Self::NotStarted,
        }
    }
}

#[cfg(test)]
#[path = "tests/meta_entitlement_wire_tests.rs"]
mod tests;
