//! The app's request to close, which the loop of an immersive app honors.
//!
//! An app calls `request_exit` of `cranpose_services`, as in a flat app. The loop asks the runtime
//! to end the running session (`xrRequestExitSession`). The runtime moves the session to STOPPING,
//! the backend ends it, and the loop leaves. The host then ends the activity. The request stays
//! recorded until the host's way of closing has landed, so a close that fails is not lost.

use cranpose::{exit_requested, take_exit_request};

// What the loop does about the app's exit request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExitStep {
    // Nothing: the app asked for no exit, or the runtime stops the session already.
    Wait,
    // Ask the runtime to end the running session.
    EndSession,
    // The session ended or never began: the loop leaves.
    Leave,
}

// The request's progress through the loop.
#[derive(Debug, Default)]
pub(crate) struct ExitRequest {
    // Whether the runtime has been asked to end the session. It is asked once.
    asked: bool,
}

impl ExitRequest {
    // The step for the loop's turn. `session_runs` says whether the session has begun and has not
    // ended.
    pub(crate) fn poll(&mut self, session_runs: bool) -> ExitStep {
        if !exit_requested() {
            return ExitStep::Wait;
        }
        if !session_runs {
            return ExitStep::Leave;
        }
        if std::mem::replace(&mut self.asked, true) {
            ExitStep::Wait
        } else {
            ExitStep::EndSession
        }
    }
}

// Ends the host with `finish`, once the loop has left, and takes the app's request only if it
// landed. Returns whether it did.
pub(crate) fn leave(finish: impl FnOnce() -> bool) -> bool {
    if !exit_requested() {
        return false;
    }
    let landed = finish();
    if landed {
        let _ = take_exit_request();
    }
    landed
}

#[cfg(test)]
#[path = "tests/exit_request_tests.rs"]
mod tests;
