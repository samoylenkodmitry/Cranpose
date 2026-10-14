//! Pointer input dispatcher plumbing.
//!
//! Platform integrations enqueue pointer events here and drain them into the
//! modifier dispatch pipeline in FIFO order.

use super::types::{PointerEvent, PointerId};

/// A first-in first-out queue of pointer events. Platform integrations push events and the shell
/// drains them into modifier dispatch.
#[derive(Default)]
pub struct PointerDispatcher {
    queue: Vec<(PointerId, PointerEvent)>,
}

impl PointerDispatcher {
    /// Creates an empty dispatcher.
    pub fn new() -> Self {
        Self { queue: Vec::new() }
    }

    /// Appends `event` to the queue, keyed by its pointer id.
    pub fn push(&mut self, event: PointerEvent) {
        self.queue.push((event.id, event));
    }

    /// Whether no event is queued.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// The number of queued events.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Drops every queued event without dispatching it.
    pub fn clear(&mut self) {
        self.queue.clear();
    }

    /// Calls `handler` with the pointer id and the event of each queued entry, in the order pushed,
    /// and empties the queue.
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
