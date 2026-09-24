use super::*;

#[derive(Debug, PartialEq, Eq)]
struct Failed(&'static str);

#[test]
fn a_signal_carries_one_value_to_whoever_waits() {
    let signal = Signal::new();
    signal.set(7u32);
    assert_eq!(pollster::block_on(signal.wait()), Some(7));
}

#[test]
fn a_closed_signal_resolves_to_nothing_rather_than_waiting_for_ever() {
    let signal = Signal::<u32>::new();
    signal.close();
    assert_eq!(pollster::block_on(signal.wait()), None);
}

#[test]
fn a_signal_set_from_another_thread_wakes_the_waiter() {
    let signal = Signal::new();
    let worker = signal.clone();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(20));
        worker.set(11u32);
    });
    assert_eq!(pollster::block_on(signal.wait()), Some(11));
    handle.join().expect("the worker finishes");
}

#[test]
fn chunks_arrive_in_the_order_they_were_produced() {
    let (channel, stream) = ChunkChannel::<Failed>::new();
    assert!(channel.push(b"one".to_vec()));
    assert!(channel.push(b"two".to_vec()));
    channel.finish();

    assert_eq!(pollster::block_on(stream.next()), Ok(Some(b"one".to_vec())));
    assert_eq!(pollster::block_on(stream.next()), Ok(Some(b"two".to_vec())));
    assert_eq!(pollster::block_on(stream.next()), Ok(None));
}

#[test]
fn a_failed_stream_reports_the_error_after_what_it_already_produced() {
    let (channel, stream) = ChunkChannel::new();
    assert!(channel.push(b"partial".to_vec()));
    channel.fail(Failed("the connection dropped"));

    assert_eq!(
        pollster::block_on(stream.next()),
        Ok(Some(b"partial".to_vec()))
    );
    assert_eq!(
        pollster::block_on(stream.next()),
        Err(Failed("the connection dropped"))
    );
}

#[test]
fn dropping_the_producer_ends_the_stream() {
    let (channel, stream) = ChunkChannel::<Failed>::new();
    drop(channel);
    assert_eq!(pollster::block_on(stream.next()), Ok(None));
}

#[test]
fn the_producer_waits_while_the_consumer_is_behind() {
    let (channel, stream) = ChunkChannel::<Failed>::new();
    let pushed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = Arc::clone(&pushed);
    let worker = std::thread::spawn(move || {
        for index in 0..MAX_PENDING_CHUNKS + 4 {
            if !channel.push(vec![index as u8]) {
                break;
            }
            counter.fetch_add(1, std::sync::atomic::Ordering::Release);
        }
        channel.finish();
    });

    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(
        pushed.load(std::sync::atomic::Ordering::Acquire) <= MAX_PENDING_CHUNKS,
        "the producer must stop at the bound rather than reading ahead without limit"
    );

    let mut received = 0usize;
    while let Ok(Some(_)) = pollster::block_on(stream.next()) {
        received += 1;
    }
    assert_eq!(received, MAX_PENDING_CHUNKS + 4);
    worker.join().expect("the worker finishes");
}

#[test]
fn abandoning_the_stream_stops_the_producer() {
    let (channel, stream) = ChunkChannel::<Failed>::new();
    assert!(channel.push(b"first".to_vec()));
    drop(stream);
    assert!(channel.is_abandoned());
    assert!(
        !channel.push(b"second".to_vec()),
        "a push after the consumer has gone must report that nobody is reading"
    );
}
