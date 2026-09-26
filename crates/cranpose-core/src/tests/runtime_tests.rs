use super::*;

#[test]
fn state_arena_alloc_skips_free_slot_outside_cell_storage() {
    let runtime = TestRuntime::new();
    let arena = StateArena::default();
    let first = arena.alloc(1_i32, runtime.handle());
    arena.inner.borrow_mut().free.push(u32::MAX);

    let second = arena.alloc(2_i32, runtime.handle());

    assert_eq!(first.slot(), 0);
    assert_eq!(second.slot(), 1);
    assert!(arena.get_typed_opt::<i32>(first).is_some());
    assert!(arena.get_typed_opt::<i32>(second).is_some());
}

#[test]
fn state_arena_alloc_skips_occupied_free_slot() {
    let runtime = TestRuntime::new();
    let arena = StateArena::default();
    let first = arena.alloc(1_i32, runtime.handle());
    arena.inner.borrow_mut().free.push(first.slot());

    let second = arena.alloc(2_i32, runtime.handle());

    assert_ne!(first.slot(), second.slot());
    assert_eq!(second.slot(), 1);
    assert!(arena.get_typed_opt::<i32>(first).is_some());
    assert!(arena.get_typed_opt::<i32>(second).is_some());
}

#[test]
fn state_arena_register_lease_ignores_stale_id() {
    let runtime = TestRuntime::new();
    let arena = StateArena::default();
    let stale_id = StateId::new(99, 0);
    let lease = Rc::new(StateHandleLease {
        id: stale_id,
        runtime: runtime.handle(),
    });

    arena.register_lease(stale_id, &lease);

    assert!(arena.retain_lease(stale_id).is_none());
}

#[test]
fn ui_continuation_type_mismatch_is_ignored_until_matching_payload() {
    let runtime = TestRuntime::new();
    let handle = runtime.handle();
    let received = Rc::new(Cell::new(None));
    let received_for_continuation = Rc::clone(&received);
    let cont_id = handle
        .register_ui_cont(move |value: u32| {
            received_for_continuation.set(Some(value));
        })
        .expect("test runtime is alive");

    handle.dispatcher().post_invoke(cont_id, "wrong payload");
    handle.drain_ui();

    assert_eq!(received.get(), None);
    assert_eq!(handle.debug_stats().ui_conts_len, 1);

    handle.dispatcher().post_invoke(cont_id, 42_u32);
    handle.drain_ui();

    assert_eq!(received.get(), Some(42));
    assert_eq!(handle.debug_stats().ui_conts_len, 0);
}

#[test]
fn work_a_polled_task_queues_runs_in_the_same_drain() {
    let runtime = TestRuntime::new();
    let handle = runtime.handle();
    let ran = Rc::new(Cell::new(false));
    let queued = Rc::clone(&ran);
    let queue = handle.clone();
    let _task = handle.spawn_ui(async move {
        queue.enqueue_ui_task(Box::new(move || queued.set(true)));
        std::future::pending::<()>().await;
    });

    handle.drain_ui();

    assert!(
        ran.get(),
        "a state observer's notice queued by a task belongs to the drain that polled it"
    );
}

/// Counts its polls, queues a notice on each, and wakes itself again.
struct Restless {
    polls: Rc<Cell<u32>>,
    notices: Rc<Cell<u32>>,
    queue: RuntimeHandle,
}

impl Future for Restless {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.polls.set(self.polls.get() + 1);
        let notices = Rc::clone(&self.notices);
        self.queue
            .enqueue_ui_task(Box::new(move || notices.set(notices.get() + 1)));
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

#[test]
fn a_task_that_wakes_itself_is_polled_once_per_drain() {
    let runtime = TestRuntime::new();
    let handle = runtime.handle();
    let polls = Rc::new(Cell::new(0));
    let notices = Rc::new(Cell::new(0));
    let _task = handle.spawn_ui(Restless {
        polls: Rc::clone(&polls),
        notices: Rc::clone(&notices),
        queue: handle.clone(),
    });

    handle.drain_ui();
    assert_eq!((polls.get(), notices.get()), (1, 1));
    handle.drain_ui();
    assert_eq!((polls.get(), notices.get()), (2, 2));
}

#[test]
fn ui_dispatcher_failed_send_does_not_leave_pending_work() {
    let runtime = Runtime::new(Arc::new(TestScheduler));
    let dispatcher = runtime.handle().dispatcher();

    drop(runtime);

    assert!(!dispatcher.has_pending());
    dispatcher.post(|| {});
    assert!(!dispatcher.has_pending());
    dispatcher.post_invoke(404, 12_u32);
    assert!(!dispatcher.has_pending());
}

#[test]
fn pending_guard_does_not_wrap_on_underflow() {
    let counter = AtomicUsize::new(0);

    {
        let _guard = PendingGuard::new(&counter);
    }

    assert_eq!(counter.load(Ordering::SeqCst), 0);
}

#[test]
fn pending_guard_decrements_pending_count() {
    let counter = AtomicUsize::new(2);

    {
        let _guard = PendingGuard::new(&counter);
    }

    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn schedule_frame_without_runtime_is_ignored() {
    let ok = std::thread::spawn(|| std::panic::catch_unwind(super::schedule_frame).is_ok())
        .join()
        .expect("test thread should join");

    assert!(ok);
}

#[test]
fn schedule_node_update_without_runtime_is_ignored() {
    let ok = std::thread::spawn(|| {
        std::panic::catch_unwind(|| {
            super::schedule_node_update(|_| Ok(()));
        })
        .is_ok()
    })
    .join()
    .expect("test thread should join");

    assert!(ok);
}
