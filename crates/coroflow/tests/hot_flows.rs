use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use coroflow::{BufferOverflow, Emitter, Flow, MutableSharedFlow, MutableStateFlow, Turbine};

#[test]
fn a_state_flow_emits_its_current_value_and_then_only_changes() {
    let state = MutableStateFlow::new(1);
    let mut run = Turbine::of(&state.as_state_flow());
    assert_eq!(run.next_now(), Poll::Ready(Some(1)));
    assert_eq!(run.next_now(), Poll::Pending);
    state.set(1);
    assert_eq!(
        run.next_now(),
        Poll::Pending,
        "an equal value is not a change"
    );
    state.set(2);
    assert_eq!(run.next_now(), Poll::Ready(Some(2)));
}

#[test]
fn a_slow_state_collector_sees_only_the_latest_value() {
    let state = MutableStateFlow::new(0);
    let mut run = Turbine::of(&state);
    assert_eq!(run.next_now(), Poll::Ready(Some(0)));
    state.set(1);
    state.set(2);
    state.update(|value| value + 1);
    assert_eq!(run.next_now(), Poll::Ready(Some(3)));
    assert_eq!(run.next_now(), Poll::Pending);
    assert_eq!(state.value(), 3);
}

#[test]
fn state_flows_count_their_collectors_and_share_identity() {
    let state = MutableStateFlow::new("a".to_string());
    let read_only = state.as_state_flow();
    assert!(read_only.same_as(&state.as_state_flow()));
    assert!(!read_only.same_as(&MutableStateFlow::new("a".to_string()).as_state_flow()));
    assert_eq!(read_only.subscription_count().value(), 0);
    let first = read_only.open();
    let second = state.open();
    assert_eq!(state.subscription_count().value(), 2);
    drop(first);
    assert_eq!(read_only.subscription_count().value(), 1);
    drop(second);
    assert_eq!(state.subscription_count().value(), 0);
    assert_eq!(read_only.value(), "a");
}

#[test]
fn a_shared_flow_broadcasts_to_every_collector_and_replays_to_new_ones() {
    let events = MutableSharedFlow::new(1, 4);
    let mut early = Turbine::of(&events.as_shared_flow());
    assert!(events.try_emit("saved"));
    assert!(events.try_emit("deleted"));
    assert_eq!(early.next_now(), Poll::Ready(Some("saved")));
    assert_eq!(early.next_now(), Poll::Ready(Some("deleted")));
    assert_eq!(early.next_now(), Poll::Pending);
    let mut late = Turbine::of(&events);
    assert_eq!(late.next_now(), Poll::Ready(Some("deleted")));
    assert_eq!(late.next_now(), Poll::Pending);
    assert_eq!(events.subscription_count().value(), 2);
    drop(late);
    assert_eq!(events.subscription_count().value(), 1);
}

#[test]
fn a_shared_flow_without_collectors_keeps_only_its_replay() {
    let silent = MutableSharedFlow::new(0, 1);
    assert!(silent.try_emit(1));
    let mut run = Turbine::of(&silent);
    assert_eq!(run.next_now(), Poll::Pending);
    assert!(silent.try_emit(2));
    assert_eq!(run.next_now(), Poll::Ready(Some(2)));
    let replaying = MutableSharedFlow::new(2, 0);
    let accepted: Vec<bool> = (1..=5).map(|value| replaying.try_emit(value)).collect();
    assert_eq!(accepted, vec![true; 5], "nobody to wait for");
    let mut late = Turbine::of(&replaying);
    assert_eq!(late.next_now(), Poll::Ready(Some(4)));
    assert_eq!(late.next_now(), Poll::Ready(Some(5)));
    assert_eq!(late.next_now(), Poll::Pending);
}

#[test]
fn a_rendezvous_emit_waits_until_a_collector_takes_the_value_or_leaves() {
    let events = MutableSharedFlow::new(0, 0);
    let mut run = Turbine::of(&events);
    assert!(!events.try_emit(1), "try_emit never waits");
    let mut cx = Context::from_waker(Waker::noop());
    let mut taken = events.emit(2);
    assert_eq!(Pin::new(&mut taken).poll(&mut cx), Poll::Pending);
    assert_eq!(run.next_now(), Poll::Ready(Some(2)));
    assert_eq!(Pin::new(&mut taken).poll(&mut cx), Poll::Ready(()));
    let mut abandoned = events.emit(3);
    assert_eq!(Pin::new(&mut abandoned).poll(&mut cx), Poll::Pending);
    drop(run);
    assert_eq!(Pin::new(&mut abandoned).poll(&mut cx), Poll::Ready(()));
}

#[test]
fn drop_latest_drops_new_values_while_a_collector_lags_and_try_emit_still_succeeds() {
    let events = MutableSharedFlow::with_overflow(0, 2, BufferOverflow::DropLatest);
    let mut run = Turbine::of(&events);
    let accepted: Vec<bool> = (1..=3).map(|value| events.try_emit(value)).collect();
    assert_eq!(
        accepted,
        vec![true, true, true],
        "a drop policy never waits"
    );
    assert_eq!(run.next_now(), Poll::Ready(Some(1)));
    assert_eq!(run.next_now(), Poll::Ready(Some(2)));
    assert_eq!(run.next_now(), Poll::Pending);
}

