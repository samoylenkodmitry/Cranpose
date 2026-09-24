use std::sync::{Arc, PoisonError};

use super::*;

fn navigation_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

#[test]
fn requests_accumulate_and_drain() {
    let _guard = navigation_lock();
    let _ = take_back_requests();
    push_back_request();
    push_back_request();
    assert_eq!(take_back_requests(), 2);
    assert_eq!(take_back_requests(), 0);
}

#[test]
fn a_registered_listener_hears_every_request() {
    let _guard = navigation_lock();
    let heard = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&heard);
    let _observer = observe_back_requests(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    let before = heard.load(Ordering::SeqCst);
    push_back_request();
    push_back_request();
    assert_eq!(heard.load(Ordering::SeqCst), before + 2);
    let _ = take_back_requests();

    let before = heard.load(Ordering::SeqCst);
    request_exit();
    assert_eq!(
        heard.load(Ordering::SeqCst),
        before + 1,
        "an exit request has to wake an idle app the way a back request does"
    );
    let _ = take_exit_request();
}

#[test]
fn the_latest_back_observer_wins_until_it_is_dropped() {
    let _guard = navigation_lock();
    let first = Arc::new(AtomicUsize::new(0));
    let second = Arc::new(AtomicUsize::new(0));
    let first_seen = Arc::clone(&first);
    let first_observer = observe_back_requests(move || {
        first_seen.fetch_add(1, Ordering::SeqCst);
    });
    let second_seen = Arc::clone(&second);
    let second_observer = observe_back_requests(move || {
        second_seen.fetch_add(1, Ordering::SeqCst);
    });
    push_back_request();
    assert_eq!(first.load(Ordering::SeqCst), 0);
    assert_eq!(second.load(Ordering::SeqCst), 1);
    drop(second_observer);
    push_back_request();
    assert_eq!(first.load(Ordering::SeqCst), 1);
    drop(first_observer);
    let _ = take_back_requests();
}

#[test]
fn an_exit_request_is_taken_once() {
    let _guard = navigation_lock();
    let _ = take_exit_request();
    assert!(!take_exit_request());
    request_exit();
    request_exit();
    assert!(take_exit_request());
    assert!(
        !take_exit_request(),
        "a drained request came back; the platform would close twice"
    );
}

#[test]
fn a_backend_can_look_at_the_request_without_consuming_it() {
    let _guard = navigation_lock();
    let _ = take_exit_request();
    assert!(!exit_requested());

    request_exit();
    assert!(exit_requested());
    assert!(
        exit_requested(),
        "looking at the request consumed it, which is the bug"
    );

    assert!(take_exit_request());
    assert!(!exit_requested());
}

#[test]
fn interception_defaults_off_and_toggles() {
    let _guard = navigation_lock();
    set_back_interception(false);
    assert!(!back_interception_enabled());
    set_back_interception(true);
    assert!(back_interception_enabled());
    set_back_interception(false);
}
