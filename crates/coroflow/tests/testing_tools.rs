use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

mod support;

use coroflow::{
    Capacity, CoroutineScope, JobOutcome, Stalled, TestScheduler, channel, delay, flow_of, run_test,
};
use support::{DropMarker, endless, timed};

#[test]
fn advance_until_idle_runs_every_timer_in_order() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let jobs: Vec<_> = [10, 2_000, 500]
        .into_iter()
        .map(|wait| scope.launch(async move { delay(Duration::from_millis(wait)).await }))
        .collect();
    scheduler.advance_until_idle();
    assert!(
        jobs.iter()
            .all(|job| job.outcome() == Some(JobOutcome::Completed))
    );
    assert_eq!(scheduler.now(), Duration::from_millis(2_000));
}

#[test]
fn run_test_waits_for_its_coroutines_and_cancels_the_background_scope() {
    let finished = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicUsize::new(0));
    let (child, background) = (Arc::clone(&finished), Arc::clone(&cancelled));
    let result = run_test(async |test| {
        test.launch(async move {
            delay(Duration::from_secs(5)).await;
            child.store(true, Ordering::SeqCst);
        });
        test.background_scope().launch(async move {
            let _marker = DropMarker(background);
            loop {
                delay(Duration::from_secs(1)).await;
            }
        });
        delay(Duration::from_secs(1)).await;
        test.scheduler().now()
    });
    assert_eq!(result, Ok(Duration::from_secs(1)));
    assert!(finished.load(Ordering::SeqCst), "the child finished first");
    assert_eq!(
        cancelled.load(Ordering::SeqCst),
        1,
        "the background work was cancelled"
    );
}

#[test]
fn run_test_reports_a_body_that_can_never_finish() {
    let result = run_test(async |_test| {
        let (_sender, receiver) = channel::<u32>(Capacity::Rendezvous);
        receiver.recv().await
    });
    assert_eq!(result, Err(Stalled));
}

#[test]
fn a_scheduler_turbine_awaits_items_across_virtual_time() {
    let scheduler = TestScheduler::new();
    let numbers = timed(vec![(100, 1), (50, 2), (10, 3)]);
    let mut run = scheduler.turbine(&numbers);
    run.expect_no_events();
    assert_eq!(run.await_item(), 1);
    assert_eq!(scheduler.now(), Duration::from_millis(100));
    run.skip_items(1);
    assert_eq!(run.await_item(), 3);
    run.await_complete();
    assert_eq!(scheduler.now(), Duration::from_millis(160));
}

#[test]
fn cancel_and_ignore_remaining_events_stops_the_collection() {
    let scheduler = TestScheduler::new();
    let dropped = Arc::new(AtomicUsize::new(0));
    let numbers = endless(&dropped);
    let mut run = scheduler.turbine(&numbers);
    assert_eq!(run.await_item(), 1);
    run.cancel_and_ignore_remaining_events();
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[test]
#[should_panic(expected = "expected an item, but the flow completed")]
fn await_item_fails_when_the_flow_completes() {
    let scheduler = TestScheduler::new();
    let empty = flow_of(Vec::<u32>::new());
    scheduler.turbine(&empty).await_item();
}

#[test]
#[should_panic(expected = "expected no events, but the flow emitted")]
fn expect_no_events_fails_when_a_value_is_ready() {
    let scheduler = TestScheduler::new();
    let ready = flow_of(vec![1]);
    scheduler.turbine(&ready).expect_no_events();
}

#[test]
#[should_panic(expected = "expected the flow to complete, but it emitted")]
fn await_complete_fails_when_the_flow_emits() {
    let scheduler = TestScheduler::new();
    let one = flow_of(vec![1]);
    scheduler.turbine(&one).await_complete();
}
