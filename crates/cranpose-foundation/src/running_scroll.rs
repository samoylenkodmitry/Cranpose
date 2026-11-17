//! The scroll that moves a scrollable state now. It is a motion after a drag
//! or an animation that the app started. One scroll runs at a time, as under
//! Compose's `MutatorMutex`: a new scroll or a jump stops the running one.

/// Names one scroll that a [`RunningScroll`] ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollId(u64);

/// How a scroll stops. Its state hands it out. The caller runs it once the
/// caller holds no borrow of the state.
#[must_use = "a scroll stops only when its stop runs"]
pub struct ScrollStop(Box<dyn FnOnce()>);

impl ScrollStop {
    /// Stops the scroll.
    pub fn run(self) {
        (self.0)();
    }
}

/// The running scroll of a scrollable state and how to stop it.
#[derive(Default)]
pub struct RunningScroll {
    running: Option<(ScrollId, ScrollStop)>,
    next: u64,
}

impl RunningScroll {
    /// Makes the scroll that `stop` stops the running one. Returns its id
    /// with the stop of the scroll that ran before. The caller runs that stop.
    pub fn start(&mut self, stop: impl FnOnce() + 'static) -> (ScrollId, Option<ScrollStop>) {
        let id = ScrollId(self.next);
        self.next += 1;
        let previous = self.running.replace((id, ScrollStop(Box::new(stop))));
        (id, previous.map(|(_, stop)| stop))
    }

    /// Forgets the scroll `id` while it runs. A scroll that ended on its own
    /// calls it.
    pub fn end(&mut self, id: ScrollId) {
        if self
            .running
            .as_ref()
            .is_some_and(|(running, _)| *running == id)
        {
            self.running = None;
        }
    }

    /// Takes the stop of the running scroll, if one runs.
    pub fn take(&mut self) -> Option<ScrollStop> {
        self.running.take().map(|(_, stop)| stop)
    }

    /// Whether a scroll runs.
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }
}
