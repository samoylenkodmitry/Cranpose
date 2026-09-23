use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use coroflow::{CoroutineScope, Flow, FlowExt, Stream, TestScheduler, delay, flow, flow_of};

#[test]
fn a_flow_block_emits_in_order_and_reruns_for_every_collection() {
    let runs = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&runs);
    let numbers = flow(move |emitter| {
        counter.fetch_add(1, Ordering::SeqCst);
        async move {
            for value in 1..=3 {
                emitter.emit(value).await;
            }
        }
    });
    assert_eq!(pollster::block_on(numbers.to_vec()), vec![1, 2, 3]);
    assert_eq!(pollster::block_on(numbers.to_vec()), vec![1, 2, 3]);
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}

#[test]
fn map_filter_and_on_each_transform_a_cold_flow() {
    let seen = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&seen);
    let result = pollster::block_on(
        flow_of(vec![1, 2, 3, 4, 5, 6])
            .on_each(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
            })
            .filter(|value| value % 2 == 0)
            .map(|value| value * 10)
            .to_vec(),
    );
    assert_eq!(result, vec![20, 40, 60]);
    assert_eq!(seen.load(Ordering::SeqCst), 6);
}

#[test]
fn take_stops_after_the_requested_count_and_drops_the_upstream() {
    let finished = Arc::new(AtomicUsize::new(0));
    let marker = Arc::clone(&finished);
    let endless = move || {
        let marker = Arc::clone(&marker);
        flow(move |emitter| {
            let marker = Arc::clone(&marker);
            async move {
                let _guard = DropMarker(marker);
                let mut value = 0;
                loop {
                    emitter.emit(value).await;
                    value += 1;
                }
            }
        })
    };
    let mut run = endless().take(3).open();
    let collected = pollster::block_on(async {
        let mut values = Vec::new();
        while let Some(value) =
            std::future::poll_fn(|cx| std::pin::Pin::new(&mut run).poll_next(cx)).await
        {
            values.push(value);
        }
        values
    });
    assert_eq!(collected, vec![0, 1, 2]);
    assert_eq!(
        finished.load(Ordering::SeqCst),
        1,
        "take must drop the upstream run"
    );
    assert_eq!(
        pollster::block_on(endless().take(0).to_vec()),
        Vec::<i32>::new()
    );
}

#[test]
fn distinct_until_changed_drops_consecutive_duplicates_only() {
    let result = pollster::block_on(
        flow_of(vec![1, 1, 2, 2, 1, 3, 3])
            .distinct_until_changed()
            .to_vec(),
    );
    assert_eq!(result, vec![1, 2, 1, 3]);
}

#[test]
fn first_and_collect_run_the_flow_as_terminal_operations() {
    assert_eq!(pollster::block_on(flow_of(vec![7, 8]).first()), Some(7));
    assert_eq!(pollster::block_on(flow_of(Vec::<i32>::new()).first()), None);
    let mut sum = 0;
    pollster::block_on(flow_of(vec![1, 2, 3]).collect(|value| sum += value));
    assert_eq!(sum, 6);
}

#[test]
fn debounce_emits_only_values_that_stay_quiet_for_the_timeout_on_virtual_time() {
    let scheduler = TestScheduler::new();
    let typed = flow(|emitter| async move {
        emitter.emit("r").await;
        delay(Duration::from_millis(100)).await;
        emitter.emit("ru").await;
        delay(Duration::from_millis(300)).await;
        emitter.emit("rust").await;
    });
    let result = scheduler.block_on(typed.debounce(Duration::from_millis(200)).to_vec());
    assert_eq!(result, Ok(vec!["ru", "rust"]));
    assert_eq!(scheduler.now(), Duration::from_millis(400));
}

#[test]
fn flat_map_latest_cancels_the_previous_inner_flow() {
    let scheduler = TestScheduler::new();
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let queries = flow(|emitter| async move {
        emitter.emit(1).await;
        delay(Duration::from_millis(50)).await;
        emitter.emit(2).await;
    });
    let (started_in, finished_in) = (Arc::clone(&started), Arc::clone(&finished));
    let results = queries.flat_map_latest(move |query| {
        let (started, finished) = (Arc::clone(&started_in), Arc::clone(&finished_in));
        flow(move |emitter| {
            let (started, finished) = (Arc::clone(&started), Arc::clone(&finished));
            async move {
                started.fetch_add(1, Ordering::SeqCst);
                delay(Duration::from_millis(100)).await;
                emitter.emit(query * 10).await;
                finished.fetch_add(1, Ordering::SeqCst);
            }
        })
    });
    assert_eq!(scheduler.block_on(results.to_vec()), Ok(vec![20]));
    assert_eq!(started.load(Ordering::SeqCst), 2);
    assert_eq!(finished.load(Ordering::SeqCst), 1);
    assert_eq!(scheduler.now(), Duration::from_millis(150));
}

#[test]
fn combine_waits_for_both_sides_and_then_emits_on_every_change() {
    let scheduler = TestScheduler::new();
    let letters = flow(|emitter| async move {
        emitter.emit('a').await;
        delay(Duration::from_millis(20)).await;
        emitter.emit('b').await;
    });
    let numbers = flow(|emitter| async move {
        delay(Duration::from_millis(10)).await;
        emitter.emit(1).await;
        delay(Duration::from_millis(20)).await;
        emitter.emit(2).await;
    });
    let combined = letters.combine(numbers, |letter, number| format!("{letter}{number}"));
    assert_eq!(
        scheduler.block_on(combined.to_vec()),
        Ok(vec!["a1".to_string(), "b1".to_string(), "b2".to_string()])
    );
}

