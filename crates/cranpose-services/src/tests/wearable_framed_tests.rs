use std::{
    io::{Read, Write},
    sync::Arc,
};

use parking_lot::Mutex;

use super::*;
use crate::wearable::{
    open_wearable_stream, send_wearable_message, set_platform_wearable_link, set_wearable_receiver,
};

/// A frame the transport sent.
struct Sent {
    peer: String,
    frame: Vec<u8>,
    confirm: bool,
}

/// A transport that keeps what it sends, for the test to deliver.
#[derive(Clone, Default)]
struct Outbox(Arc<Mutex<Vec<Sent>>>);

impl Outbox {
    /// Hands every frame sent so far to `link`, as from `peer`.
    fn deliver(&self, link: &FramedWearableLink<Outbox>, peer: &str) {
        for sent in self.0.lock().drain(..) {
            link.receive(peer, &sent.frame);
        }
    }
}

impl WearableTransport for Outbox {
    fn peers(&self) -> Result<Vec<WearablePeer>, WearableError> {
        Ok(Vec::new())
    }

    fn send(&self, peer: &str, frame: &[u8], confirm: bool) -> Result<(), WearableError> {
        self.0.lock().push(Sent {
            peer: peer.to_owned(),
            frame: frame.to_vec(),
            confirm,
        });
        Ok(())
    }

    fn open_url(&self, _peer: &str, _url: &str) -> Result<(), WearableError> {
        Err(WearableError::Unavailable)
    }
}

/// Installs a link on this side and returns what it sends, the other side's
/// link and what that side's application receives.
fn linked() -> (
    Outbox,
    FramedWearableLink<Outbox>,
    Arc<Mutex<Vec<WearableEvent>>>,
) {
    let outbox = Outbox::default();
    set_platform_wearable_link(Some(Arc::new(FramedWearableLink::new(outbox.clone()))));
    let received = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::clone(&received);
    set_wearable_receiver(Some(Arc::new(move |event| events.lock().push(event))));
    (outbox, FramedWearableLink::new(Outbox::default()), received)
}

#[test]
fn a_message_arrives_with_its_path_and_data() {
    let _guard = crate::registry::test_service_guard();
    let (outbox, phone, received) = linked();

    send_wearable_message("phone", "/cranslate/hello", b"\x00\x01text").expect("sent");
    assert!(
        outbox
            .0
            .lock()
            .iter()
            .all(|sent| sent.peer == "phone" && sent.confirm)
    );
    outbox.deliver(&phone, "watch");

    let received = received.lock();
    let [WearableEvent::Message { peer, path, data }] = received.as_slice() else {
        panic!("one message arrives");
    };
    assert_eq!(
        (peer.as_str(), path.as_str()),
        ("watch", "/cranslate/hello")
    );
    assert_eq!(data, b"\x00\x01text");
}

#[test]
fn a_stream_arrives_whole_in_few_frames_and_ends() {
    let _guard = crate::registry::test_service_guard();
    let (outbox, phone, received) = linked();
    let sent: Vec<u8> = (0..2 * CHUNK_BYTES as u32 + 1000)
        .map(|index| (index % 251) as u8)
        .collect();

    let mut stream = open_wearable_stream("phone", "/cranslate/audio").expect("opened");
    for piece in sent.chunks(320) {
        stream.write_all(piece).expect("written");
    }
    drop(stream);
    // The opening, three frames of bytes and the end.
    assert_eq!(outbox.0.lock().len(), 5);
    assert!(outbox.0.lock().iter().skip(1).all(|sent| !sent.confirm));
    outbox.deliver(&phone, "watch");

    let Some(WearableEvent::Stream(mut stream)) = received.lock().pop() else {
        panic!("a stream arrives");
    };
    assert_eq!(
        (stream.peer.as_str(), stream.path.as_str()),
        ("watch", "/cranslate/audio")
    );
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).expect("read to the end");
    assert_eq!(bytes, sent);
}

#[test]
fn flush_sends_what_the_writer_holds() {
    let _guard = crate::registry::test_service_guard();
    let (outbox, phone, received) = linked();

    let mut stream = open_wearable_stream("phone", "/audio").expect("opened");
    stream.write_all(b"first words").expect("written");
    stream.flush().expect("flushed");
    outbox.deliver(&phone, "watch");

    let Some(WearableEvent::Stream(mut incoming)) = received.lock().pop() else {
        panic!("a stream arrives");
    };
    let mut bytes = [0; 11];
    incoming
        .read_exact(&mut bytes)
        .expect("the flushed bytes arrive");
    assert_eq!(&bytes, b"first words");
    drop(stream);
}

#[test]
fn losing_the_device_ends_its_streams() {
    let _guard = crate::registry::test_service_guard();
    let (outbox, phone, received) = linked();

    let mut stream = open_wearable_stream("phone", "/audio").expect("opened");
    stream.write_all(b"cut off").expect("written");
    stream.flush().expect("flushed");
    outbox.deliver(&phone, "watch");
    phone.disconnect("watch");

    let Some(WearableEvent::Stream(mut incoming)) = received.lock().pop() else {
        panic!("a stream arrives");
    };
    let mut bytes = Vec::new();
    incoming.read_to_end(&mut bytes).expect("the stream ends");
    assert_eq!(bytes, b"cut off");
    drop(stream);
}

#[test]
fn a_damaged_frame_is_dropped() {
    let _guard = crate::registry::test_service_guard();
    let (_outbox, phone, received) = linked();

    for frame in [
        &[][..],
        &[MESSAGE, 9, 0, b'/'],
        &[CHUNK, 1],
        &[OPEN, 0, 0, 0, 0, 0xff],
        &[7],
    ] {
        phone.receive("watch", frame);
    }
    phone.receive("watch", &[CHUNK, 5, 0, 0, 0, 1, 2]);
    assert!(received.lock().is_empty());
}
