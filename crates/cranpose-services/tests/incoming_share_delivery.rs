//! A share must reach the application's collector no matter when it arrives:
//! before the first composition, during one, or between two of them.
//!
//! These tests walk the same road the Android host walks:
//! `publish_incoming_content` from a platform thread, and an application
//! composition that collects `rememberIncomingContent()` with `CollectEvents`.
//! They live in one file on purpose: the inbox is process-global, and this
//! binary runs its tests one at a time via a shared lock so two tests never
//! interleave their publishes.

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};

use cranpose_core::{CollectEvents, Composition, MemoryApplier, location_key};
use cranpose_services::{
    IncomingContent, clear_incoming_content, publish_incoming_content, rememberIncomingContent,
};

fn serial() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

fn pump_until(composition: &mut Composition<MemoryApplier>, done: impl Fn() -> bool) -> bool {
    let runtime = composition.runtime_handle();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        runtime.drain_ui();
        if done() {
            return true;
        }
        std::thread::yield_now();
    }
    done()
}

fn item(name: &str) -> IncomingContent {
    IncomingContent::from_bytes(vec![1, 2, 3]).with_name(name)
}

fn render_collector(
    composition: &mut Composition<MemoryApplier>,
    key: cranpose_core::Key,
    seen: &Rc<RefCell<Vec<String>>>,
) {
    let sink = Rc::clone(seen);
    composition
        .render(key, move || {
            let shared = rememberIncomingContent();
            let sink = Rc::clone(&sink);
            CollectEvents(shared, (), move |item: IncomingContent| {
                sink.borrow_mut().push(item.display_name());
            });
        })
        .expect("render succeeds");
}

#[test]
fn a_share_published_before_the_first_composition_is_imported() {
    let _guard = serial();
    clear_incoming_content();

    publish_incoming_content(item("early.png"));

    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let seen = Rc::new(RefCell::new(Vec::new()));
    render_collector(&mut composition, key, &seen);

    assert!(
        pump_until(&mut composition, || !seen.borrow().is_empty()),
        "the share from before the first composition never reached the collector"
    );
    assert_eq!(*seen.borrow(), vec!["early.png".to_string()]);
}

#[test]
fn a_share_published_while_the_composition_lives_is_imported() {
    let _guard = serial();
    clear_incoming_content();

    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let seen = Rc::new(RefCell::new(Vec::new()));
    render_collector(&mut composition, key, &seen);
    assert!(pump_until(&mut composition, || true));

    let publisher = std::thread::spawn(|| publish_incoming_content(item("warm.png")));
    publisher.join().expect("publisher thread finishes");

    assert!(
        pump_until(&mut composition, || !seen.borrow().is_empty()),
        "the share published into the live composition never reached the collector"
    );
    assert_eq!(*seen.borrow(), vec!["warm.png".to_string()]);
}

#[test]
fn a_share_published_between_two_compositions_is_imported_by_the_second() {
    let _guard = serial();
    clear_incoming_content();

    let key = location_key(file!(), line!(), column!());
    {
        let mut first = Composition::new(MemoryApplier::new());
        let seen = Rc::new(RefCell::new(Vec::new()));
        render_collector(&mut first, key, &seen);
        assert!(pump_until(&mut first, || true));
    }

    publish_incoming_content(item("between.png"));

    let mut second = Composition::new(MemoryApplier::new());
    let seen = Rc::new(RefCell::new(Vec::new()));
    render_collector(&mut second, key, &seen);

    assert!(
        pump_until(&mut second, || !seen.borrow().is_empty()),
        "the share published between two compositions never reached the second collector"
    );
    assert_eq!(*seen.borrow(), vec!["between.png".to_string()]);
}

#[test]
fn a_share_published_during_first_render_before_effects_is_imported() {
    let _guard = serial();
    clear_incoming_content();

    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    composition
        .render(key, move || {
            let shared = rememberIncomingContent();
            publish_incoming_content(item("mid-render.png"));
            let sink = Rc::clone(&sink);
            CollectEvents(shared, (), move |item: IncomingContent| {
                sink.borrow_mut().push(item.display_name());
            });
        })
        .expect("render succeeds");

    assert!(
        pump_until(&mut composition, || !seen.borrow().is_empty()),
        "the share published during the first render never reached the collector"
    );
    assert_eq!(*seen.borrow(), vec!["mid-render.png".to_string()]);
}
