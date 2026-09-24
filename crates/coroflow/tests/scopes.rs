use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use coroflow::{
    CoroutineScope, Dispatchers, JobOutcome, MainScope, TaskFailed, TestScheduler, delay,
    with_context,
};

struct DropMarker(Arc<AtomicUsize>);

impl Drop for DropMarker {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn a_launched_job_reports_completion_to_joiners() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let job = scope.launch(async {
        delay(Duration::from_millis(10)).await;
    });
    assert!(job.is_active());
    assert_eq!(job.outcome(), None);
    assert_eq!(scheduler.block_on(job.join()), Ok(JobOutcome::Completed));
    assert!(!job.is_active());
    assert!(!job.is_cancelled());
    assert_eq!(scheduler.now(), Duration::from_millis(10));
}

#[test]
fn cancelling_a_job_drops_its_future_without_polling_it_again() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let dropped = Arc::new(AtomicUsize::new(0));
    let resumed = Arc::new(AtomicUsize::new(0));
    let (marker, after) = (Arc::clone(&dropped), Arc::clone(&resumed));
    let job = scope.launch(async move {
        let _guard = DropMarker(marker);
        delay(Duration::from_millis(100)).await;
        after.fetch_add(1, Ordering::SeqCst);
    });
    scheduler.advance_time_by(Duration::from_millis(10));
    job.cancel();
    assert!(job.is_cancelled());
    scheduler.advance_time_by(Duration::from_millis(200));
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(resumed.load(Ordering::SeqCst), 0);
    assert_eq!(job.outcome(), Some(JobOutcome::Cancelled));
}

#[test]
fn dropping_a_scope_cancels_everything_it_launched_and_refuses_new_work() {
    let scheduler = TestScheduler::new();
    let dropped = Arc::new(AtomicUsize::new(0));
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let handle = scope.handle();
    for _ in 0..3 {
        let marker = Arc::clone(&dropped);
        scope.launch(async move {
            let _guard = DropMarker(marker);
            std::future::pending::<()>().await;
        });
    }
    scheduler.run_current();
    assert!(scope.is_active());
    drop(scope);
    scheduler.run_current();
    assert_eq!(dropped.load(Ordering::SeqCst), 3);
    let late = handle.launch(async {});
    assert_eq!(late.outcome(), Some(JobOutcome::Cancelled));
}

#[test]
fn a_cancelled_scope_rejects_launches() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    scope.cancel();
    assert!(!scope.is_active());
    assert_eq!(
        scope.launch(async {}).outcome(),
        Some(JobOutcome::Cancelled)
    );
}

#[test]
fn a_scope_handle_launches_into_its_live_scope() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let ran = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&ran);
    let job = scope.handle().launch(async move {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    scheduler.run_current();
    assert_eq!(job.outcome(), Some(JobOutcome::Completed));
    assert_eq!(ran.load(Ordering::SeqCst), 1);
    assert!(scope.dispatcher().clock().now() == scheduler.now());
}

#[test]
fn a_main_scope_runs_futures_that_hold_thread_bound_state() {
    let scheduler = TestScheduler::new();
    let scope = MainScope::new(scheduler.main_dispatcher());
    let counter = Rc::new(Cell::new(0));
    let shared = Rc::clone(&counter);
    let job = scope.launch(async move {
        delay(Duration::from_millis(5)).await;
        shared.set(shared.get() + 1);
    });
    scheduler.advance_time_by(Duration::from_millis(5));
    assert_eq!(counter.get(), 1);
    assert_eq!(job.outcome(), Some(JobOutcome::Completed));
    assert!(scope.is_active());
    assert!(scope.dispatcher().dispatcher().clock().now() == Duration::from_millis(5));
    scope.cancel();
    assert!(!scope.is_active());
}

#[test]
fn dropping_a_main_scope_cancels_its_coroutines() {
    let scheduler = TestScheduler::new();
    let dropped = Arc::new(AtomicUsize::new(0));
    let scope = MainScope::new(scheduler.main_dispatcher());
    let marker = Arc::clone(&dropped);
    let job = scope.launch(async move {
        let _guard = DropMarker(marker);
        std::future::pending::<()>().await;
    });
    scheduler.run_current();
    drop(scope);
    scheduler.run_current();
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(job.outcome(), Some(JobOutcome::Cancelled));
}

#[test]
fn a_panicking_coroutine_ends_as_panicked_without_taking_the_scheduler_down() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let job = scope.launch(async {
        panic!("boom");
    });
    scheduler.run_current();
    assert_eq!(job.outcome(), Some(JobOutcome::Panicked));
}

#[test]
fn with_context_runs_work_elsewhere_and_returns_its_result() {
    let scheduler = TestScheduler::new();
    let io = scheduler.dispatcher();
    let result = scheduler.block_on(async move {
        with_context(&io, async {
            delay(Duration::from_millis(30)).await;
            21 * 2
        })
        .await
    });
    assert_eq!(result, Ok(Ok(42)));
    assert_eq!(scheduler.now(), Duration::from_millis(30));
}

#[test]
fn with_context_reports_a_panic_as_task_failed() {
    let scheduler = TestScheduler::new();
    let io = scheduler.dispatcher();
    let result = scheduler.block_on(async move {
        with_context(&io, async {
            panic!("broken");
        })
        .await
    });
    assert_eq!(result, Ok(Err::<(), _>(TaskFailed)));
}

#[test]
fn with_context_on_the_real_io_pool_crosses_threads() {
    let caller = std::thread::current().id();
    let worker = pollster::block_on(with_context(&Dispatchers::io(), async {
        std::thread::current().id()
    }));
    assert!(matches!(worker, Ok(id) if id != caller));
}

#[test]
fn delay_on_the_system_clock_waits_real_time() {
    let started = std::time::Instant::now();
    let scope = CoroutineScope::new(Dispatchers::default_pool());
    let job = scope.launch(async {
        delay(Duration::from_millis(20)).await;
    });
    assert_eq!(pollster::block_on(job.join()), JobOutcome::Completed);
    assert!(started.elapsed() >= Duration::from_millis(15));
}

#[test]
fn a_scheduler_with_nothing_to_do_reports_a_stall() {
    let scheduler = TestScheduler::default();
    assert_eq!(
        scheduler.block_on(std::future::pending::<()>()),
        Err(coroflow::Stalled)
    );
}
