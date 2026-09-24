use std::{
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use coroflow::{Flow, FlowExt, TestScheduler, delay, flow_of};

use crate::support::{DropMarker, endless, timed};

fn timeline<F: Flow>(scheduler: &TestScheduler, flow: &F) -> Vec<(u64, F::Item)> {
    let mut seen = Vec::new();
    let finished = scheduler.block_on(flow.collect(|value| {
        seen.push((scheduler.now().as_millis() as u64, value));
    }));
    assert!(finished.is_ok(), "the flow never completed");
    seen
}

async fn slowly<T>(value: T) -> T {
    delay(Duration::from_millis(100)).await;
    value
}

#[test]
fn map_async_transforms_one_value_at_a_time_in_order() {
    let scheduler = TestScheduler::new();
    let offset = Arc::new(1_000);
    let mapped = flow_of(vec![1, 2, 3]).map_async(async move |value| slowly(value + *offset).await);
    assert_eq!(
        timeline(&scheduler, &mapped),
        vec![(100, 1_001), (200, 1_002), (300, 1_003)]
    );
}

#[test]
fn map_latest_cancels_the_transformation_a_newer_value_supersedes() {
    let scheduler = TestScheduler::new();
    let cancelled = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&cancelled);
    let mapped = timed(vec![(0, 1), (50, 2), (150, 3)]).map_latest(async move |value| {
        let marker = DropMarker(Arc::clone(&counter));
        let result = slowly(value * 10).await;
        std::mem::forget(marker);
        result
    });
    assert_eq!(timeline(&scheduler, &mapped), vec![(150, 20), (300, 30)]);
    assert_eq!(
        cancelled.load(Ordering::SeqCst),
        1,
        "only the first one was cancelled"
    );
}

#[test]
fn filter_async_and_filter_map_async_keep_what_their_suspending_lambda_accepts() {
    let scheduler = TestScheduler::new();
    let evens = flow_of(vec![1, 2, 3, 4]).filter_async(async |value| slowly(value % 2 == 0).await);
    assert_eq!(timeline(&scheduler, &evens), vec![(200, 2), (400, 4)]);
    let halves = flow_of(vec![1, 2, 3, 4])
        .filter_map_async(async |value| slowly((value % 2 == 0).then_some(value / 2)).await);
    assert_eq!(scheduler.block_on(halves.to_vec()), Ok(vec![1, 2]));
}

#[test]
fn on_each_async_finishes_its_action_before_emitting() {
    let scheduler = TestScheduler::new();
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&log);
    let inspected = flow_of(vec!["a", "b"]).on_each_async(async move |value| {
        slowly(()).await;
        sink.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(value);
    });
    assert_eq!(
        timeline(&scheduler, &inspected),
        vec![(100, "a"), (200, "b")]
    );
    assert_eq!(
        *log.lock().unwrap_or_else(PoisonError::into_inner),
        vec!["a", "b"]
    );
}

#[test]
fn transform_may_emit_any_number_of_values_and_suspend_between_them() {
    let scheduler = TestScheduler::new();
    let expanded = flow_of(vec![1, 2]).transform(async |value, emitter| {
        emitter.emit(value).await;
        slowly(()).await;
        emitter.emit(value * 10).await;
    });
    assert_eq!(
        timeline(&scheduler, &expanded),
        vec![(0, 1), (100, 10), (100, 2), (200, 20)]
    );
    let nothing =
        flow_of(vec![1, 2]).transform(async |_value, _emitter: coroflow::Emitter<u32>| {});
    assert_eq!(scheduler.block_on(nothing.to_vec()), Ok(vec![]));
}

#[test]
fn transform_while_completes_and_cancels_the_upstream_once_it_returns_false() {
    let scheduler = TestScheduler::new();
    let dropped = Arc::new(AtomicUsize::new(0));
    let endless = endless(&dropped);
    let bounded = endless.transform_while(async |value: u32, emitter| {
        emitter.emit(value).await;
        value < 3
    });
    assert_eq!(scheduler.block_on(bounded.to_vec()), Ok(vec![1, 2, 3]));
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        1,
        "the upstream was cancelled"
    );
}

#[test]
fn transform_latest_keeps_what_a_cancelled_block_already_emitted() {
    let scheduler = TestScheduler::new();
    let expanded = timed(vec![(0, 1), (50, 2)]).transform_latest(async |value, emitter| {
        emitter.emit(value).await;
        slowly(()).await;
        emitter.emit(value * 10).await;
    });
    assert_eq!(
        timeline(&scheduler, &expanded),
        vec![(0, 1), (50, 2), (150, 20)]
    );
}

#[test]
fn collect_async_awaits_each_action_before_taking_the_next_value() {
    let scheduler = TestScheduler::new();
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&log);
    let collected = scheduler.block_on(flow_of(vec![1, 2, 3]).collect_async(async move |value| {
        slowly(()).await;
        sink.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(value);
    }));
    assert_eq!(collected, Ok(()));
    assert_eq!(scheduler.now(), Duration::from_millis(300));
    assert_eq!(
        *log.lock().unwrap_or_else(PoisonError::into_inner),
        vec![1, 2, 3]
    );
}

#[test]
fn collect_latest_only_finishes_the_action_for_the_newest_value() {
    let scheduler = TestScheduler::new();
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&log);
    let collected = scheduler.block_on(timed(vec![(0, 1), (50, 2), (150, 3)]).collect_latest(
        async move |value| {
            slowly(()).await;
            sink.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(value);
        },
    ));
    assert_eq!(collected, Ok(()));
    assert_eq!(
        *log.lock().unwrap_or_else(PoisonError::into_inner),
        vec![2, 3]
    );
    assert_eq!(scheduler.now(), Duration::from_millis(300));
}

#[test]
fn take_while_and_skip_while_split_the_flow_at_the_first_failing_value() {
    let numbers = flow_of(vec![1, 2, 5, 1, 7]);
    assert_eq!(
        pollster::block_on(numbers.clone().take_while(|value| *value < 3).to_vec()),
        vec![1, 2]
    );
    assert_eq!(
        pollster::block_on(numbers.skip_while(|value| *value < 3).to_vec()),
        vec![5, 1, 7]
    );
}
