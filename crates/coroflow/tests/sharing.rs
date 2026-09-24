use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::Poll,
    time::Duration,
};

use coroflow::{
    CoroutineScope, Flow, FlowExt, MainScope, SendFlow, SharingCommand, SharingStarted,
    TestScheduler, Turbine, delay, flow, flow_of,
};

struct Ticker {
    starts: Arc<AtomicUsize>,
    live: Arc<AtomicUsize>,
}

impl Ticker {
    fn new() -> Self {
        Self {
            starts: Arc::new(AtomicUsize::new(0)),
            live: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn flow(&self) -> impl SendFlow<Item = u32> {
        let (starts, live) = (Arc::clone(&self.starts), Arc::clone(&self.live));
        flow(move |emitter| {
            starts.fetch_add(1, Ordering::SeqCst);
            let live = Arc::clone(&live);
            async move {
                live.fetch_add(1, Ordering::SeqCst);
                let _live = LiveGuard(live);
                let mut tick = 0;
                loop {
                    tick += 1;
                    emitter.emit(tick).await;
                    delay(Duration::from_millis(100)).await;
                }
            }
        })
    }
}

struct LiveGuard(Arc<AtomicUsize>);

impl Drop for LiveGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[test]
fn while_subscribed_starts_with_the_first_collector_and_stops_after_the_timeout() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let ticker = Ticker::new();
    let state = ticker.flow().state_in(
        &scope,
        SharingStarted::while_subscribed(Duration::from_millis(5_000)),
        0,
    );
    scheduler.advance_time_by(Duration::from_millis(1_000));
    assert_eq!(
        ticker.starts.load(Ordering::SeqCst),
        0,
        "no collector, no upstream"
    );
    assert_eq!(state.value(), 0);

    let run = state.open();
    scheduler.advance_time_by(Duration::from_millis(250));
    assert_eq!(ticker.starts.load(Ordering::SeqCst), 1);
    assert_eq!(state.value(), 3);

    drop(run);
    scheduler.advance_time_by(Duration::from_millis(4_900));
    assert_eq!(
        ticker.live.load(Ordering::SeqCst),
        1,
        "still inside the stop timeout"
    );
    scheduler.advance_time_by(Duration::from_millis(200));
    assert_eq!(
        ticker.live.load(Ordering::SeqCst),
        0,
        "stopped after the timeout"
    );
    let last = state.value();
    scheduler.advance_time_by(Duration::from_millis(1_000));
    assert_eq!(state.value(), last, "the last value is kept while stopped");

    let _again = state.open();
    scheduler.advance_time_by(Duration::from_millis(10));
    assert_eq!(
        ticker.starts.load(Ordering::SeqCst),
        2,
        "a new collector restarts it"
    );
}

#[test]
fn a_collector_returning_within_the_timeout_keeps_the_same_upstream_run() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let ticker = Ticker::new();
    let state = ticker.flow().state_in(
        &scope,
        SharingStarted::while_subscribed(Duration::from_millis(5_000)),
        0,
    );
    let first = state.open();
    scheduler.advance_time_by(Duration::from_millis(50));
    drop(first);
    scheduler.advance_time_by(Duration::from_millis(3_000));
    let _second = state.open();
    scheduler.advance_time_by(Duration::from_millis(10_000));
    assert_eq!(ticker.starts.load(Ordering::SeqCst), 1);
    assert_eq!(ticker.live.load(Ordering::SeqCst), 1);
}

#[test]
fn eagerly_starts_at_once_and_lazily_waits_for_the_first_collector() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let eager = Ticker::new();
    let lazy = Ticker::new();
    let eager_state = eager.flow().state_in(&scope, SharingStarted::Eagerly, 0);
    let lazy_state = lazy.flow().state_in(&scope, SharingStarted::Lazily, 0);
    scheduler.advance_time_by(Duration::from_millis(150));
    assert_eq!(eager_state.value(), 2);
    assert_eq!(lazy.starts.load(Ordering::SeqCst), 0);
    drop(lazy_state.open());
    scheduler.advance_time_by(Duration::from_millis(10_000));
    assert_eq!(lazy.live.load(Ordering::SeqCst), 1, "lazily never stops");
    assert_eq!(eager.starts.load(Ordering::SeqCst), 1);
}

