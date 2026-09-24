use std::{
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use coroflow::{
    CoroutineScope, Dispatchers, JobOutcome, TaskFailed, TestScheduler, await_all, delay, join_all,
    yield_now,
};

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[test]
fn invoke_on_completion_runs_when_the_job_ends_or_at_once_if_it_has() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let job = scope.launch(async { delay(Duration::from_millis(10)).await });
    let log = Arc::clone(&seen);
    job.invoke_on_completion(move |outcome| locked(&log).push(("early", outcome)));
    scheduler.advance_time_by(Duration::from_millis(10));
    let log = Arc::clone(&seen);
    job.invoke_on_completion(move |outcome| locked(&log).push(("late", outcome)));
    assert_eq!(
        *locked(&seen),
        vec![
            ("early", JobOutcome::Completed),
            ("late", JobOutcome::Completed)
        ]
    );
}

#[test]
fn cancel_and_join_waits_for_the_cancelled_coroutine() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let job = scope.launch(async { delay(Duration::from_secs(60)).await });
    scheduler.run_current();
    assert_eq!(
        scheduler.block_on(job.cancel_and_join()),
        Ok(JobOutcome::Cancelled)
    );
}

#[test]
fn join_all_waits_for_every_job_and_await_all_fails_fast() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let jobs: Vec<_> = [30, 10, 20]
        .into_iter()
        .map(|wait| scope.launch(async move { delay(Duration::from_millis(wait)).await }))
        .collect();
    assert_eq!(scheduler.block_on(join_all(jobs)), Ok(()));
    assert_eq!(scheduler.now(), Duration::from_millis(30));
    let values: Vec<_> = [3, 1, 2]
        .into_iter()
        .map(|value| {
            scope.async_(async move {
                delay(Duration::from_millis(value * 10)).await;
                value
            })
        })
        .collect();
    assert_eq!(scheduler.block_on(await_all(values)), Ok(Ok(vec![3, 1, 2])));
    let started = scheduler.now();
    let doomed = vec![
        scope.async_(async {
            delay(Duration::from_millis(1_000)).await;
            1
        }),
        scope.async_(async {
            delay(Duration::from_millis(10)).await;
            panic!("failed")
        }),
    ];
    assert_eq!(scheduler.block_on(await_all(doomed)), Ok(Err(TaskFailed)));
    assert_eq!(
        scheduler.now() - started,
        Duration::from_millis(10),
        "did not wait for the slow one"
    );
}

#[test]
fn a_lazy_coroutine_runs_only_once_started_joined_or_awaited() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let ran = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&ran);
    let job = scope.launch_lazy(async move {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    scheduler.run_current();
    assert_eq!(ran.load(Ordering::SeqCst), 0);
    assert!(job.is_active());
    assert!(job.start());
    assert!(!job.start(), "only the first start starts it");
    scheduler.run_current();
    assert_eq!(ran.load(Ordering::SeqCst), 1);

    let joined = scope.launch_lazy(async {});
    assert_eq!(scheduler.block_on(joined.join()), Ok(JobOutcome::Completed));
    let answer = scope.async_lazy(async { 42 });
    scheduler.run_current();
    assert_eq!(answer.outcome(), None, "not started yet");
    assert_eq!(scheduler.block_on(answer), Ok(Ok(42)));

    let never = Arc::clone(&ran);
    let cancelled = scope.launch_lazy(async move {
        never.fetch_add(1, Ordering::SeqCst);
    });
    cancelled.cancel();
    scheduler.run_current();
    assert_eq!(cancelled.outcome(), Some(JobOutcome::Cancelled));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        1,
        "a cancelled lazy job never runs"
    );
}

#[test]
fn children_lists_the_coroutines_still_running() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let short = scope.launch(async { delay(Duration::from_millis(10)).await });
    let long = scope.launch(async { delay(Duration::from_millis(100)).await });
    assert_eq!(scope.children().len(), 2);
    scheduler.advance_time_by(Duration::from_millis(10));
    assert_eq!(short.outcome(), Some(JobOutcome::Completed));
    let children = scope.children();
    assert_eq!(children.len(), 1);
    assert!(children.iter().all(coroflow::Job::is_active));
    assert!(long.is_active());
}

#[test]
fn the_exception_handler_sees_launch_failures_but_not_async_ones() {
    let scheduler = TestScheduler::new();
    let failures = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&failures);
    let scope = CoroutineScope::new(scheduler.dispatcher())
        .with_exception_handler(move |message| locked(&log).push(message.to_owned()));
    scope.launch(async { panic!("launch broke") });
    let deferred = scope.async_(async {
        panic!("async broke");
    });
    scheduler.run_current();
    assert_eq!(*locked(&failures), vec!["launch broke".to_owned()]);
    assert_eq!(scheduler.block_on(deferred), Ok(Err::<(), _>(TaskFailed)));
}

#[test]
fn limited_parallelism_caps_how_many_steps_run_at_once() {
    let limited = Dispatchers::io().limited_parallelism(2);
    let scope = CoroutineScope::new(limited);
    let running = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let jobs: Vec<_> = (0..8)
        .map(|_| {
            let (running, peak) = (Arc::clone(&running), Arc::clone(&peak));
            scope.launch(async move {
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(20));
                running.fetch_sub(1, Ordering::SeqCst);
            })
        })
        .collect();
    pollster::block_on(join_all(jobs));
    let peak = peak.load(Ordering::SeqCst);
    assert!((1..=2).contains(&peak), "{peak} steps ran at once");
}

#[test]
fn limited_parallelism_of_one_runs_steps_in_dispatch_order() {
    let scope = CoroutineScope::new(Dispatchers::default_pool().limited_parallelism(1));
    let order = Arc::new(Mutex::new(Vec::new()));
    let jobs: Vec<_> = (0..20)
        .map(|index| {
            let order = Arc::clone(&order);
            scope.launch(async move { locked(&order).push(index) })
        })
        .collect();
    pollster::block_on(join_all(jobs));
    assert_eq!(*locked(&order), (0..20).collect::<Vec<_>>());
}

#[test]
fn single_thread_runs_every_step_on_its_own_named_thread() {
    let scope = CoroutineScope::new(Dispatchers::single_thread("coroflow-test-single"));
    let threads = Arc::new(Mutex::new(Vec::new()));
    let jobs: Vec<_> = (0..4)
        .map(|_| {
            let threads = Arc::clone(&threads);
            scope.launch(async move {
                for _ in 0..3 {
                    let current = thread::current();
                    locked(&threads).push((current.id(), current.name().map(str::to_owned)));
                    yield_now().await;
                }
            })
        })
        .collect();
    pollster::block_on(join_all(jobs));
    let threads = locked(&threads);
    assert_eq!(threads.len(), 12);
    assert!(threads.iter().all(|seen| *seen == threads[0]));
    assert_eq!(threads[0].1.as_deref(), Some("coroflow-test-single-0"));
}

#[test]
fn unconfined_runs_a_launched_body_until_its_first_suspension_before_returning() {
    let scope = CoroutineScope::new(Dispatchers::unconfined());
    let reached = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&reached);
    let order = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&order);
    let job = scope.launch(async move {
        flag.store(true, Ordering::SeqCst);
        locked(&log).push("before yield");
        yield_now().await;
        locked(&log).push("after yield");
    });
    assert!(reached.load(Ordering::SeqCst), "ran inside launch");
    assert_eq!(job.outcome(), Some(JobOutcome::Completed));
    assert_eq!(*locked(&order), vec!["before yield", "after yield"]);
}
