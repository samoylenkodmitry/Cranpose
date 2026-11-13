use std::sync::mpsc::Receiver;

use crate::present_runtime::{PresentMsg, PresentState};

#[doc(hidden)]
pub struct InlinePresentRuntime {
    state: PresentState,
    msg_rx: Receiver<PresentMsg>,
    shutdown_seen: bool,
}

impl InlinePresentRuntime {
    pub(crate) fn new(state: PresentState, msg_rx: Receiver<PresentMsg>) -> Self {
        Self {
            state,
            msg_rx,
            shutdown_seen: false,
        }
    }

    pub fn pump(&mut self) -> bool {
        if self.shutdown_seen {
            return false;
        }
        while let Ok(msg) = self.msg_rx.try_recv() {
            if !self.state.run_once(msg) {
                self.shutdown_seen = true;
                return false;
            }
        }
        self.state.consume_waiting();
        true
    }

    pub fn has_waiting_packet(&self) -> bool {
        self.state.has_waiting_packet()
    }

    pub fn step_one_message(&mut self) -> bool {
        if self.shutdown_seen {
            return false;
        }
        match self.msg_rx.try_recv() {
            Ok(msg) => {
                if !self.state.run_once(msg) {
                    self.shutdown_seen = true;
                }
                true
            }
            Err(_) => false,
        }
    }

    pub fn consume_waiting(&mut self) {
        self.state.consume_waiting();
    }
}