#[test]
fn combine_completes_without_emitting_when_one_side_is_empty() {
    let combined = flow_of(vec![1, 2]).combine(flow_of(Vec::<i32>::new()), |a, b| a + b);
    assert_eq!(pollster::block_on(combined.to_vec()), Vec::<i32>::new());
}

#[test]
fn flow_on_runs_the_upstream_on_the_given_dispatcher() {
    let upstream_thread = Arc::new(std::sync::Mutex::new(None));
    let record = Arc::clone(&upstream_thread);
    let numbers = flow_of(vec![1, 2, 3])
        .on_each(move |_| {
            *record
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some(std::thread::current().id());
        })
        .flow_on(coroflow::Dispatchers::default_pool());
    let collected = pollster::block_on(numbers.to_vec());
    assert_eq!(collected, vec![1, 2, 3]);
    let recorded = *upstream_thread
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_ne!(recorded, Some(std::thread::current().id()));
    assert!(recorded.is_some());
}

#[test]
fn dropping_a_flow_on_run_cancels_its_upstream_coroutine() {
    let scheduler = TestScheduler::new();
    let finished = Arc::new(AtomicUsize::new(0));
    let marker = Arc::clone(&finished);
    let ticks = flow(move |emitter| {
        let marker = Arc::clone(&marker);
        async move {
            let _guard = DropMarker(marker);
            loop {
                delay(Duration::from_millis(10)).await;
                emitter.emit(()).await;
            }
        }
    })
    .flow_on(scheduler.dispatcher());
    let run = ticks.open();
    scheduler.advance_time_by(Duration::from_millis(35));
    drop(run);
    scheduler.run_current();
    assert_eq!(finished.load(Ordering::SeqCst), 1);
}

#[test]
fn boxed_flows_erase_types_and_stay_re_runnable() {
    let boxed = flow_of(vec![1, 2]).map(|value| value + 1).boxed();
    let clone = boxed.clone();
    assert_eq!(pollster::block_on(boxed.to_vec()), vec![2, 3]);
    assert_eq!(pollster::block_on(clone.to_vec()), vec![2, 3]);
    let local = std::rc::Rc::new(5);
    let local_boxed = flow_of(vec![1])
        .map(move |value| value + *local)
        .boxed_local();
    let shared = local_boxed.clone();
    assert_eq!(pollster::block_on(local_boxed.to_vec()), vec![6]);
    assert_eq!(pollster::block_on(shared.to_vec()), vec![6]);
}

#[test]
fn a_coroutine_scope_collects_a_flow_launched_into_it() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let total = Arc::new(AtomicUsize::new(0));
    let sink = Arc::clone(&total);
    let ticks = flow(|emitter| async move {
        for value in 1..=3 {
            delay(Duration::from_millis(10)).await;
            emitter.emit(value).await;
        }
    });
    scope.launch(async move {
        ticks
            .collect(|value| {
                sink.fetch_add(value, Ordering::SeqCst);
            })
            .await;
    });
    scheduler.advance_time_by(Duration::from_millis(25));
    assert_eq!(total.load(Ordering::SeqCst), 3);
    scheduler.advance_time_by(Duration::from_millis(10));
    assert_eq!(total.load(Ordering::SeqCst), 6);
}

struct DropMarker(Arc<AtomicUsize>);

impl Drop for DropMarker {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn on_start_and_on_completion_bracket_every_collection_including_cancelled_ones() {
    let started = Arc::new(AtomicUsize::new(0));
    let completed = Arc::new(AtomicUsize::new(0));
    let (on_start, on_completion) = (Arc::clone(&started), Arc::clone(&completed));
    let numbers = flow_of(vec![1, 2, 3])
        .on_start(move || {
            on_start.fetch_add(1, Ordering::SeqCst);
        })
        .on_completion(move || {
            on_completion.fetch_add(1, Ordering::SeqCst);
        });
    assert_eq!(pollster::block_on(numbers.to_vec()), vec![1, 2, 3]);
    assert_eq!(completed.load(Ordering::SeqCst), 1);
    let run = numbers.open();
    assert_eq!(started.load(Ordering::SeqCst), 2);
    drop(run);
    assert_eq!(
        completed.load(Ordering::SeqCst),
        2,
        "a cancelled run completes too"
    );
}

#[test]
fn start_with_emits_its_value_before_every_run_of_the_upstream() {
    let scheduler = TestScheduler::new();
    let slow = flow(|emitter| async move {
        delay(Duration::from_millis(400)).await;
        emitter.emit(2).await;
    })
    .start_with(1);
    let mut run = slow.open();
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    assert_eq!(
        std::pin::Pin::new(&mut run).poll_next(&mut cx),
        std::task::Poll::Ready(Some(1)),
        "the first value does not wait for the upstream"
    );
    assert_eq!(scheduler.block_on(slow.to_vec()), Ok(vec![1, 2]));
}
