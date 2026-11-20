//! The app's view of the entitlement check, and its choice of what a failure does.

use cranpose::{MutableState, mutableStateOf};
use cranpose_core::CompositionLocal;

use crate::{Entitlement, OnNotEntitled, locals::with_locals};

/// The entitlement check of the Meta Horizon Store as the app sees it: what it found and what the
/// backend does when it finds the viewer not entitled. An app reads it through
/// [`local_entitlement_check`]. Jetpack XR has no equivalent.
///
/// The check exists only in a build that turns on this crate's `meta-entitlement` feature and
/// lists the `meta-entitlement` service in its Gradle build. Whoever turns it on accepts the Meta
/// Platform Technologies SDK License Agreement (<https://developers.meta.com/vr/licenses/oculussdk/>),
/// because the service downloads the Meta Platform SDK.
#[derive(Clone, Copy, PartialEq)]
pub struct EntitlementCheck {
    entitlement: MutableState<Entitlement>,
    on_not_entitled: MutableState<OnNotEntitled>,
}

impl EntitlementCheck {
    // A check that has found nothing yet and quits on a failure.
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    pub(crate) fn new() -> Self {
        Self {
            entitlement: mutableStateOf(Entitlement::Checking),
            on_not_entitled: mutableStateOf(OnNotEntitled::Quit),
        }
    }

    /// What the check found so far. A composition that reads it composes again when the answer
    /// comes.
    pub fn entitlement(&self) -> Entitlement {
        self.entitlement.get()
    }

    /// What the backend does when the check finds the viewer not entitled. It quits by default.
    pub fn on_not_entitled(&self) -> OnNotEntitled {
        self.on_not_entitled.get()
    }

    /// Chooses what the backend does when the check finds the viewer not entitled. Set it
    /// before the answer comes: the first composition of the content is early enough, because the
    /// check takes at least some milliseconds. A choice that comes after a failure changes
    /// nothing, because the backend has acted on it.
    pub fn set_on_not_entitled(&self, policy: OnNotEntitled) {
        self.on_not_entitled.set(policy);
    }

    // The policy as the backend reads it in a frame, outside the composition.
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    pub(crate) fn policy(&self) -> OnNotEntitled {
        self.on_not_entitled.get_non_reactive()
    }

    // Tells the app what the check found.
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    pub(crate) fn publish(&self, entitlement: Entitlement) {
        if self.entitlement.get_non_reactive() != entitlement {
            self.entitlement.set(entitlement);
        }
    }
}

/// The composition local that holds the app's [`EntitlementCheck`], or `None` where no check
/// runs: in a build without the `meta-entitlement` feature and outside an Android headset.
///
/// ```ignore
/// if let Some(check) = local_entitlement_check().current() {
///     // A demo goes on without the store's answer and shows what the answer was
///     check.set_on_not_entitled(OnNotEntitled::Continue);
///     Text(format!("{:?}", check.entitlement()), Modifier::empty(), TextStyle::default());
/// }
/// ```
pub fn local_entitlement_check() -> CompositionLocal<Option<EntitlementCheck>> {
    with_locals(|locals| locals.entitlement_check.clone())
}

#[cfg(test)]
#[path = "tests/entitlement_check_tests.rs"]
mod tests;
