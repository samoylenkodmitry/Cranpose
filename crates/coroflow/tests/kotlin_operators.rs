use std::{
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    task::Poll,
    time::Duration,
};

use coroflow::{
    CoroutineScope, Flow, FlowExt, JobOutcome, MutableStateFlow, SendFlow, TestScheduler, TimedOut,
    Turbine, combine_all, combine4, combine5, delay, flow, flow_of,
};

use crate::support::{endless, timed};

fn every_110ms() -> impl SendFlow<Item = u32> + Clone {
    flow(|emitter| async move {
        for value in 0..10 {
            emitter.emit(value).await;
            delay(Duration::from_millis(110)).await;
        }
    })
}

#[test]
fn sample_emits_the_latest_value_once_per_period_like_kotlins_example() {
    let scheduler = TestScheduler::new();
    let sampled = every_110ms().sample(Duration::from_millis(200));
    assert_eq!(
        scheduler.block_on(sampled.to_vec()),
        Ok(vec![1, 3, 5, 7, 9])
    );
}

#[test]
fn timeout_reports_a_silent_upstream_and_completes() {
    let scheduler = TestScheduler::new();
    let stalls = timed(vec![(0, 1), (50, 2), (300, 3)]).timeout(Duration::from_millis(100));
    assert_eq!(
        scheduler.block_on(stalls.to_vec()),
        Ok(vec![Ok(1), Ok(2), Err(TimedOut)])
    );
    assert_eq!(scheduler.now(), Duration::from_millis(150));
    let steady = timed(vec![(90, 1), (90, 2)]).timeout(Duration::from_millis(100));
    assert_eq!(scheduler.block_on(steady.to_vec()), Ok(vec![Ok(1), Ok(2)]));
}

#[test]
fn on_empty_emits_a_fallback_only_for_an_empty_flow() {
    let fallback = async |emitter: coroflow::Emitter<u32>| {
        emitter.emit(0).await;
        emitter.emit(1).await;
    };
    let empty = flow_of(Vec::<u32>::new()).on_empty(fallback);
    assert_eq!(pollster::block_on(empty.to_vec()), vec![0, 1]);
    let full = flow_of(vec![7_u32]).on_empty(fallback);
    assert_eq!(pollster::block_on(full.to_vec()), vec![7]);
}

#[test]
fn with_index_distinct_by_running_reduce_and_chunked_shape_values_like_kotlin() {
    assert_eq!(
        pollster::block_on(flow_of(vec!['a', 'b']).with_index().to_vec()),
        vec![(0, 'a'), (1, 'b')]
    );
    let by_id = flow_of(vec![(1, "a"), (1, "b"), (2, "c"), (1, "d")])
        .distinct_until_changed_by(|(id, _)| *id);
    assert_eq!(
        pollster::block_on(by_id.to_vec()),
        vec![(1, "a"), (2, "c"), (1, "d")]
    );
    let sums = flow_of(vec![1, 2, 3, 4]).running_reduce(|sum, value| sum + value);
    assert_eq!(pollster::block_on(sums.to_vec()), vec![1, 3, 6, 10]);
    let pairs = flow_of(vec![1, 2, 3, 4, 5]).chunked(2);
    assert_eq!(
        pollster::block_on(pairs.to_vec()),
        vec![vec![1, 2], vec![3, 4], vec![5]]
    );
}

#[test]
fn flatten_concat_runs_inner_flows_in_turn_and_flatten_merge_interleaves_them() {
    let scheduler = TestScheduler::new();
    let inner = || {
        flow_of(vec![
            timed(vec![(10, 1), (100, 2)]),
            timed(vec![(50, 10), (10, 20)]),
        ])
    };
    assert_eq!(
        scheduler.block_on(inner().flatten_concat().to_vec()),
        Ok(vec![1, 2, 10, 20])
    );
    assert_eq!(
        scheduler.block_on(inner().flatten_merge(2).to_vec()),
        Ok(vec![1, 10, 20, 2])
    );
}

#[test]
fn emit_all_forwards_every_value_of_another_flow() {
    let numbers = flow(|emitter| async move {
        emitter.emit(0).await;
        emitter.emit_all(flow_of(vec![1, 2])).await;
        emitter.emit(3).await;
    });
    assert_eq!(pollster::block_on(numbers.to_vec()), vec![0, 1, 2, 3]);
}

