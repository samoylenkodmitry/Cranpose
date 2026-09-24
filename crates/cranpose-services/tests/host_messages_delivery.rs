use std::{cell::RefCell, rc::Rc};

use cranpose_core::{CollectEvents, Composition, MemoryApplier, location_key};
use cranpose_services::{
    HostMessage, clear_host_messages, publish_host_message, rememberHostMessages,
};

use crate::composition_support::{pump_until, serial};

fn render_collector(
    composition: &mut Composition<MemoryApplier>,
    channel: &'static str,
    seen: &Rc<RefCell<Vec<String>>>,
) {
    let key = location_key(file!(), line!(), column!());
    let sink = Rc::clone(seen);
    composition
        .render(key, move || {
            let messages = rememberHostMessages(channel);
            let sink = Rc::clone(&sink);
            CollectEvents(messages, (), move |payload: String| {
                sink.borrow_mut().push(payload);
            });
        })
        .expect("render succeeds");
}

#[test]
fn a_message_sent_before_the_screen_collects_is_replayed_to_it() {
    let _guard = serial();
    clear_host_messages();
    publish_host_message(HostMessage::new("delivery.theme", "dark"));

    let mut composition = Composition::new(MemoryApplier::new());
    let seen = Rc::new(RefCell::new(Vec::new()));
    render_collector(&mut composition, "delivery.theme", &seen);

    assert!(
        pump_until(&mut composition, || !seen.borrow().is_empty()),
        "the message sent before the first composition never reached the collector"
    );
    assert_eq!(*seen.borrow(), vec!["dark".to_string()]);
}

#[test]
fn a_message_sent_from_another_thread_reaches_the_live_composition() {
    let _guard = serial();
    clear_host_messages();

    let mut composition = Composition::new(MemoryApplier::new());
    let seen = Rc::new(RefCell::new(Vec::new()));
    render_collector(&mut composition, "delivery.editor", &seen);
    assert!(pump_until(&mut composition, || true));

    std::thread::spawn(|| {
        publish_host_message(HostMessage::new("delivery.other", "ignored"));
        publish_host_message(HostMessage::new("delivery.editor", "main.rs"));
    })
    .join()
    .expect("publisher thread finishes");

    assert!(
        pump_until(&mut composition, || !seen.borrow().is_empty()),
        "the message published into the live composition never reached the collector"
    );
    assert_eq!(*seen.borrow(), vec!["main.rs".to_string()]);
}

#[test]
fn cleared_messages_are_not_replayed() {
    let _guard = serial();
    publish_host_message(HostMessage::new("delivery.cleared", "stale"));
    clear_host_messages();

    let mut composition = Composition::new(MemoryApplier::new());
    let seen = Rc::new(RefCell::new(Vec::new()));
    render_collector(&mut composition, "delivery.cleared", &seen);
    composition.runtime_handle().drain_ui();

    assert!(seen.borrow().is_empty());
}
