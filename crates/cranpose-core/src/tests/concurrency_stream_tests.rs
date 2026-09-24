use super::*;

#[test]
fn a_channel_wakes_its_collector_and_ends_when_closed() {
    let channel: EventChannel<u32> = EventChannel::new();
    let stream = channel.stream();

    let mut pending = Box::pin(stream.next());
    let waker = Waker::noop().clone();
    let mut context = Context::from_waker(&waker);
    assert!(pending.as_mut().poll(&mut context).is_pending());

    channel.send(7);
    assert_eq!(pending.as_mut().poll(&mut context), Poll::Ready(Some(7)));

    channel.send(8);
    channel.close();
    assert_eq!(pollster::block_on(stream.next()), Some(8));
    assert_eq!(pollster::block_on(stream.next()), None);
    assert_eq!(stream.delivered(), 2);
}

#[test]
fn an_event_goes_to_exactly_one_collector() {
    let channel: EventChannel<u32> = EventChannel::new();
    let first = channel.stream();
    let second = first.clone();
    channel.send(1);
    channel.close();
    assert_eq!(pollster::block_on(first.next()), Some(1));
    assert_eq!(pollster::block_on(second.next()), None);
}

#[test]
fn sending_after_close_is_ignored() {
    let channel: EventChannel<u32> = EventChannel::new();
    let stream = channel.stream();
    channel.close();
    channel.send(1);
    assert_eq!(pollster::block_on(stream.next()), None);
    assert_eq!(channel.pending(), 0);
}

#[test]
fn blocking_work_resolves_with_its_result() {
    let doubled = pollster::block_on(withBlocking(|| 21 * 2));
    assert_eq!(doubled, 42);
}
