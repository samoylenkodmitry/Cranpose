use std::sync::PoisonError;

use super::*;

fn recording_observer() -> (Observer, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);
    let observer: Observer = Arc::new(move |payload: String| {
        recorder
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(payload);
    });
    (observer, seen)
}

fn seen(log: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    log.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

#[test]
fn a_new_observer_is_handed_the_newest_message_of_its_channel() {
    let mut mailbox = Mailbox::new();
    assert!(
        mailbox
            .publish(&HostMessage::new("theme", "light"))
            .is_empty()
    );
    assert!(
        mailbox
            .publish(&HostMessage::new("theme", "dark"))
            .is_empty()
    );

    let (observer, _log) = recording_observer();
    assert_eq!(
        mailbox.observe(1, "theme", observer),
        Some("dark".to_string()),
        "only the newest message is replayed"
    );
}

#[test]
fn a_message_reaches_only_the_observers_of_its_channel() {
    let mut mailbox = Mailbox::new();
    let (theme, _theme_log) = recording_observer();
    let (editor, _editor_log) = recording_observer();
    assert_eq!(mailbox.observe(1, "theme", theme), None);
    assert_eq!(mailbox.observe(2, "editor", editor), None);

    assert_eq!(
        mailbox
            .publish(&HostMessage::new("editor", "main.rs"))
            .len(),
        1
    );
}

#[test]
fn a_removed_observer_receives_nothing() {
    let mut mailbox = Mailbox::new();
    let (observer, _log) = recording_observer();
    mailbox.observe(7, "theme", observer);
    mailbox.remove_observer(7);

    assert!(
        mailbox
            .publish(&HostMessage::new("theme", "dark"))
            .is_empty()
    );
}

#[test]
fn clearing_forgets_the_newest_messages() {
    let mut mailbox = Mailbox::new();
    mailbox.publish(&HostMessage::new("theme", "dark"));
    mailbox.clear();

    let (observer, _log) = recording_observer();
    assert_eq!(mailbox.observe(1, "theme", observer), None);
}

#[test]
fn published_messages_reach_a_registered_observer_until_it_is_dropped() {
    let channel = "host-messages-test.observe";
    let log = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&log);
    let registration = observe_host_messages(channel, move |payload| {
        recorder
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(payload);
    });

    publish_host_message(HostMessage::new(channel, "first"));
    drop(registration);
    publish_host_message(HostMessage::new(channel, "second"));

    assert_eq!(seen(&log), vec!["first".to_string()]);
}

#[test]
fn sending_goes_to_the_installed_outbox_and_reports_a_missing_host() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&sent);
    install_host_outbox(move |message| {
        recorder
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(message);
    });
    assert!(send_to_host("ide.notify", "saved"));

    clear_host_outbox();
    assert!(!send_to_host("ide.notify", "lost"));

    assert_eq!(
        *sent.lock().unwrap_or_else(PoisonError::into_inner),
        vec![HostMessage::new("ide.notify", "saved")]
    );
}
