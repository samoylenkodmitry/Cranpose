use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::Poll,
    time::Duration,
};

use coroflow::{
    CoroutineScope, FlowExt, MainScope, SharingStarted, TaskFailed, TestScheduler, TimedOut,
    Turbine, combine3, delay, flow, flow_of, merge, with_timeout,
};

fn after(millis: u64, value: u32) -> impl coroflow::SendFlow<Item = u32> + Clone {
    flow(move |emitter| async move {
        delay(Duration::from_millis(millis)).await;
        emitter.emit(value).await;
    })
}

struct DropMarker(Arc<AtomicUsize>);

impl Drop for DropMarker {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn merge_interleaves_values_as_they_arrive_and_completes_after_all() {
    let scheduler = TestScheduler::new();
    let merged = merge(vec![
        after(30, 3).boxed(),
        after(10, 1).boxed(),
        after(20, 2).boxed(),
    ]);
    assert_eq!(scheduler.block_on(merged.to_vec()), Ok(vec![1, 2, 3]));
    assert_eq!(scheduler.now(), Duration::from_millis(30));
    let empty: Vec<coroflow::BoxFlow<u32>> = Vec::new();
    assert_eq!(pollster::block_on(merge(empty).to_vec()), Vec::<u32>::new());
}

#[test]
fn merge_takes_turns_between_ready_flows() {
    let merged = merge(vec![flow_of(vec![1, 1, 1]), flow_of(vec![2, 2, 2])]);
    assert_eq!(pollster::block_on(merged.to_vec()), vec![1, 2, 1, 2, 1, 2]);
}

#[test]
fn zip_pairs_values_in_order_and_stops_with_the_shorter_flow() {
    let zipped = flow_of(vec![1, 2, 3]).zip(flow_of(vec!["a", "b"]), |n, s| format!("{n}{s}"));
    assert_eq!(pollster::block_on(zipped.to_vec()), vec!["1a", "2b"]);
}

#[test]
fn combine3_waits_for_all_three_and_emits_on_each_change() {
    let scheduler = TestScheduler::new();
    let combined = combine3(after(10, 1), after(20, 2), after(30, 3), |a, b, c| {
        a + b + c
    });
    assert_eq!(scheduler.block_on(combined.to_vec()), Ok(vec![6]));
    let never = combine3(
        flow_of(vec![1]),
        flow_of(Vec::<u32>::new()),
        flow_of(vec![3]),
        |a, b, c| a + b + c,
    );
    assert_eq!(pollster::block_on(never.to_vec()), Vec::<u32>::new());
}

#[test]
fn flat_map_concat_runs_inner_flows_one_after_another() {
    let scheduler = TestScheduler::new();
    let concatenated = flow_of(vec![30, 10]).flat_map_concat(|millis| after(millis, millis as u32));
    assert_eq!(scheduler.block_on(concatenated.to_vec()), Ok(vec![30, 10]));
    assert_eq!(scheduler.now(), Duration::from_millis(40));
}

#[test]
fn flat_map_merge_runs_up_to_its_concurrency_at_once() {
    let scheduler = TestScheduler::new();
    let merged = flow_of(vec![100, 100, 100]).flat_map_merge(2, |millis| after(millis, 1));
    assert_eq!(scheduler.block_on(merged.to_vec()), Ok(vec![1, 1, 1]));
    assert_eq!(scheduler.now(), Duration::from_millis(200));
    let fast = flow_of(vec![30, 10]).flat_map_merge(4, |millis| after(millis, millis as u32));
    assert_eq!(scheduler.block_on(fast.to_vec()), Ok(vec![10, 30]));
}

#[test]
fn scan_skip_and_filter_map_shape_values() {
    let sums = flow_of(vec![1, 2, 3]).scan(0, |sum, value| sum + value);
    assert_eq!(pollster::block_on(sums.to_vec()), vec![0, 1, 3, 6]);
    let tail = flow_of(vec![1, 2, 3, 4]).skip(2);
    assert_eq!(pollster::block_on(tail.to_vec()), vec![3, 4]);
    let parsed = flow_of(vec!["1", "x", "3"]).filter_map(|text| text.parse::<u32>().ok());
    assert_eq!(pollster::block_on(parsed.to_vec()), vec![1, 3]);
}

#[test]
fn catch_switches_to_the_fallback_flow_on_the_first_error() {
    let flaky = flow_of(vec![Ok(1), Err("boom"), Ok(2)]);
    let recovered = flaky.catch(|error: &str| flow_of(vec![error.len() as u32 * 100]));
    assert_eq!(pollster::block_on(recovered.to_vec()), vec![1, 400]);
}

fn failing_runs(
    failures: usize,
    runs: Arc<AtomicUsize>,
) -> impl coroflow::SendFlow<Item = Result<u32, String>> + Clone {
    flow(move |emitter| {
        let run = runs.fetch_add(1, Ordering::SeqCst);
        async move {
            emitter.emit(Ok(1)).await;
            if run < failures {
                emitter.emit(Err(format!("run {run} failed"))).await;
            } else {
                emitter.emit(Ok(2)).await;
            }
        }
    })
}

#[test]
fn retry_reruns_the_upstream_until_it_succeeds() {
    let runs = Arc::new(AtomicUsize::new(0));
    let values = pollster::block_on(failing_runs(2, Arc::clone(&runs)).retry(3).to_vec());
    assert_eq!(values, vec![Ok(1), Ok(1), Ok(1), Ok(2)]);
    assert_eq!(runs.load(Ordering::SeqCst), 3);
}

#[test]
fn retry_passes_the_last_error_on_once_attempts_run_out() {
    let runs = Arc::new(AtomicUsize::new(0));
    let values = pollster::block_on(failing_runs(10, Arc::clone(&runs)).retry(2).to_vec());
    assert_eq!(
        values,
        vec![Ok(1), Ok(1), Ok(1), Err("run 2 failed".to_string())]
    );
    assert_eq!(runs.load(Ordering::SeqCst), 3);
}

#[test]
fn retry_when_backs_off_on_virtual_time() {
    let scheduler = TestScheduler::new();
    let runs = Arc::new(AtomicUsize::new(0));
    let backoff = failing_runs(2, Arc::clone(&runs))
        .retry_when(|_: &String, attempt| Some(Duration::from_millis(100 << attempt)));
    let values = scheduler.block_on(backoff.to_vec());
    assert_eq!(values.map(|values| values.len()), Ok(4));
    assert_eq!(scheduler.now(), Duration::from_millis(100 + 200));
}

#[test]
fn share_in_multicasts_one_upstream_and_replays_to_late_collectors() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let starts = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&starts);
    let ticks = flow(move |emitter| {
        counter.fetch_add(1, Ordering::SeqCst);
        async move {
            for tick in 1..=3 {
                delay(Duration::from_millis(10)).await;
                emitter.emit(tick).await;
            }
        }
    })
    .share_in(&scope, SharingStarted::while_subscribed(Duration::ZERO), 1);
    let mut first = Turbine::of(&ticks);
    let mut second = Turbine::of(&ticks);
    scheduler.advance_time_by(Duration::from_millis(15));
    assert_eq!(first.next_now(), Poll::Ready(Some(1)));
    assert_eq!(second.next_now(), Poll::Ready(Some(1)));
    scheduler.advance_time_by(Duration::from_millis(10));
    let mut late = Turbine::of(&ticks);
    assert_eq!(late.next_now(), Poll::Ready(Some(2)), "the replayed value");
    assert_eq!(
        starts.load(Ordering::SeqCst),
        1,
        "one upstream for all collectors"
    );
    drop((first, second, late));
    scheduler.run_current();
    assert_eq!(ticks.subscription_count(), 0);
}

