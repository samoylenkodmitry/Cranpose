//! Derived state, produced state, event collection and the frame clock.
//!
//! These are the pieces an application reaches for when something outside the
//! composition has to reach in: a computation over other state, a producer, a
//! stream, a frame. Each one is only correct if it is scoped to the composition
//! that created it, so each test here composes, drives, and then asks what
//! survived.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use cranpose_core::{
    composer_context::current_composer, derivedStateOf, launchBlocking, location_key,
    mutableStateOf, remember, rememberCoroutineScope, CollectEvents, Composition, EventChannel,
    MemoryApplier,
};

fn composition() -> Composition<MemoryApplier> {
    Composition::new(MemoryApplier::new())
}

/// Pumps the runtime until `done` answers true, or the deadline passes.
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

#[test]
fn derived_state_recomputes_from_the_state_it_reads() {
    let mut composition = composition();
    let key = location_key(file!(), line!(), column!());
    let source = mutableStateOf(2u32);
    let seen = Rc::new(RefCell::new(Vec::new()));

    let render = |composition: &mut Composition<MemoryApplier>| {
        let sink = Rc::clone(&seen);
        composition
            .render(key, move || {
                let doubled = derivedStateOf(move || source.get() * 2);
                sink.borrow_mut().push(doubled.get());
            })
            .expect("render succeeds");
    };

    render(&mut composition);
    source.set(5);
    render(&mut composition);

    assert_eq!(
        *seen.borrow(),
        vec![4, 10],
        "the derived value did not follow its source"
    );
}

#[test]
fn a_coroutine_scope_launches_work_onto_the_runtime_and_survives_recomposition() {
    let mut composition = composition();
    let key = location_key(file!(), line!(), column!());
    let ran = Rc::new(Cell::new(0usize));

    for _ in 0..2 {
        let flag = Rc::clone(&ran);
        composition
            .render(key, move || {
                let scope = rememberCoroutineScope();
                let flag = Rc::clone(&flag);
                scope.launch(async move { flag.set(flag.get() + 1) });
            })
            .expect("render succeeds");
    }

    // A scope torn down by the second pass would cancel the first pass's task
    // before it ever ran, and only one of the two would land.
    assert!(
        pump_until(&mut composition, || ran.get() == 2),
        "the scope delivered {} of two launches",
        ran.get()
    );
}

#[test]
fn collect_events_delivers_every_event_the_stream_carries() {
    let mut composition = composition();
    let key = location_key(file!(), line!(), column!());
    let channel: EventChannel<u32> = EventChannel::new();
    let collected = Rc::new(RefCell::new(Vec::new()));

    let stream = channel.stream();
    let sink = Rc::clone(&collected);
    composition
        .render(key, move || {
            let stream = stream.clone();
            let sink = Rc::clone(&sink);
            CollectEvents(stream, (), move |event| sink.borrow_mut().push(event));
        })
        .expect("render succeeds");

    channel.send(1);
    channel.send(2);
    channel.send(3);

    assert!(
        pump_until(&mut composition, || collected.borrow().len() == 3),
        "the collector saw {:?} rather than three events",
        collected.borrow()
    );
    assert_eq!(*collected.borrow(), vec![1, 2, 3]);
}

#[test]
fn blocking_work_runs_off_the_ui_thread_and_reports_back_on_it() {
    let mut composition = composition();
    let key = location_key(file!(), line!(), column!());
    let worker_thread = Arc::new(std::sync::Mutex::new(None));
    let reported = Rc::new(Cell::new(0u32));

    let thread_sink = Arc::clone(&worker_thread);
    let ui_sink = Rc::clone(&reported);
    let ui_thread = std::thread::current().id();
    composition
        .render(key, move || {
            let thread_sink = Arc::clone(&thread_sink);
            let ui_sink = Rc::clone(&ui_sink);
            remember(move || {
                launchBlocking(
                    move || {
                        *thread_sink.lock().expect("worker thread id") =
                            Some(std::thread::current().id());
                        21u32 * 2
                    },
                    move |answer| ui_sink.set(answer),
                );
            })
            .with(|_| ());
        })
        .expect("render succeeds");

    assert!(
        pump_until(&mut composition, || reported.get() != 0),
        "the blocking work never reported back"
    );
    assert_eq!(reported.get(), 42);
    assert_ne!(
        worker_thread
            .lock()
            .expect("worker thread id")
            .expect("ran"),
        ui_thread,
        "blocking work ran on the thread it was supposed to keep free"
    );
}

#[test]
fn the_current_composer_is_only_reachable_from_inside_a_composition() {
    assert!(
        current_composer().is_none(),
        "a composer was handed out with no composition running"
    );

    let mut composition = composition();
    let inside = Rc::new(Cell::new(false));
    let flag = Rc::clone(&inside);
    composition
        .render(location_key(file!(), line!(), column!()), move || {
            flag.set(current_composer().is_some());
        })
        .expect("render succeeds");
    assert!(inside.get(), "no composer was reachable while composing");

    assert!(
        current_composer().is_none(),
        "the composer outlived the render that installed it"
    );
}

#[test]
fn produced_state_starts_at_its_initial_value_and_takes_what_the_producer_publishes() {
    let mut composition = composition();
    let key = location_key(file!(), line!(), column!());
    let seen = Rc::new(RefCell::new(Vec::new()));

    let render = |composition: &mut Composition<MemoryApplier>| {
        let sink = Rc::clone(&seen);
        composition
            .render(key, move || {
                let sink = Rc::clone(&sink);
                let value = cranpose_core::produceState(0u32, (), |scope| {
                    Box::pin(async move {
                        scope.set(1);
                        scope.set(2);
                    })
                });
                sink.borrow_mut().push(value.get());
            })
            .expect("render succeeds");
    };

    render(&mut composition);
    // The first read happens before the producer has run: a produced state that
    // skipped its initial value would leave the first frame blank.
    assert_eq!(*seen.borrow(), vec![0]);

    let runtime = composition.runtime_handle();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && seen.borrow().last() != Some(&2) {
        runtime.drain_ui();
        render(&mut composition);
    }
    assert_eq!(
        seen.borrow().last(),
        Some(&2),
        "the producer's last value never reached the state: {:?}",
        seen.borrow()
    );
}
