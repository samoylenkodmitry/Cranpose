use std::{
    sync::{Arc, Mutex, PoisonError},
    task::Poll,
    time::Duration,
};

use coroflow::{
    Capacity, CoroutineScope, FlowExt, SendError, TestScheduler, TryRecvError, TrySendError,
    callback_flow, channel, channel_flow, delay, produce,
};

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[test]
fn a_buffered_channel_makes_senders_wait_only_when_full() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let (sender, receiver) = channel(Capacity::Buffered(2));
    let sent = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&sent);
    scope.launch(async move {
        for value in 1..=3 {
            if sender.send(value).await.is_ok() {
                locked(&log).push(value);
            }
        }
    });
    scheduler.run_current();
    assert_eq!(*locked(&sent), vec![1, 2], "the third send waits for room");
    assert_eq!(receiver.try_recv(), Ok(1));
    scheduler.run_current();
    assert_eq!(*locked(&sent), vec![1, 2, 3]);
    assert_eq!(receiver.try_recv(), Ok(2));
    assert_eq!(receiver.try_recv(), Ok(3));
    assert_eq!(
        receiver.try_recv(),
        Err(TryRecvError::Closed),
        "the sender is gone"
    );
}

#[test]
fn a_rendezvous_send_completes_only_when_a_receiver_takes_the_value() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let (sender, receiver) = channel(Capacity::Rendezvous);
    let delivered = Arc::new(Mutex::new(false));
    let flag = Arc::clone(&delivered);
    scope.launch(async move {
        if sender.send("hello").await.is_ok() {
            *locked(&flag) = true;
        }
    });
    scheduler.advance_time_by(Duration::from_millis(100));
    assert!(!*locked(&delivered), "nobody received yet");
    assert_eq!(receiver.try_recv(), Ok("hello"));
    scheduler.run_current();
    assert!(*locked(&delivered));
}

#[test]
fn a_conflated_channel_keeps_only_the_newest_value() {
    let (sender, receiver) = channel(Capacity::Conflated);
    for value in 1..=3 {
        assert_eq!(sender.try_send(value), Ok(()));
    }
    assert_eq!(receiver.try_recv(), Ok(3));
    assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));
    let (unlimited, drain) = channel(Capacity::Unlimited);
    for value in 0..1_000 {
        assert_eq!(unlimited.try_send(value), Ok(()));
    }
    drop(unlimited);
    assert_eq!(pollster::block_on(drain.to_vec()).len(), 1_000);
}

#[test]
fn closing_lets_receivers_drain_and_then_rejects_values() {
    let (sender, receiver) = channel(Capacity::BUFFERED);
    assert_eq!(sender.try_send(1), Ok(()));
    sender.close();
    assert!(sender.is_closed());
    assert_eq!(pollster::block_on(sender.send(2)), Err(SendError(2)));
    assert_eq!(sender.try_send(3), Err(TrySendError::Closed(3)));
    assert_eq!(pollster::block_on(receiver.recv()), Some(1));
    assert_eq!(pollster::block_on(receiver.recv()), None);
    let (orphan, gone) = channel::<u32>(Capacity::Rendezvous);
    assert_eq!(orphan.try_send(1), Err(TrySendError::Full(1)));
    drop(gone);
    assert_eq!(pollster::block_on(orphan.send(1)), Err(SendError(1)));
}

#[test]
fn each_value_goes_to_exactly_one_receiver() {
    let (sender, first) = channel(Capacity::Unlimited);
    let second = first.clone();
    for value in 0..10 {
        assert_eq!(sender.try_send(value), Ok(()));
    }
    drop(sender);
    let mut all: Vec<u32> = Vec::new();
    loop {
        match (first.try_recv(), second.try_recv()) {
            (Err(_), Err(_)) => break,
            (a, b) => all.extend(a.into_iter().chain(b)),
        }
    }
    all.sort_unstable();
    assert_eq!(all, (0..10).collect::<Vec<_>>());
}

#[test]
fn produce_streams_a_coroutines_values_into_a_flow() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let numbers = produce(&scope, Capacity::Rendezvous, |sender| async move {
        for value in 1..=3 {
            delay(Duration::from_millis(10)).await;
            if sender.send(value).await.is_err() {
                return;
            }
        }
    });
    assert_eq!(scheduler.block_on(numbers.to_vec()), Ok(vec![1, 2, 3]));
    assert_eq!(scheduler.now(), Duration::from_millis(30));
}

#[test]
fn channel_flow_lets_concurrent_producers_send_and_reruns_per_collection() {
    let scheduler = TestScheduler::new();
    let merged = channel_flow(|producer| async move {
        for offset in [0, 100] {
            let producer = producer.clone();
            producer.clone().launch(async move {
                for step in 1..=2 {
                    delay(Duration::from_millis(step * 10 + offset / 10)).await;
                    let _ = producer.send(offset + step).await;
                }
            });
        }
    });
    let mut first = scheduler.block_on(merged.to_vec()).unwrap_or_default();
    first.sort_unstable();
    assert_eq!(first, vec![1, 2, 101, 102]);
    assert_eq!(
        scheduler
            .block_on(merged.to_vec())
            .map(|values| values.len()),
        Ok(4)
    );
}

#[test]
fn callback_flow_unregisters_its_callback_when_the_collector_stops() {
    type Callback = Box<dyn Fn(u32) + Send>;
    let scheduler = TestScheduler::new();
    let callbacks: Arc<Mutex<Vec<Callback>>> = Arc::default();
    let registry = Arc::clone(&callbacks);
    let ticks = callback_flow(move |producer| {
        let registry = Arc::clone(&registry);
        async move {
            let sender = producer.sender();
            locked(&registry).push(Box::new(move |tick| {
                let _ = sender.try_send(tick);
            }));
            let cleanup = Arc::clone(&registry);
            producer.await_close(move || locked(&cleanup).clear()).await;
        }
    });
    let mut run = scheduler.turbine(&ticks);
    assert_eq!(run.next_now(), Poll::Pending);
    scheduler.run_current();
    assert_eq!(locked(&callbacks).len(), 1, "registered on collection");
    for callback in locked(&callbacks).iter() {
        callback(7);
    }
    scheduler.run_current();
    assert_eq!(run.next_now(), Poll::Ready(Some(7)));
    drop(run);
    scheduler.run_current();
    assert!(
        locked(&callbacks).is_empty(),
        "unregistered on cancellation"
    );
}
