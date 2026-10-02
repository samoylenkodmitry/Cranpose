use std::{cell::Cell, rc::Rc};

use cranpose::prelude::*;
use cranpose_core::rememberMutableStateOf;
use cranpose_native::{NativeContent, NativeError, NativeSession, SendToHost, WebView};

fn component() -> std::sync::Arc<NativeSession> {
    NativeSession::new(|| {
        let control = Rc::new(Cell::new(None::<cranpose_core::MutableState<i32>>));
        let draw_control = Rc::clone(&control);
        NativeContent::new(move || {
            let count = rememberMutableStateOf(|| 0);
            draw_control.set(Some(count));
            SendToHost("count", count.value().to_string());
            WebView(
                "https://example.com",
                Modifier::empty().width(200.0).height(100.0),
                |_| {},
            );
        })
        .on_event(move |event| {
            if event.name == "increment" {
                let count = control.get().expect("composed count");
                count.set(count.value() + 1);
            }
        })
    })
}

#[test]
fn independent_sessions_exchange_events_and_preserve_native_identity() {
    let a = component();
    let b = component();
    let first = pollster::block_on(a.frame(400, 300, 2.0)).expect("first");
    let second = pollster::block_on(b.frame(400, 300, 2.0)).expect("second");
    assert_eq!(first.slots[0].value, "https://example.com");
    assert!(second.events.iter().any(|event| event.value == "0"));
    a.send_event("increment".into(), String::new())
        .expect("event");
    let changed = pollster::block_on(a.frame(400, 300, 2.0)).expect("changed");
    assert!(
        changed
            .events
            .iter()
            .any(|event| event.name == "count" && event.value == "1")
    );
    assert_eq!(changed.slots[0].id, first.slots[0].id);
    assert_eq!(changed.slots[0].value, first.slots[0].value);
    let unchanged = pollster::block_on(b.frame(400, 300, 2.0)).expect("isolated");
    assert!(!unchanged.events.iter().any(|event| event.value == "1"));
    a.shutdown();
    assert!(matches!(
        pollster::block_on(a.frame(400, 300, 2.0)),
        Err(NativeError::Closed)
    ));
    assert!(a.send_event("increment".into(), String::new()).is_err());
    b.shutdown();
}

#[test]
fn hidden_session_resumes_and_invalid_size_does_not_poison_it() {
    let session = component();
    pollster::block_on(session.frame(400, 300, 2.0)).expect("initial");
    session.set_visible(false).expect("hide");
    session
        .send_event("increment".into(), String::new())
        .expect("event while hidden");
    assert!(
        pollster::block_on(session.frame(400, 300, 2.0))
            .expect("hidden")
            .pixels
            .is_empty()
    );
    session.set_visible(true).expect("show");
    let resumed = pollster::block_on(session.frame(600, 400, 2.0)).expect("resumed");
    assert_eq!(resumed.pixels.len(), 600 * 400 * 4);
    assert!(resumed.events.iter().any(|event| event.value == "1"));
    assert!(pollster::block_on(session.frame(0, 1, 1.0)).is_err());
    pollster::block_on(session.frame(400, 300, 2.0)).expect("recover");
    session.shutdown();
    session.shutdown();
}
