use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use coroflow::{
    ChildFailed, CoroutineScope, Either, JobOutcome, TestScheduler, TimedOut, coroutine_scope,
    delay, select, select_all, supervisor_scope, with_timeout, yield_now,
};

struct DropMarker(Arc<AtomicUsize>);

impl Drop for DropMarker {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn coroutine_scope_returns_after_the_block_and_all_children_finish() {
    let scheduler = TestScheduler::new();
    let finished = Arc::new(AtomicUsize::new(0));
    let (slow, fast) = (Arc::clone(&finished), Arc::clone(&finished));
    let result = scheduler.block_on(coroutine_scope(move |scope| async move {
        scope.launch(async move {
            delay(Duration::from_millis(100)).await;
            slow.fetch_add(1, Ordering::SeqCst);
        });
        scope.launch(async move {
            delay(Duration::from_millis(50)).await;
            fast.fetch_add(1, Ordering::SeqCst);
        });
        "block done"
    }));
    assert_eq!(result, Ok(Ok("block done")));
    assert_eq!(finished.load(Ordering::SeqCst), 2);
    assert_eq!(scheduler.now(), Duration::from_millis(100));
}

#[test]
fn a_failing_child_cancels_its_siblings_and_the_block() {
    let scheduler = TestScheduler::new();
    let dropped = Arc::new(AtomicUsize::new(0));
    let (sibling, body) = (Arc::clone(&dropped), Arc::clone(&dropped));
    let result = scheduler.block_on(coroutine_scope(move |scope| async move {
        scope.launch(async {
            delay(Duration::from_millis(10)).await;
            panic!("child failed");
        });
        scope.launch(async move {
            let _marker = DropMarker(sibling);
            delay(Duration::from_millis(1_000)).await;
        });
        let _marker = DropMarker(body);
        delay(Duration::from_millis(500)).await;
        "unreachable"
    }));
    assert_eq!(result, Ok(Err(ChildFailed)));
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        2,
        "sibling and block were dropped"
    );
    assert_eq!(scheduler.now(), Duration::from_millis(10));
}

#[test]
fn supervisor_scope_keeps_siblings_running_after_a_failure() {
    let scheduler = TestScheduler::new();
    let finished = Arc::new(AtomicUsize::new(0));
    let survivor = Arc::clone(&finished);
    let result = scheduler.block_on(supervisor_scope(move |scope| async move {
        scope.launch(async {
            delay(Duration::from_millis(10)).await;
            panic!("child failed");
        });
        scope.launch(async move {
            delay(Duration::from_millis(100)).await;
            survivor.fetch_add(1, Ordering::SeqCst);
        });
        7
    }));
    assert_eq!(result, Ok(7));
    assert_eq!(finished.load(Ordering::SeqCst), 1);
    assert_eq!(scheduler.now(), Duration::from_millis(100));
}

#[test]
fn dropping_a_coroutine_scope_cancels_its_children() {
    let scheduler = TestScheduler::new();
    let dropped = Arc::new(AtomicUsize::new(0));
    let child = Arc::clone(&dropped);
    let result = scheduler.block_on(with_timeout(
        Duration::from_millis(50),
        coroutine_scope(move |scope| async move {
            scope.launch(async move {
                let _marker = DropMarker(child);
                delay(Duration::from_millis(1_000)).await;
            });
        }),
    ));
    scheduler.run_current();
    assert_eq!(result, Ok(Err(TimedOut)));
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn a_top_level_scope_is_a_supervisor() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let failing = scope.launch(async { panic!("child failed") });
    let sibling = scope.launch(async { delay(Duration::from_millis(10)).await });
    scheduler.advance_time_by(Duration::from_millis(10));
    assert_eq!(failing.outcome(), Some(JobOutcome::Panicked));
    assert_eq!(sibling.outcome(), Some(JobOutcome::Completed));
    assert!(scope.is_active());
}

#[test]
fn select_returns_the_first_result_and_prefers_the_first_future() {
    let scheduler = TestScheduler::new();
    let raced = scheduler.block_on(select(delay(Duration::from_millis(20)), async {
        delay(Duration::from_millis(10)).await;
        "second"
    }));
    assert_eq!(raced, Ok(Either::Right("second")));
    let both_ready = pollster::block_on(select(async { 1 }, async { 2 }));
    assert_eq!(both_ready, Either::Left(1));
    let many = scheduler.block_on(select_all(vec![
        delay(Duration::from_millis(30)),
        delay(Duration::from_millis(5)),
        delay(Duration::from_millis(10)),
    ]));
    assert_eq!(many, Ok((1, ())));
}

#[test]
fn yield_now_lets_other_coroutines_run_in_between() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let order = Arc::new(Mutex::new(Vec::new()));
    for name in ["a", "b"] {
        let order = Arc::clone(&order);
        scope.launch(async move {
            for step in 0..2 {
                order
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(format!("{name}{step}"));
                yield_now().await;
            }
        });
    }
    scheduler.run_current();
    let order = order
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(order, vec!["a0", "b0", "a1", "b1"]);
}
