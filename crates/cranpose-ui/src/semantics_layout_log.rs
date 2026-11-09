use std::{
    cell::{Cell, RefCell},
    sync::atomic::{AtomicUsize, Ordering},
};

use cranpose_core::{NodeId, collections::node_bits::NodeBits};

const MAX_PENDING: usize = 4096;

/// How many logs are armed on any thread. Most layout changes happen with
/// none, and this answers that with one load where a thread-local read
/// costs a key lookup on targets without native thread-locals.
static ARMED_LOGS: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct SemanticsLayoutLog {
    armed: Cell<bool>,
    epoch: Cell<u64>,
    // The nodes `pending` holds, each once.
    noted: RefCell<NodeBits>,
    pending: RefCell<Vec<NodeId>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LayoutSync {
    pub(crate) epoch: u64,
    pub(crate) continuous: bool,
}

impl Default for SemanticsLayoutLog {
    fn default() -> Self {
        Self {
            armed: Cell::new(false),
            epoch: Cell::new(0),
            noted: RefCell::default(),
            pending: RefCell::new(Vec::new()),
        }
    }
}

impl SemanticsLayoutLog {
    fn set_armed(&self, armed: bool) {
        if self.armed.replace(armed) == armed {
            return;
        }
        if armed {
            ARMED_LOGS.fetch_add(1, Ordering::Relaxed);
        } else {
            ARMED_LOGS.fetch_sub(1, Ordering::Relaxed);
        }
    }

    fn record(&self, node: NodeId) {
        if !self.armed.get() || !self.noted.borrow_mut().insert(node) {
            return;
        }
        let mut pending = self.pending.borrow_mut();
        if pending.len() < MAX_PENDING {
            pending.push(node);
            return;
        }
        pending.clear();
        drop(pending);
        *self.noted.borrow_mut() = NodeBits::default();
        self.epoch.set(self.epoch.get().wrapping_add(1));
        self.set_armed(false);
    }

    pub(crate) fn begin_sync(&self, changed: &mut Vec<NodeId>) -> LayoutSync {
        let continuous = self.armed.get();
        changed.clear();
        std::mem::swap(&mut *self.pending.borrow_mut(), changed);
        let mut noted = self.noted.borrow_mut();
        for &node in changed.iter() {
            noted.remove(node);
        }
        drop(noted);
        self.set_armed(true);
        LayoutSync {
            epoch: self.epoch.get(),
            continuous,
        }
    }
}

impl Drop for SemanticsLayoutLog {
    fn drop(&mut self) {
        self.set_armed(false);
    }
}

/// Notes that layout changed `node`, for the semantics logs that are armed.
/// The check that none is armed is inlined where a layout state changes.
#[inline]
pub(crate) fn record_layout_change(node: NodeId) {
    if ARMED_LOGS.load(Ordering::Relaxed) != 0 {
        record_armed_layout_change(node);
    }
}

#[inline(never)]
fn record_armed_layout_change(node: NodeId) {
    crate::render_state::with_current_semantics_layout_log(|log| log.record(node));
}

#[cfg(test)]
#[path = "tests/semantics_layout_log_tests.rs"]
mod tests;
