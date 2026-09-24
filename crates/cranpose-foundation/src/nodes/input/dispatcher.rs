//! Pointer input dispatcher plumbing.
//!
//! Platform integrations enqueue pointer events here and drain them into the
//! modifier dispatch pipeline in FIFO order.

use super::types::{PointerEvent, PointerId};

#[derive(Default)]
pub struct PointerDispatcher {
    queue: Vec<(PointerId, PointerEvent)>,
}

impl PointerDispatcher {
    pub fn new() -> Self {
        Self { queue: Vec::new() }
    }

    pub fn push(&mut self, event: PointerEvent) {
        self.queue.push((event.id, event));
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn clear(&mut self) {
        self.queue.clear();
    }

    pub fn drain<F>(&mut self, mut handler: F)
    where
        F: FnMut(PointerId, PointerEvent),
    {
        for (id, event) in self.queue.drain(..) {
            handler(id, event);
        }
    }
}

#[cfg(test)]
#[path = "tests/dispatcher_tests.rs"]
mod tests;
