use std::sync::PoisonError;

use super::*;

fn recording_observer() -> (Observer, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);
    let observer: Observer = Arc::new(move |item: IncomingContent| {
        recorder
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(item.display_name());
    });
    (observer, seen)
}

#[test]
fn an_item_published_before_anyone_listens_is_backlogged_once() {
    let mut inbox = Inbox::new();

    assert!(
        inbox
            .publish(IncomingContent::from_bytes(vec![1, 2, 3]).with_name("scan.jpg"))
            .is_none(),
        "with nobody listening the item belongs in the backlog, not delivered"
    );

    let (first, _first_seen) = recording_observer();
    assert_eq!(
        inbox.observe(1, first).len(),
        1,
        "the first observer to register receives the backlog"
    );

    let (second, _second_seen) = recording_observer();
    assert!(
        inbox.observe(2, second).is_empty(),
        "the backlog drains into that observer and is not replayed to the next"
    );
}

#[test]
fn the_backlog_reaches_the_first_observer_that_registers() {
    let mut inbox = Inbox::new();
    inbox.publish(IncomingContent::from_bytes(vec![1, 2, 3]).with_name("scan.jpg"));

    let (observer, seen) = recording_observer();
    let replay = inbox.observe(1, Arc::clone(&observer));
    for item in replay {
        observer(item);
    }

    assert_eq!(
        seen.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_slice(),
        ["scan.jpg"]
    );
}

#[test]
fn observers_stop_receiving_once_dropped() {
    let mut inbox = Inbox::new();
    let (observer, _seen) = recording_observer();
    inbox.observe(7, observer);

    let delivered = inbox
        .publish(IncomingContent::from_bytes(vec![1]))
        .expect("a registered observer receives the item");
    assert_eq!(delivered.len(), 1);

    inbox.remove_observer(7);
    assert!(
        inbox
            .publish(IncomingContent::from_bytes(vec![2]))
            .is_none(),
        "once the last observer is gone the item goes back to the backlog"
    );
}

#[test]
fn clearing_discards_the_backlog() {
    let mut inbox = Inbox::new();
    inbox.publish(IncomingContent::from_bytes(vec![1]));
    inbox.clear();

    let (observer, _seen) = recording_observer();
    assert!(
        inbox.observe(1, observer).is_empty(),
        "a cleared backlog has nothing left to replay"
    );
}

#[test]
fn bytes_become_readable_content_with_the_reported_name() {
    let item = IncomingContent::from_bytes(b"payload".to_vec())
        .with_name("note.txt")
        .with_mime_type("text/plain");
    let content = item.content().expect("bytes always resolve");
    assert_eq!(content.metadata().name, "note.txt");
    assert_eq!(content.metadata().mime_type.as_deref(), Some("text/plain"));
    assert_eq!(
        pollster::block_on(content.read_all()).expect("the bytes read back"),
        b"payload"
    );
}

#[test]
fn a_uri_item_names_itself_from_its_last_segment() {
    let item = IncomingContent::from_uri("content://media/external/images/42");
    assert_eq!(item.display_name(), "42");
}
