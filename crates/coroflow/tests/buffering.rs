use std::{
    pin::Pin,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use coroflow::{
    CoroutineScope, Flow, FlowExt, MutableSharedFlow, SendFlow, Stream, TestScheduler, delay, flow,
};

type Log = Arc<Mutex<Vec<(u64, u32)>>>;

fn ticks(scheduler: &TestScheduler, emitted: &Log) -> impl SendFlow<Item = u32> + Clone {
    let clock = Arc::clone(scheduler.dispatcher().clock());
    let emitted = Arc::clone(emitted);
    flow(move |emitter| {
        let (clock, emitted) = (Arc::clone(&clock), Arc::clone(&emitted));
        async move {
            for value in 1..=5 {
                delay(Duration::from_millis(10)).await;
                let at = clock.now().as_millis() as u64;
                emitted
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push((at, value));
                emitter.emit(value).await;
            }
        }
    })
}

fn collect_slowly<F>(scheduler: &TestScheduler, flow: F) -> Log
where
    F: Flow + Send + 'static,
    F::Run: Send + 'static,
    F::Item: Into<u32> + Send,
{
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let clock = Arc::clone(scheduler.dispatcher().clock());
    let received: Log = Arc::default();
    let log = Arc::clone(&received);
    scope.launch(async move {
        let mut run = flow.open();
        while let Some(value) = std::future::poll_fn(|cx| Pin::new(&mut run).poll_next(cx)).await {
            let at = clock.now().as_millis() as u64;
            log.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((at, value.into()));
            delay(Duration::from_millis(100)).await;
        }
    });
    scheduler.advance_time_by(Duration::from_secs(2));
    received
}

fn values(log: &Log) -> Vec<(u64, u32)> {
    log.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

#[test]
fn without_a_buffer_the_upstream_waits_for_a_slow_collector() {
    let scheduler = TestScheduler::new();
    let emitted: Log = Arc::default();
    collect_slowly(&scheduler, ticks(&scheduler, &emitted));
    let times: Vec<u64> = values(&emitted).iter().map(|(at, _)| *at).collect();
    assert_eq!(times, vec![10, 120, 230, 340, 450]);
}

#[test]
fn buffer_lets_the_upstream_run_ahead_of_a_slow_collector() {
    let scheduler = TestScheduler::new();
    let emitted: Log = Arc::default();
    let received = collect_slowly(&scheduler, ticks(&scheduler, &emitted).buffer(8));
    let times: Vec<u64> = values(&emitted).iter().map(|(at, _)| *at).collect();
    assert_eq!(times, vec![10, 20, 30, 40, 50]);
    let got: Vec<u32> = values(&received).iter().map(|(_, value)| *value).collect();
    assert_eq!(got, vec![1, 2, 3, 4, 5], "nothing is lost");
}

#[test]
fn conflate_hands_a_slow_collector_only_the_newest_value() {
    let scheduler = TestScheduler::new();
    let emitted: Log = Arc::default();
    let received = collect_slowly(&scheduler, ticks(&scheduler, &emitted).conflate());
    let got: Vec<u32> = values(&received).iter().map(|(_, value)| *value).collect();
    assert_eq!(got, vec![1, 5]);
}

#[test]
fn a_suspending_shared_emit_waits_for_the_slowest_collector() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let events = MutableSharedFlow::new(0, 1);
    let emitted: Log = Arc::default();
    let (emitter, log) = (events.clone(), Arc::clone(&emitted));
    let clock = Arc::clone(scheduler.dispatcher().clock());
    scope.launch(async move {
        delay(Duration::from_millis(10)).await;
        for value in 1..=5_u32 {
            emitter.emit(value).await;
            let at = clock.now().as_millis() as u64;
            log.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((at, value));
        }
    });
    let received = collect_slowly(&scheduler, events.as_shared_flow());
    assert_eq!(
        values(&emitted),
        vec![(10, 1), (10, 2), (110, 3), (210, 4), (310, 5)]
    );
    let got: Vec<u32> = values(&received).iter().map(|(_, value)| *value).collect();
    assert_eq!(got, vec![1, 2, 3, 4, 5], "nothing is lost");
}
