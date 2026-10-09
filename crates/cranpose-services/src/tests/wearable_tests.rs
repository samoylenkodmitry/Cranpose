use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

use parking_lot::Mutex;

use super::*;

#[derive(Default)]
struct Recorded {
    sent: Mutex<Vec<(String, String, Vec<u8>)>>,
}

struct FakeLink(Arc<Recorded>);

impl WearableLink for FakeLink {
    fn peers(&self) -> Result<Vec<WearablePeer>, WearableError> {
        Ok(vec![WearablePeer {
            id: "watch".into(),
            name: "Pixel Watch".into(),
        }])
    }

    fn send_message(&self, peer: &str, path: &str, data: &[u8]) -> Result<(), WearableError> {
        self.0
            .sent
            .lock()
            .push((peer.into(), path.into(), data.into()));
        Ok(())
    }

    fn open_stream(&self, peer: &str, path: &str) -> Result<Box<dyn Write + Send>, WearableError> {
        if peer == "gone" {
            return Err(WearableError::Failed(format!(
                "{peer} is out of range for {path}"
            )));
        }
        Ok(Box::new(Vec::new()))
    }
}

#[test]
fn every_call_is_unavailable_without_a_platform_link() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_wearable_link();
    assert!(matches!(wearable_peers(), Err(WearableError::Unavailable)));
    assert!(matches!(
        send_wearable_message("watch", "/line", b"hi"),
        Err(WearableError::Unavailable)
    ));
    assert!(matches!(
        open_wearable_stream("watch", "/audio"),
        Err(WearableError::Unavailable)
    ));
}

#[test]
fn calls_reach_the_installed_platform_link() {
    let _guard = crate::registry::test_service_guard();
    let recorded = Arc::new(Recorded::default());
    set_platform_wearable_link(Arc::new(FakeLink(Arc::clone(&recorded))));

    let peers = wearable_peers().map_err(|error| error.to_string());
    assert_eq!(
        peers,
        Ok(vec![WearablePeer {
            id: "watch".into(),
            name: "Pixel Watch".into()
        }])
    );
    assert!(send_wearable_message("watch", "/line", b"hello").is_ok());
    assert_eq!(
        *recorded.sent.lock(),
        vec![("watch".to_owned(), "/line".to_owned(), b"hello".to_vec())]
    );
    assert!(open_wearable_stream("watch", "/audio").is_ok());
    assert!(matches!(
        open_wearable_stream("gone", "/audio"),
        Err(WearableError::Failed(_))
    ));
    clear_platform_wearable_link();
}

#[test]
fn events_reach_the_receiver_and_streams_read_to_their_end() {
    let _guard = crate::registry::test_service_guard();
    let received = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&received);
    set_wearable_receiver(Arc::new(move |event: WearableEvent| {
        let entry = match event {
            WearableEvent::Message { peer, path, data } => (peer, path, data),
            WearableEvent::Stream(mut stream) => {
                let mut bytes = Vec::new();
                let read = stream.read_to_end(&mut bytes).map(|_| bytes);
                (stream.peer, stream.path, read.unwrap_or_default())
            }
        };
        sink.lock().push(entry);
    }));

    publish_wearable_event(WearableEvent::Message {
        peer: "phone".into(),
        path: "/line".into(),
        data: b"Hello".to_vec(),
    });
    let audio: Box<dyn Read + Send> = Box::new(Cursor::new(vec![1, 2, 3]));
    publish_wearable_event(WearableEvent::Stream(WearableStream::new(
        "watch".into(),
        "/audio".into(),
        audio,
    )));

    assert_eq!(
        *received.lock(),
        vec![
            ("phone".to_owned(), "/line".to_owned(), b"Hello".to_vec()),
            ("watch".to_owned(), "/audio".to_owned(), vec![1, 2, 3]),
        ]
    );
    clear_wearable_receiver();
}

#[test]
fn events_without_a_receiver_are_dropped() {
    let _guard = crate::registry::test_service_guard();
    clear_wearable_receiver();
    publish_wearable_event(WearableEvent::Message {
        peer: "phone".into(),
        path: "/line".into(),
        data: Vec::new(),
    });
}
