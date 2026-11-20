//! The loop's reading of the entitlement check: when it asks the platform and what it does with
//! the answer.
//!
//! The Platform SDK answers on its own thread, and the answer waits in a field. The loop asks for
//! it a few times each second while the check runs and stops asking once it has the answer, so a
//! frame pays for no call. A check that gets no answer for too long counts as failed, because an
//! app must not run unchecked when the platform stays silent.

use std::time::Duration;

use crate::{Entitlement, OnNotEntitled};

// How often the loop asks while the check runs.
const ASK_EVERY: Duration = Duration::from_millis(100);

// How long a check may run before it counts as failed. Meta asks for the check within 10 seconds
// of the launch, and the SDK answers within a few seconds.
pub(crate) const DEADLINE: Duration = Duration::from_secs(30);

// What the platform says about the check. Only an Android build with the `meta-entitlement`
// feature gets an answer other than `Checking`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(any(test, all(feature = "meta-entitlement", target_os = "android"))),
    expect(dead_code, reason = "only the Android entitlement bridge answers")
)]
pub(crate) enum Answer {
    Checking,
    Entitled,
    NotEntitled,
}

// What the loop does once the check has an outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Verdict {
    pub(crate) entitlement: Entitlement,
    // Whether the app ends, by the policy that the app chose
    pub(crate) quit: bool,
    // Whether the platform gave no answer in time
    pub(crate) timed_out: bool,
}

#[derive(Debug, Default)]
pub(crate) struct EntitlementWatch {
    asked: Option<Duration>,
    settled: bool,
}

impl EntitlementWatch {
    // Whether the check has an outcome. The loop asks nothing more then.
    pub(crate) fn settled(&self) -> bool {
        self.settled
    }

    // One turn of the loop, `now` after the check began. Asks the platform when the last ask is
    // old enough. Returns the outcome once, and `None` while the check runs.
    pub(crate) fn step(
        &mut self,
        now: Duration,
        policy: OnNotEntitled,
        ask: impl FnOnce() -> Answer,
    ) -> Option<Verdict> {
        if self.settled {
            return None;
        }
        if self
            .asked
            .is_some_and(|last| now.saturating_sub(last) < ASK_EVERY)
        {
            return None;
        }
        self.asked = Some(now);
        let (entitlement, timed_out) = match ask() {
            Answer::Entitled => (Entitlement::Entitled, false),
            Answer::NotEntitled => (Entitlement::NotEntitled, false),
            Answer::Checking if now >= DEADLINE => (Entitlement::NotEntitled, true),
            Answer::Checking => return None,
        };
        self.settled = true;
        Some(Verdict {
            entitlement,
            quit: entitlement == Entitlement::NotEntitled && policy == OnNotEntitled::Quit,
            timed_out,
        })
    }
}

#[cfg(test)]
#[path = "tests/entitlement_watch_tests.rs"]
mod tests;