#[test]
fn with_timeout_returns_in_time_results_and_cancels_late_work() {
    let scheduler = TestScheduler::new();
    let quick = scheduler.block_on(with_timeout(Duration::from_millis(50), async {
        delay(Duration::from_millis(10)).await;
        7
    }));
    assert_eq!(quick, Ok(Ok(7)));
    let dropped = Arc::new(AtomicUsize::new(0));
    let marker = Arc::clone(&dropped);
    let slow = scheduler.block_on(with_timeout(Duration::from_millis(50), async move {
        let _guard = DropMarker(marker);
        delay(Duration::from_secs(10)).await;
    }));
    assert_eq!(slow, Ok(Err(TimedOut)));
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(scheduler.now(), Duration::from_millis(60));
}

#[test]
fn async_runs_children_concurrently_and_reports_failure() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let first = scope.async_(async {
        delay(Duration::from_millis(100)).await;
        1
    });
    let second = scope.async_(async {
        delay(Duration::from_millis(100)).await;
        2
    });
    let sum = scheduler.block_on(async move { Ok::<_, TaskFailed>(first.await? + second.await?) });
    assert_eq!(sum, Ok(Ok(3)));
    assert_eq!(
        scheduler.now(),
        Duration::from_millis(100),
        "they ran side by side"
    );

    let doomed = scope.async_(async {
        delay(Duration::from_secs(1)).await;
        0
    });
    assert!(doomed.job().is_active());
    scope.cancel();
    assert_eq!(scheduler.block_on(doomed), Ok(Err(TaskFailed)));
}

#[test]
fn main_scope_async_may_hold_thread_bound_state() {
    let scheduler = TestScheduler::new();
    let scope = MainScope::new(scheduler.main_dispatcher());
    let shared = std::rc::Rc::new(std::cell::Cell::new(20));
    let local = std::rc::Rc::clone(&shared);
    let deferred = scope.async_(async move { local.get() + 1 });
    assert_eq!(scheduler.block_on(deferred), Ok(Ok(21)));
}
