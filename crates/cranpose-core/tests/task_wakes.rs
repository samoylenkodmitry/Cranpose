use std::{
    cell::Cell,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};

use cranpose_core::{Composition, MemoryApplier};

/// Ready once another thread sets it, waking the task that waits.
#[derive(Clone, Default)]
struct Signal(Arc<(AtomicBool, Mutex<Option<Waker>>)>);

impl Signal {
    fn set(&self) {
        self.0.0.store(true, Ordering::Release);
        if let Some(waker) = self
            .0
            .1
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            waker.wake();
        }
    }
}

impl Future for Signal {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        *self
            .0
            .1
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cx.waker().clone());
        if self.0.0.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[test]
fn a_task_woken_from_another_thread_runs_after_polls_that_found_nothing() {
    let composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let signal = Signal::default();
    let done = Rc::new(Cell::new(false));
    let finished = Rc::clone(&done);
    let waited = signal.clone();
    let _task = runtime.spawn_ui(async move {
        waited.await;
        finished.set(true);
    });
    // The task waits; later polls find no task woken.
    for _ in 0..3 {
        runtime.drain_ui();
    }
    assert!(!done.get());

    let late = signal.clone();
    std::thread::spawn(move || signal.set())
        .join()
        .expect("the waking thread ends");
    runtime.drain_ui();
    assert!(done.get(), "the woken task did not run");

    // The finished task's waker wakes once more: once drained, the runtime
    // has no UI work left.
    late.set();
    runtime.drain_ui();
    assert!(
        !runtime.has_pending_ui(),
        "a stale wake keeps UI work pending"
    );
}

#[test]
fn work_queued_by_a_resumed_task_runs_in_the_drain_that_resumed_it() {
    let composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let signal = Signal::default();
    let ran = Rc::new(Cell::new(false));
    let queued = Rc::clone(&ran);
    let waited = signal.clone();
    let queue = runtime.clone();
    let _task = runtime.spawn_ui(async move {
        waited.await;
        queue.enqueue_ui_task(Box::new(move || queued.set(true)));
    });
    // The task waits, and a drain with nothing queued runs nothing.
    runtime.drain_ui();
    assert!(!ran.get());
    assert!(!runtime.has_pending_ui());

    std::thread::spawn(move || signal.set())
        .join()
        .expect("the waking thread ends");
    runtime.drain_ui();
    assert!(
        ran.get(),
        "the resumed task's work waited for another drain"
    );
    assert!(!runtime.has_pending_ui());
}

#[test]
fn a_queued_task_runs_while_a_stale_wake_is_all_that_else_waits() {
    let composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let signal = Signal::default();
    let waited = signal.clone();
    let _task = runtime.spawn_ui(waited);
    runtime.drain_ui();
    let late = signal.clone();
    signal.set();
    runtime.drain_ui();
    // The finished task's waker wakes once more, then a task is queued that
    // queues another: one drain runs both.
    late.set();
    let count = Rc::new(Cell::new(0));
    let (first, second) = (Rc::clone(&count), Rc::clone(&count));
    let queue = runtime.clone();
    runtime.enqueue_ui_task(Box::new(move || {
        first.set(first.get() + 1);
        queue.enqueue_ui_task(Box::new(move || second.set(second.get() + 1)));
    }));
    runtime.drain_ui();
    assert_eq!(count.get(), 2, "a queued task waited for another drain");
    assert!(!runtime.has_pending_ui());
}
