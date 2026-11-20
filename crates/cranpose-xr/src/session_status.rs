//! Where an OpenXR session stands, and what the app does in each place.
//!
//! Meta's Virtual Reality Checks (VRC.Quest.Input.4 and VRC.Quest.Functional.2) give two rules.
//! An immersive app that has lost input focus keeps rendering, hides the user's hands and
//! controllers and ignores their input. An app that the system pauses stops its interactive
//! elements. The runtime says which of the two holds through the session's state.

use cranpose::{LifecycleState, window_lifecycle_state};

// The states of an OpenXR session (`XrSessionState`) that the backend tells apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionState {
    Idle,
    Ready,
    Synchronized,
    Visible,
    Focused,
    Stopping,
    LossPending,
    Exiting,
}

// Where the session stands after its events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionStatus {
    // The session has not begun or has ended: the app waits.
    Idle,
    // The session runs and the runtime shows none of its layers: the app ends frames with no
    // layers.
    Hidden,
    // The runtime shows the app's layers, with something over them such as the system menu. The
    // app gets no input.
    Visible,
    // The runtime shows the app's layers and sends the headset's input to the app.
    Focused,
    // The runtime ends the session: the app ends.
    Exit,
}

impl SessionStatus {
    // The status that the runtime's state puts the app in. READY and SYNCHRONIZED are a session
    // that runs and is not shown yet. STOPPING is the runtime's request to end the session, which
    // the app has ended by the time it asks for the status, so it counts as idle.
    pub(crate) fn of(state: SessionState) -> Self {
        match state {
            SessionState::Idle | SessionState::Stopping => Self::Idle,
            SessionState::Ready | SessionState::Synchronized => Self::Hidden,
            SessionState::Visible => Self::Visible,
            SessionState::Focused => Self::Focused,
            SessionState::LossPending | SessionState::Exiting => Self::Exit,
        }
    }

    // Whether the session runs: the app begins and ends frames.
    pub(crate) fn runs(self) -> bool {
        matches!(self, Self::Hidden | Self::Visible | Self::Focused)
    }

    // Whether the runtime shows the app's layers.
    pub(crate) fn shows(self) -> bool {
        matches!(self, Self::Visible | Self::Focused)
    }

    // Whether the app gets the controllers' input.
    pub(crate) fn listens(self) -> bool {
        self == Self::Focused
    }

    // The lifecycle that the app sees, as Compose sees a window: resumed while the app has input
    // focus, paused while it shows without it, stopped while the runtime shows none of it and
    // destroyed once the session ends. A session that has not begun is stopped.
    pub(crate) fn lifecycle(self) -> LifecycleState {
        match self {
            Self::Exit => LifecycleState::Destroyed,
            status => window_lifecycle_state(status.shows(), status.listens()),
        }
    }

    // The aims that the app may use: both, with input focus, and none without it. The rule holds in
    // one place, so that no way of pointing, such as a hand or a controller, can bypass it.
    pub(crate) fn input<T>(self, aims: [Option<T>; 2]) -> [Option<T>; 2] {
        if self.listens() { aims } else { [None, None] }
    }
}

// A change of the session's status, for the log: the status that the session leaves, how many
// frames the app ran in it and the lifecycle that the app now sees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StatusChange {
    pub(crate) from: SessionStatus,
    pub(crate) to: SessionStatus,
    pub(crate) frames: u32,
    pub(crate) lifecycle: LifecycleState,
}

// What the loop keeps to tell the changes of the status: the status of the turn before and the
// frames run in it.
#[derive(Debug)]
pub(crate) struct StatusWatch {
    status: SessionStatus,
    frames: u32,
}

impl Default for StatusWatch {
    fn default() -> Self {
        Self {
            status: SessionStatus::Idle,
            frames: 0,
        }
    }
}

impl StatusWatch {
    // Counts a frame that the app ran in the current status.
    pub(crate) fn ran(&mut self) {
        self.frames = self.frames.saturating_add(1);
    }

    // The change, if `status` differs from the status of the turn before.
    pub(crate) fn change(&mut self, status: SessionStatus) -> Option<StatusChange> {
        if status == self.status {
            return None;
        }
        let change = StatusChange {
            from: self.status,
            to: status,
            frames: std::mem::take(&mut self.frames),
            lifecycle: status.lifecycle(),
        };
        self.status = status;
        Some(change)
    }
}

#[cfg(test)]
#[path = "tests/session_status_tests.rs"]
mod tests;