#[test]
fn launch_in_collects_in_the_scope_and_returns_its_job() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let job = timed(vec![(10, 1), (10, 2)])
        .on_each(move |value| {
            log.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(*value);
        })
        .launch_in(&scope);
    scheduler.advance_time_by(Duration::from_millis(100));
    assert_eq!(job.outcome(), Some(JobOutcome::Completed));
    assert_eq!(
        *seen.lock().unwrap_or_else(PoisonError::into_inner),
        vec![1, 2]
    );
}

#[test]
fn produce_in_feeds_a_channel_and_stops_once_the_receiver_is_gone() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let dropped = Arc::new(AtomicUsize::new(0));
    let endless = endless(&dropped);
    let receiver = endless.produce_in(&scope);
    scheduler.advance_time_by(Duration::from_millis(30));
    assert_eq!(receiver.try_recv(), Ok(1));
    assert_eq!(receiver.try_recv(), Ok(2));
    drop(receiver);
    scheduler.advance_time_by(Duration::from_millis(30));
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        1,
        "the upstream was cancelled"
    );
}

#[test]
fn combine4_combine5_and_combine_all_emit_once_every_flow_has_a_value() {
    let scheduler = TestScheduler::new();
    let four = combine4(
        timed(vec![(10, 1)]),
        timed(vec![(20, 2)]),
        timed(vec![(30, 3)]),
        timed(vec![(40, 4), (10, 40)]),
        |a, b, c, d| a + b + c + d,
    );
    assert_eq!(scheduler.block_on(four.to_vec()), Ok(vec![10, 46]));
    let five = combine5(
        flow_of(vec![1]),
        flow_of(vec![2]),
        flow_of(vec![3]),
        flow_of(vec![4]),
        flow_of(vec![5]),
        |a, b, c, d, e| vec![*a, *b, *c, *d, *e],
    );
    assert_eq!(pollster::block_on(five.to_vec()), vec![vec![1, 2, 3, 4, 5]]);
    let states: Vec<_> = (0..3).map(MutableStateFlow::new).collect();
    let total = combine_all(
        states.iter().map(MutableStateFlow::as_state_flow).collect(),
        |values: &[i32]| values.iter().sum::<i32>(),
    );
    let mut run = Turbine::of(&total);
    assert_eq!(run.next_now(), Poll::Ready(Some(3)));
    if let Some(last) = states.last() {
        last.set(10);
    }
    assert_eq!(run.next_now(), Poll::Ready(Some(11)));
    let nothing = combine_all(Vec::<coroflow::FlowOf<u32>>::new(), |values: &[u32]| {
        values.len()
    });
    assert_eq!(pollster::block_on(nothing.to_vec()), Vec::<usize>::new());
    let starved = combine_all(
        vec![flow_of(vec![1]), flow_of(Vec::new())],
        |values: &[u32]| values.len(),
    );
    assert_eq!(pollster::block_on(starved.to_vec()), Vec::<usize>::new());
}

#[test]
fn fold_reduce_count_last_and_single_run_the_flow_to_one_answer() {
    let numbers = flow_of(vec![1, 2, 3, 4]);
    assert_eq!(
        pollster::block_on(numbers.fold(10, |sum, value| sum + value)),
        20
    );
    assert_eq!(pollster::block_on(numbers.reduce(|a, b| a * b)), Some(24));
    assert_eq!(pollster::block_on(numbers.count()), 4);
    assert_eq!(pollster::block_on(numbers.last()), Some(4));
    assert_eq!(pollster::block_on(numbers.single()), None);
    assert_eq!(pollster::block_on(flow_of(vec![7]).single()), Some(7));
    let empty = flow_of(Vec::<u32>::new());
    assert_eq!(pollster::block_on(empty.reduce(|a, b| a + b)), None);
    assert_eq!(pollster::block_on(empty.last()), None);
    assert_eq!(pollster::block_on(empty.single()), None);
}

#[test]
fn a_chain_of_new_operators_stays_send() {
    fn assert_send_flow<F>(flow: &F)
    where
        F: Flow + Send,
        F::Run: Send,
    {
        let _ = flow;
    }
    let chain = timed(vec![(0, 1)])
        .sample(Duration::from_millis(10))
        .with_index()
        .distinct_until_changed_by(|(index, _)| *index)
        .chunked(2)
        .timeout(Duration::from_millis(50));
    assert_send_flow(&chain);
}
