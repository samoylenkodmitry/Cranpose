use std::cell::{Cell, RefCell};

use cranpose_core::{NodeId, collections::map::HashSet};

pub(crate) struct DirtyNodeQueue {
    pending: RefCell<HashSet<NodeId>>,
    idle_batch: Cell<Option<Vec<NodeId>>>,
}

impl DirtyNodeQueue {
    pub(crate) fn new() -> Self {
        Self {
            pending: RefCell::new(HashSet::default()),
            idle_batch: Cell::new(Some(Vec::new())),
        }
    }

    pub(crate) fn schedule(&self, node_id: NodeId) {
        self.pending.borrow_mut().insert(node_id);
    }

    pub(crate) fn has_pending(&self) -> bool {
        !self.pending.borrow().is_empty()
    }

    pub(crate) fn clear(&self) {
        self.pending.borrow_mut().clear();
    }

    pub(crate) fn process(&self, mut processor: impl FnMut(NodeId)) {
        if !self.has_pending() {
            return;
        }
        let Some(mut batch) = self.idle_batch.take() else {
            return;
        };
        batch.extend(self.pending.borrow_mut().drain());

        let mut next = 0;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            while let Some(&node_id) = batch.get(next) {
                next += 1;
                processor(node_id);
            }
        }));

        if next < batch.len() {
            self.pending.borrow_mut().extend(batch.drain(next..));
        }
        batch.clear();
        self.idle_batch.set(Some(batch));

        if let Err(payload) = result {
            std::panic::resume_unwind(payload);
        }
    }
}

#[cfg(test)]
#[path = "tests/dirty_node_queue_tests.rs"]
mod tests;
