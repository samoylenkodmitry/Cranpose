use std::task::Poll;

use coroflow::{Flow, MutableSharedFlow, MutableStateFlow, Turbine};

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
    assert_eq!(read_only.subscription_count(), 0);
    let first = read_only.open();
    let second = state.open();
    assert_eq!(state.subscription_count(), 2);
    drop(first);
    assert_eq!(read_only.subscription_count(), 1);
    drop(second);
    assert_eq!(state.subscription_count(), 0);
    assert_eq!(read_only.value(), "a");
}

#[test]
fn a_shared_flow_broadcasts_to_every_collector_and_replays_to_new_ones() {
    let events = MutableSharedFlow::new(1, 4);
    let mut early = Turbine::of(&events.as_shared_flow());
    events.emit("saved");
    events.emit("deleted");
    assert_eq!(early.next_now(), Poll::Ready(Some("saved")));
    assert_eq!(early.next_now(), Poll::Ready(Some("deleted")));
    assert_eq!(early.next_now(), Poll::Pending);
    let mut late = Turbine::of(&events);
    assert_eq!(late.next_now(), Poll::Ready(Some("deleted")));
    assert_eq!(late.next_now(), Poll::Pending);
    assert_eq!(events.subscription_count(), 2);
    drop(late);
    assert_eq!(events.subscription_count(), 1);
}

#[test]
fn a_shared_flow_without_replay_does_not_deliver_past_events() {
    let events = MutableSharedFlow::new(0, 0);
    events.emit(1);
    let mut run = Turbine::of(&events);
    assert_eq!(run.next_now(), Poll::Pending);
    events.emit(2);
    assert_eq!(run.next_now(), Poll::Ready(Some(2)));
}

#[test]
fn a_lagging_shared_collector_skips_values_that_fell_out_of_the_buffer() {
    let events = MutableSharedFlow::new(0, 2);
    let mut run = Turbine::of(&events);
    for value in 1..=5 {
        events.emit(value);
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
