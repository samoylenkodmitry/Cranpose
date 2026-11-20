//! The entitlement check as the loop runs it.

use std::time::Instant;

use super::host::Host;
use crate::{
    Entitlement, EntitlementCheck,
    entitlement_watch::{EntitlementWatch, Verdict},
};

// One run of the check, and what the loop knows of its progress. The app's handle on the check
// lives in the composition, which does not exist yet when the check starts, so each turn gets it.
pub(crate) struct EntitlementRun {
    watch: EntitlementWatch,
    started: Instant,
}

impl EntitlementRun {
    // Starts the check when the build has one and the host can run it: an Android build with the
    // `meta-entitlement` feature. Any other build gets `None` and pays nothing.
    pub(crate) fn start(host: &Host) -> Option<Self> {
        host.begin_entitlement().then(|| Self {
            watch: EntitlementWatch::default(),
            started: Instant::now(),
        })
    }

    // One turn of the loop: takes the platform's answer when there is one, tells the app and ends
    // the app when its policy says so. The loop's exit request then ends the session and the
    // activity, as for any app that closes itself.
    pub(crate) fn poll(&mut self, host: &Host, check: &EntitlementCheck) {
        if self.watch.settled() {
            return;
        }
        let verdict = self.watch.step(self.started.elapsed(), check.policy(), || {
            host.entitlement_answer()
        });
        if let Some(verdict) = verdict {
            settle(host, check, verdict);
        }
    }
}

fn settle(host: &Host, check: &EntitlementCheck, verdict: Verdict) {
    check.publish(verdict.entitlement);
    let why = if verdict.timed_out {
        "the platform gave no answer in time".to_owned()
    } else {
        host.entitlement_detail()
    };
    match verdict.entitlement {
        Entitlement::Entitled => {
            log::info!("cranpose xr: entitlement check: Entitled ({why})");
        }
        _ if verdict.quit => {
            log::warn!("cranpose xr: entitlement check: NotEntitled ({why}); the app quits");
            cranpose::request_exit();
        }
        _ => {
            log::warn!("cranpose xr: entitlement check: NotEntitled ({why}); the app goes on");
        }
    }
}
