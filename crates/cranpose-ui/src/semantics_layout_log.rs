use std::cell::{Cell, RefCell};

use cranpose_core::NodeId;

thread_local! {
    static ARMED_LOGS: Cell<usize> = const { Cell::new(0) };
}

const MAX_PENDING: usize = 4096;

pub(crate) struct SemanticsLayoutLog {
    armed: Cell<bool>,
    epoch: Cell<u64>,
    sync: Cell<u64>,
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
            sync: Cell::new(1),
            pending: RefCell::new(Vec::new()),
        }
    }
}

impl SemanticsLayoutLog {
    fn set_armed(&self, armed: bool) {
        if self.armed.replace(armed) == armed {
            return;
        }
        let _ = ARMED_LOGS.try_with(|logs| {
            logs.set(if armed {
                logs.get() + 1
            } else {
                logs.get().saturating_sub(1)
            });
        });
    }

    fn record(&self, node: NodeId, stamp: &mut u64) {
        let sync = self.sync.get();
        if !self.armed.get() || *stamp == sync {
            return;
        }
        *stamp = sync;
        let mut pending = self.pending.borrow_mut();
        if pending.len() < MAX_PENDING {
            pending.push(node);
            return;
        }
        pending.clear();
        drop(pending);
        self.epoch.set(self.epoch.get().wrapping_add(1));
        self.set_armed(false);
    }

    pub(crate) fn begin_sync(&self, changed: &mut Vec<NodeId>) -> LayoutSync {
        let continuous = self.armed.get();
        changed.clear();
        std::mem::swap(&mut *self.pending.borrow_mut(), changed);
        self.sync.set(self.sync.get().wrapping_add(1));
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

pub(crate) fn record_layout_change(node: NodeId, stamp: &mut u64) {
    if ARMED_LOGS.try_with(Cell::get).unwrap_or(0) == 0 {
        return;
    }
    crate::render_state::with_current_semantics_layout_log(|log| log.record(node, stamp));
}

#[cfg(test)]
#[path = "tests/semantics_layout_log_tests.rs"]
mod tests;