#[test]
fn cancelling_the_scope_stops_sharing_and_keeps_the_last_value() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let ticker = Ticker::new();
    let state = ticker.flow().state_in(&scope, SharingStarted::Eagerly, 0);
    scheduler.advance_time_by(Duration::from_millis(150));
    drop(scope);
    scheduler.run_current();
    assert_eq!(ticker.live.load(Ordering::SeqCst), 0);
    assert_eq!(state.value(), 2);
}

#[test]
fn a_main_scope_can_share_a_flow_that_captures_thread_bound_state() {
    let scheduler = TestScheduler::new();
    let scope = MainScope::new(scheduler.main_dispatcher());
    let multiplier = Rc::new(Cell::new(10));
    let factor = Rc::clone(&multiplier);
    let state = Ticker::new()
        .flow()
        .map(move |tick| tick * factor.get())
        .state_in(&scope, SharingStarted::Eagerly, 0);
    scheduler.advance_time_by(Duration::from_millis(10));
    assert_eq!(state.value(), 10);
    let mut run = Turbine::of(&state);
    assert_eq!(run.next_now(), Poll::Ready(Some(10)));
    multiplier.set(100);
    scheduler.advance_time_by(Duration::from_millis(100));
    assert_eq!(run.next_now(), Poll::Ready(Some(200)));
}

#[test]
fn state_in_first_waits_for_the_first_value_and_keeps_following_the_upstream() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let ticker = Ticker::new();
    let state = scheduler.block_on(ticker.flow().state_in_first(&scope));
    let Ok(Some(state)) = state else {
        panic!("the upstream emitted, so there is a state");
    };
    assert_eq!(state.value(), 1);
    scheduler.advance_time_by(Duration::from_millis(250));
    assert_eq!(state.value(), 3, "it keeps collecting eagerly");
    let empty = scheduler.block_on(flow_of(Vec::<u32>::new()).state_in_first(&scope));
    assert!(matches!(empty, Ok(None)), "an empty upstream has no state");
}

#[test]
fn a_custom_strategy_follows_its_commands_and_can_reset_the_state() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let ticker = Ticker::new();
    let started = SharingStarted::custom(|count| {
        count
            .map(|collectors| {
                if collectors > 0 {
                    SharingCommand::Start
                } else {
                    SharingCommand::StopAndResetReplayCache
                }
            })
            .boxed()
    });
    let state = ticker.flow().state_in(&scope, started, 0);
    scheduler.advance_time_by(Duration::from_millis(100));
    assert_eq!(ticker.starts.load(Ordering::SeqCst), 0);
    let run = state.open();
    scheduler.advance_time_by(Duration::from_millis(250));
    assert_eq!(state.value(), 3);
    drop(run);
    scheduler.run_current();
    assert_eq!(ticker.live.load(Ordering::SeqCst), 0, "stopped at once");
    assert_eq!(state.value(), 0, "reset to the initial value");
}

#[test]
fn replay_expiration_resets_a_stopped_state_once_it_expires() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let ticker = Ticker::new();
    let started = SharingStarted::while_subscribed(Duration::from_millis(100))
        .replay_expiration(Duration::from_millis(1_000));
    let state = ticker.flow().state_in(&scope, started, 0);
    let run = state.open();
    scheduler.advance_time_by(Duration::from_millis(250));
    drop(run);
    scheduler.advance_time_by(Duration::from_millis(500));
    assert_eq!(state.value(), 4, "stopped at 350 ms, but not expired yet");
    scheduler.advance_time_by(Duration::from_millis(700));
    assert_eq!(state.value(), 0, "expired back to the initial value");
}

#[test]
fn share_in_makes_its_upstream_wait_for_a_collector_that_falls_behind() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let produced = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&produced);
    let fast = flow(move |emitter| {
        let counter = Arc::clone(&counter);
        async move {
            for value in 1..=100_u32 {
                counter.fetch_add(1, Ordering::SeqCst);
                emitter.emit(value).await;
            }
        }
    });
    let shared = fast.share_in(&scope, SharingStarted::Eagerly, 0);
    let mut run = Turbine::of(&shared);
    scheduler.run_current();
    assert_eq!(
        produced.load(Ordering::SeqCst),
        65,
        "64 buffered and one waiting to be emitted"
    );
    let mut received = Vec::new();
    loop {
        scheduler.run_current();
        match run.next_now() {
            Poll::Ready(Some(value)) => received.push(value),
            _ => break,
        }
    }
    assert_eq!(
        received,
        (1..=100).collect::<Vec<_>>(),
        "nothing was dropped"
    );
}