#[test]
fn drop_oldest_lets_a_lagging_collector_skip_values_that_fell_out() {
    let events = MutableSharedFlow::with_overflow(0, 2, BufferOverflow::DropOldest);
    let mut run = Turbine::of(&events);
    for value in 1..=5 {
        assert!(events.try_emit(value));
    }
    assert_eq!(run.next_now(), Poll::Ready(Some(4)));
    assert_eq!(run.next_now(), Poll::Ready(Some(5)));
    assert_eq!(run.next_now(), Poll::Pending);
}

#[test]
fn update_may_read_the_flow_it_updates_and_retries_on_a_racing_write() {
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let state = MutableStateFlow::new(1);
        let mut calls = 0;
        state.update(|value| {
            calls += 1;
            if calls == 1 {
                state.set(10);
            }
            value + state.value()
        });
        let _ = done.send((state.value(), calls));
    });
    assert_eq!(
        finished.recv_timeout(std::time::Duration::from_secs(5)),
        Ok((20, 2)),
        "the transform ran once more on the value the racing write left"
    );
}

#[test]
fn compare_and_set_update_and_get_and_get_and_update_behave_like_kotlin() {
    let state = MutableStateFlow::new(1);
    assert!(!state.compare_and_set(&2, 5), "the value was not 2");
    assert!(state.compare_and_set(&1, 5));
    assert!(state.compare_and_set(&5, 5), "an equal value still matches");
    assert_eq!(state.update_and_get(|value| value * 2), 10);
    assert_eq!(state.get_and_update(|value| value + 1), 10);
    assert_eq!(state.value(), 11);
}

#[test]
fn subscription_count_is_a_state_flow_that_follows_collectors() {
    let events = MutableSharedFlow::<u32>::new(0, 4);
    let mut counts = Turbine::of(&events.subscription_count());
    assert_eq!(counts.next_now(), Poll::Ready(Some(0)));
    let first = events.open();
    assert_eq!(counts.next_now(), Poll::Ready(Some(1)));
    let second = events.open();
    drop(first);
    assert_eq!(
        counts.next_now(),
        Poll::Ready(Some(1)),
        "a slow collector sees the latest count"
    );
    drop(second);
    assert_eq!(counts.next_now(), Poll::Ready(Some(0)));
    assert_eq!(
        events.subscription_count().subscription_count().value(),
        1,
        "the count is a state flow whose own collector is counted"
    );
}

#[test]
fn replay_cache_lists_the_replayed_values_and_resetting_forgets_them_for_new_collectors() {
    let events = MutableSharedFlow::new(2, 4);
    let mut early = Turbine::of(&events);
    for value in 1..=3 {
        assert!(events.try_emit(value));
    }
    assert_eq!(events.replay_cache(), vec![2, 3]);
    events.reset_replay_cache();
    assert!(events.replay_cache().is_empty());
    assert_eq!(
        early.next_now(),
        Poll::Ready(Some(1)),
        "a current collector keeps its values"
    );
    let mut late = Turbine::of(&events);
    assert_eq!(late.next_now(), Poll::Pending);
    assert!(events.try_emit(4));
    assert_eq!(late.next_now(), Poll::Ready(Some(4)));
    assert_eq!(events.replay_cache(), vec![4]);
}

#[test]
fn on_subscription_emits_to_its_own_collector_after_subscribing() {
    let events = MutableSharedFlow::new(0, 4);
    let feed = events.clone();
    let greeted = events.on_subscription(async move |emitter: Emitter<&str>| {
        emitter.emit("hello").await;
        feed.try_emit("broadcast");
    });
    let mut other = Turbine::of(&events);
    let mut run = Turbine::of(&greeted);
    assert_eq!(run.next_now(), Poll::Ready(Some("hello")));
    assert_eq!(
        run.next_now(),
        Poll::Ready(Some("broadcast")),
        "subscribed before the action ran"
    );
    assert_eq!(other.next_now(), Poll::Ready(Some("broadcast")));
    assert_eq!(
        other.next_now(),
        Poll::Pending,
        "the greeting went to one collector"
    );
}

#[test]
fn a_waiting_value_stays_invisible_until_the_slowest_collector_makes_room() {
    let events = MutableSharedFlow::new(0, 1);
    let mut fast = Turbine::of(&events);
    let mut slow = Turbine::of(&events);
    let mut cx = Context::from_waker(Waker::noop());
    assert!(events.try_emit(1));
    assert_eq!(fast.next_now(), Poll::Ready(Some(1)));
    let mut second = events.emit(2);
    assert_eq!(Pin::new(&mut second).poll(&mut cx), Poll::Pending);
    assert_eq!(fast.next_now(), Poll::Pending, "2 waits in the queue");
    assert_eq!(slow.next_now(), Poll::Ready(Some(1)));
    assert_eq!(Pin::new(&mut second).poll(&mut cx), Poll::Ready(()));
    assert_eq!(fast.next_now(), Poll::Ready(Some(2)));
    assert_eq!(slow.next_now(), Poll::Ready(Some(2)));
}

#[test]
fn a_cancelled_emit_withdraws_its_value() {
    let events = MutableSharedFlow::new(0, 1);
    let mut run = Turbine::of(&events);
    let mut cx = Context::from_waker(Waker::noop());
    assert!(events.try_emit(1));
    let mut withdrawn = events.emit(2);
    assert_eq!(Pin::new(&mut withdrawn).poll(&mut cx), Poll::Pending);
    drop(withdrawn);
    assert_eq!(run.next_now(), Poll::Ready(Some(1)));
    assert!(events.try_emit(3));
    assert_eq!(
        run.next_now(),
        Poll::Ready(Some(3)),
        "2 was never delivered"
    );
    assert_eq!(run.next_now(), Poll::Pending);
}
