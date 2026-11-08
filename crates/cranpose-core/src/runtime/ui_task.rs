use std::rc::Rc;

use super::RuntimeHandle;

// Work queued for the runtime thread: a closure that runs once, or a task
// its owner keeps and queues again without allocating.
pub(super) enum UiTask {
    Once(Box<dyn FnOnce() + 'static>),
    Shared(Rc<dyn Fn() + 'static>),
}

impl UiTask {
    pub(super) fn run(self) {
        match self {
            Self::Once(task) => task(),
            Self::Shared(task) => task(),
        }
    }
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the runtime's task entry points live with the task they queue"
)]
impl RuntimeHandle {
    /// Schedules work that must run on the runtime thread.
    ///
    /// The closure executes on the UI thread immediately when the runtime
    /// drains its local queue, so it may capture `Rc`/`RefCell` values. Calling
    /// this from any other thread is a logic error and will panic in debug
    /// builds via the inner assertion.
    pub fn enqueue_ui_task(&self, task: Box<dyn FnOnce() + 'static>) {
        if let Some(inner) = self.inner.upgrade() {
            inner.enqueue_ui_task(UiTask::Once(task));
        } else {
            task();
        }
    }

    /// Schedules `task`, which its owner keeps and schedules again, to run on
    /// the runtime thread, as [`RuntimeHandle::enqueue_ui_task`] runs a
    /// closure once. Scheduling it clones the `Rc` and allocates nothing.
    pub fn enqueue_shared_ui_task(&self, task: Rc<dyn Fn() + 'static>) {
        match self.inner.upgrade() {
            Some(inner) => inner.enqueue_ui_task(UiTask::Shared(task)),
            None => task(),
        }
    }
}
