#![cfg(not(target_arch = "wasm32"))]

use std::{
    cell::{Cell, RefCell},
    future::Future,
    num::NonZeroUsize,
    pin::Pin,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};

use cranpose_core::{
    BlockingError, BlockingExecutor, BlockingExecutorConfig, BlockingTask, Composition,
    MemoryApplier, launchBlocking, withBlocking,
};

struct Waiter(std::thread::Thread);

thread_local! {
    static THREAD_RESOURCE: RefCell<Option<mpsc::Sender<()>>> = const { RefCell::new(None) };
}

impl Wake for Waiter {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

fn finish<T>(future: impl Future<Output = T>) -> T {
    let mut future = Box::pin(future);
    let waker = Waker::from(Arc::new(Waiter(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Poll::Ready(result) = future.as_mut().poll(&mut context) {
            return result;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "blocking work stranded its caller");
        std::thread::park_timeout(remaining);
    }
}

fn executor() -> BlockingExecutor {
    BlockingExecutor::new(BlockingExecutorConfig {
        max_threads: NonZeroUsize::MIN,
        max_queued: NonZeroUsize::MIN,
        idle_timeout: Duration::from_secs(1),
    })
}

fn occupy(executor: &BlockingExecutor) -> (BlockingTask<()>, mpsc::Sender<()>) {
    let (started, entered) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let task = executor
        .try_submit(move || {
            started.send(()).expect("report start");
            released
                .recv_timeout(Duration::from_secs(5))
                .expect("release worker");
        })
        .expect("admit blocker");
    entered
        .recv_timeout(Duration::from_secs(5))
        .expect("worker starts");
    (task, release)
}

#[test]
fn a_panicking_worker_resolves_instead_of_stranding_its_caller() {
    assert_eq!(
        finish(withBlocking(|| -> () { panic!("worker failure") })),
        Err(BlockingError::Panicked)
    );
    assert_eq!(finish(withBlocking(|| 42)), Ok(42));
}

#[test]
fn saturation_rejects_new_work_without_running_it_inline() {
    let executor = executor();
    let (running, release) = occupy(&executor);
    let waiting = executor.try_submit(|| 42).expect("one waiting job");
    assert!(matches!(
        executor.try_submit(|| panic!("rejected work must not run")),
        Err(BlockingError::Saturated)
    ));
    release.send(()).expect("release worker");
    assert_eq!(finish(running), Ok(()));
    assert_eq!(finish(waiting), Ok(42));
}

#[test]
fn a_second_worker_can_finish_while_the_first_is_blocked() {
    let executor = BlockingExecutor::new(BlockingExecutorConfig {
        max_threads: NonZeroUsize::new(2).expect("two workers"),
        max_queued: NonZeroUsize::MIN,
        idle_timeout: Duration::from_secs(1),
    });
    let (running, release) = occupy(&executor);
    let independent = executor.try_submit(|| 42).expect("second job");
    assert_eq!(finish(independent), Ok(42));
    release.send(()).expect("release first worker");
    assert_eq!(finish(running), Ok(()));
}

#[test]
fn dropping_waiting_work_releases_its_captures_and_capacity_immediately() {
    let executor = executor();
    let (running, release) = occupy(&executor);
    let (capture, released_capture) = mpsc::channel::<()>();
    let ran = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&ran);
    let waiting = executor
        .try_submit(move || {
            seen.store(true, Ordering::SeqCst);
            drop(capture);
        })
        .expect("waiting work");
    drop(waiting);
    assert_eq!(
        released_capture.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    );
    let replacement = executor.try_submit(|| 7).expect("capacity released");
    release.send(()).expect("release worker");
    assert_eq!(finish(running), Ok(()));
    assert_eq!(finish(replacement), Ok(7));
    assert!(!ran.load(Ordering::SeqCst));
}

#[test]
fn shutdown_fails_waiting_work_but_allows_running_work_to_finish() {
    let executor = executor();
    let (running, release) = occupy(&executor);
    let waiting = executor.try_submit(|| 7).expect("waiting work");
    executor.shutdown();
    assert_eq!(finish(waiting), Err(BlockingError::Shutdown));
    assert!(matches!(
        executor.try_submit(|| 9),
        Err(BlockingError::Shutdown)
    ));
    release.send(()).expect("release worker");
    assert_eq!(finish(running), Ok(()));
}

#[test]
fn only_the_last_executor_owner_closes_waiting_work() {
    let executor = executor();
    let owner = executor.clone();
    drop(executor);
    let (running, release) = occupy(&owner);
    let waiting = owner.try_submit(|| 7).expect("remaining owner admits work");
    drop(owner);
    assert_eq!(finish(waiting), Err(BlockingError::Shutdown));
    release.send(()).expect("release worker");
    assert_eq!(finish(running), Ok(()));
}

#[test]
fn dropping_a_running_task_does_not_interrupt_the_closure_or_stall_the_pool() {
    let executor = executor();
    let (running, release) = occupy(&executor);
    drop(running);
    let waiting = executor.try_submit(|| 11).expect("next work admitted");
    release.send(()).expect("running closure remains alive");
    assert_eq!(finish(waiting), Ok(11));
}

#[test]
fn dropping_a_runtime_cancels_its_queued_work() {
    let executor = executor();
    let (running, release) = occupy(&executor);
    let composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let waiting = executor
        .try_submit(|| panic!("disposed work must not run"))
        .expect("waiting work");
    runtime
        .spawn_ui(async move {
            let _ = waiting.await;
        })
        .expect("runtime task");
    runtime.drain_ui();
    drop(composition);
    let replacement = executor
        .try_submit(|| 19)
        .expect("disposed task released queue capacity");
    release.send(()).expect("release worker");
    assert_eq!(finish(running), Ok(()));
    assert_eq!(finish(replacement), Ok(19));
}

#[test]
fn idle_retirement_and_new_submissions_keep_delivering_results() {
    let executor = BlockingExecutor::new(BlockingExecutorConfig {
        idle_timeout: Duration::ZERO,
        ..BlockingExecutorConfig {
            max_threads: NonZeroUsize::MIN,
            max_queued: NonZeroUsize::MIN,
            ..BlockingExecutorConfig::default()
        }
    });
    let (thread_resource, released) = mpsc::channel();
    let task = executor
        .try_submit(move || {
            THREAD_RESOURCE.with(|held| {
                held.borrow_mut().replace(thread_resource);
            });
        })
        .expect("admit resource owner");
    assert_eq!(finish(task), Ok(()));
    assert_eq!(
        released.recv_timeout(Duration::from_secs(5)),
        Err(mpsc::RecvTimeoutError::Disconnected),
        "the idle worker retained its thread-local resource"
    );
    for value in 0..100 {
        let task = executor.try_submit(move || value).expect("admit work");
        assert_eq!(finish(task), Ok(value));
    }
}

#[test]
fn cancelling_a_callback_suppresses_delivery_while_running_work_finishes() {
    let mut composition = Composition::new(MemoryApplier::new());
    let handle = Rc::new(RefCell::new(None));
    let capture = Rc::clone(&handle);
    let delivered = Rc::new(Cell::new(false));
    let seen = Rc::clone(&delivered);
    let (release, released) = mpsc::channel();
    let (started, entered) = mpsc::channel();
    let (finished, done) = mpsc::channel();
    let mut inputs = Some((started, released, finished, seen));
    composition
        .render(1, move || {
            let (started, released, finished, seen) = inputs.take().expect("initial render");
            *capture.borrow_mut() = Some(
                launchBlocking(
                    move || {
                        started.send(()).expect("worker starts");
                        released
                            .recv_timeout(Duration::from_secs(5))
                            .expect("release worker");
                        finished.send(()).expect("worker finishes");
                    },
                    move |_| seen.set(true),
                )
                .expect("admit callback"),
            );
        })
        .expect("render");
    composition.runtime_handle().drain_ui();
    entered
        .recv_timeout(Duration::from_secs(5))
        .expect("worker start");
    handle.borrow().as_ref().expect("handle").cancel();
    release.send(()).expect("release worker");
    done.recv_timeout(Duration::from_secs(5))
        .expect("running work finishes");
    composition.runtime_handle().drain_ui();
    assert!(!delivered.get());
}

#[test]
fn waiting_for_capacity_yields_and_then_completes_without_app_retries() {
    let executor = executor();
    let (running, release) = occupy(&executor);
    let queued = executor.try_submit(|| 7).expect("fill the queue");
    let mut waiting = Box::pin(executor.submit(|| 42));
    assert!(
        waiting
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    release.send(()).expect("release worker");
    assert_eq!(finish(running), Ok(()));
    assert_eq!(finish(queued), Ok(7));
    assert_eq!(finish(waiting), Ok(42));
}

struct Notification(mpsc::Sender<()>);

impl Wake for Notification {
    fn wake(self: Arc<Self>) {
        let _ = self.0.send(());
    }
}

fn watch_pending(future: Pin<&mut impl Future>) -> mpsc::Receiver<()> {
    let (signal, notified) = mpsc::channel();
    let waker = Waker::from(Arc::new(Notification(signal)));
    assert!(future.poll(&mut Context::from_waker(&waker)).is_pending());
    assert_eq!(notified.try_recv(), Err(mpsc::TryRecvError::Empty));
    notified
}

fn notified(signal: &mpsc::Receiver<()>) {
    signal
        .recv_timeout(Duration::from_secs(5))
        .expect("wake suspended caller");
}

#[test]
fn cancelling_queued_work_wakes_a_capacity_waiter() {
    let executor = executor();
    let (running, release) = occupy(&executor);
    let queued = executor.try_submit(|| 7).expect("fill the queue");
    let mut waiting = Box::pin(executor.submit(|| 42));
    let signal = watch_pending(waiting.as_mut());
    drop(queued);
    notified(&signal);
    assert!(
        waiting
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    assert!(matches!(
        executor.try_submit(|| 9),
        Err(BlockingError::Saturated)
    ));
    release.send(()).expect("release worker");
    assert_eq!(finish(running), Ok(()));
    assert_eq!(finish(waiting), Ok(42));
}

#[test]
fn dropping_a_notified_capacity_waiter_releases_captures_and_wakes_the_next() {
    let executor = executor();
    let (running, release) = occupy(&executor);
    let queued = executor.try_submit(|| 7).expect("fill the queue");
    let (capture, released_capture) = mpsc::channel::<()>();
    let mut first = Box::pin(executor.submit(move || {
        drop(capture);
        panic!("cancelled work must never start");
    }));
    let first_signal = watch_pending(first.as_mut());
    let mut second = Box::pin(executor.submit(|| 42));
    let second_signal = watch_pending(second.as_mut());
    drop(queued);
    notified(&first_signal);
    drop(first);
    assert_eq!(
        released_capture.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    );
    notified(&second_signal);
    release.send(()).expect("release worker");
    assert_eq!(finish(running), Ok(()));
    assert_eq!(finish(second), Ok(42));
}

#[test]
fn shutdown_wakes_every_caller_waiting_for_capacity() {
    let executor = executor();
    let (running, release) = occupy(&executor);
    let queued = executor.try_submit(|| 7).expect("fill the queue");
    let mut waiting: Vec<_> = (0..3)
        .map(|value| Box::pin(executor.submit(move || value)))
        .collect();
    let signals: Vec<_> = waiting
        .iter_mut()
        .map(|future| watch_pending(future.as_mut()))
        .collect();
    executor.shutdown();
    for (signal, future) in signals.iter().zip(waiting) {
        notified(signal);
        assert_eq!(finish(future), Err(BlockingError::Shutdown));
    }
    assert_eq!(finish(queued), Err(BlockingError::Shutdown));
    release.send(()).expect("release worker");
    assert_eq!(finish(running), Ok(()));
}

#[test]
fn runtime_disposal_releases_captures_of_work_waiting_for_capacity() {
    let executor = executor();
    let (running, release) = occupy(&executor);
    let queued = executor.try_submit(|| 7).expect("fill the queue");
    let (capture, released_capture) = mpsc::channel::<()>();
    let composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let owner = executor.clone();
    runtime
        .spawn_ui(async move {
            let _ = owner
                .submit(move || {
                    drop(capture);
                    panic!("disposed work must never start");
                })
                .await;
        })
        .expect("runtime task");
    runtime.drain_ui();
    assert_eq!(released_capture.try_recv(), Err(mpsc::TryRecvError::Empty));
    drop(composition);
    assert_eq!(
        released_capture.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    );
    release.send(()).expect("release worker");
    assert_eq!(finish(running), Ok(()));
    assert_eq!(finish(queued), Ok(7));
    assert_eq!(finish(executor.submit(|| 42)), Ok(42));
}

#[test]
fn taking_a_job_wakes_its_successor_before_that_job_finishes() {
    let executor = executor();
    let (running, release) = occupy(&executor);
    let (release_queued, hold_queued) = mpsc::channel();
    let queued = executor
        .try_submit(move || {
            hold_queued
                .recv_timeout(Duration::from_secs(5))
                .expect("release queued job");
        })
        .expect("fill the queue");
    let mut successor = Box::pin(executor.submit(|| 42));
    let signal = watch_pending(successor.as_mut());
    release.send(()).expect("release first worker");
    notified(&signal);
    assert!(
        successor
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    assert!(matches!(
        executor.try_submit(|| 9),
        Err(BlockingError::Saturated)
    ));
    release_queued.send(()).expect("release queued job");
    assert_eq!(finish(running), Ok(()));
    assert_eq!(finish(queued), Ok(()));
    assert_eq!(finish(successor), Ok(42));
}

#[test]
fn launch_blocking_waits_for_capacity_without_rejecting_callbacks_or_blocking_ui() {
    let mut composition = Composition::new(MemoryApplier::new());
    let callbacks = Rc::new(RefCell::new(Vec::new()));
    let observed = Rc::clone(&callbacks);
    let jobs = std::thread::available_parallelism()
        .expect("CPU count")
        .get()
        * 6
        + 1;
    let (release, released) = mpsc::channel::<()>();
    let gate = Arc::new(std::sync::Mutex::new(released));
    let ui_thread = std::thread::current().id();
    composition
        .render(1, move || {
            for value in 0..jobs {
                let gate = Arc::clone(&gate);
                let callbacks = Rc::clone(&callbacks);
                launchBlocking(
                    move || {
                        assert_ne!(std::thread::current().id(), ui_thread);
                        assert_eq!(
                            gate.lock()
                                .expect("release gate")
                                .recv_timeout(Duration::from_secs(5)),
                            Err(mpsc::RecvTimeoutError::Disconnected)
                        );
                        value
                    },
                    move |result| {
                        callbacks
                            .borrow_mut()
                            .push(result.expect("overload waits instead of failing"));
                    },
                )
                .expect("runtime accepts callback");
            }
        })
        .expect("render");
    let runtime = composition.runtime_handle();
    runtime.drain_ui();
    assert!(observed.borrow().is_empty());
    drop(release);
    finish(std::future::poll_fn(|context| {
        runtime.drain_ui();
        if observed.borrow().len() == jobs {
            Poll::Ready(())
        } else {
            context.waker().wake_by_ref();
            Poll::Pending
        }
    }));
    observed.borrow_mut().sort_unstable();
    assert_eq!(*observed.borrow(), (0..jobs).collect::<Vec<_>>());
}
