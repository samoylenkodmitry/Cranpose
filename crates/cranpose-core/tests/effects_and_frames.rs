use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use cranpose_core::{
    CollectEvents, Composition, EventChannel, MemoryApplier, composer_context::current_composer,
    derivedStateOf, launchBlocking, location_key, mutableStateOf, remember, rememberCoroutineScope,
    rememberEventStream,
};

fn composition() -> Composition<MemoryApplier> {
    Composition::new(MemoryApplier::new())
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

type PlatformObserver = Arc<dyn Fn(u32) + Send + Sync>;

#[derive(Clone, Default)]
struct PlatformService {
    observers: Arc<Mutex<Vec<(u64, PlatformObserver)>>>,
    next_id: Arc<AtomicU64>,
}

struct PlatformRegistration {
    service: PlatformService,
    id: u64,
}

impl Drop for PlatformRegistration {
    fn drop(&mut self) {
        self.service
            .observers
            .lock()
            .expect("observer registry")
            .retain(|(id, _)| *id != self.id);
    }
}

impl PlatformService {
    fn observe(&self, observer: impl Fn(u32) + Send + Sync + 'static) -> PlatformRegistration {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.observers
            .lock()
            .expect("observer registry")
            .push((id, Arc::new(observer)));
        PlatformRegistration {
            service: self.clone(),
            id,
        }
    }

    fn publish(&self, event: u32) {
        let observers: Vec<_> = self
            .observers
            .lock()
            .expect("observer registry")
            .iter()
            .map(|(_, observer)| Arc::clone(observer))
            .collect();
        for observer in observers {
            observer(event);
        }
    }

    fn observer_count(&self) -> usize {
        self.observers.lock().expect("observer registry").len()
    }
}

#[test]
fn two_event_streams_in_one_composable_each_deliver_exactly_once() {
    let mut composition = composition();
    let key = location_key(file!(), line!(), column!());
    let shares = PlatformService::default();
    let pressure = PlatformService::default();
    let tick = mutableStateOf(0u32);
    let seen_shares = Rc::new(RefCell::new(Vec::new()));
    let seen_pressure = Rc::new(RefCell::new(Vec::new()));

    let render = |composition: &mut Composition<MemoryApplier>| {
        let shares = shares.clone();
        let pressure = pressure.clone();
        let share_sink = Rc::clone(&seen_shares);
        let pressure_sink = Rc::clone(&seen_pressure);
        composition
            .render(key, move || {
                let _ = tick.get();
                let shares = shares.clone();
                let pressure = pressure.clone();
                let share_sink = Rc::clone(&share_sink);
                let pressure_sink = Rc::clone(&pressure_sink);
                let share_stream =
                    rememberEventStream((), move |sender| shares.observe(move |e| sender.send(e)));
                CollectEvents(share_stream, (), move |event| {
                    share_sink.borrow_mut().push(event)
                });
                let pressure_stream = rememberEventStream((), move |sender| {
                    pressure.observe(move |e| sender.send(e))
                });
                CollectEvents(pressure_stream, (), move |event| {
                    pressure_sink.borrow_mut().push(event)
                });
            })
            .expect("render succeeds");
    };

    render(&mut composition);
    assert_eq!(shares.observer_count(), 1, "one subscription per stream");
    assert_eq!(pressure.observer_count(), 1, "one subscription per stream");

    pressure.publish(10);
    assert!(
        pump_until(&mut composition, || !seen_pressure.borrow().is_empty()),
        "the pressure event never arrived"
    );
    assert_eq!(
        *seen_pressure.borrow(),
        vec![10],
        "one publish must deliver exactly once"
    );
    assert!(
        seen_shares.borrow().is_empty(),
        "the share collector saw another service's events: {:?}",
        seen_shares.borrow()
    );

    tick.set(1);
    render(&mut composition);
    assert_eq!(
        shares.observer_count(),
        1,
        "recomposition duplicated the share subscription"
    );
    assert_eq!(
        pressure.observer_count(),
        1,
        "recomposition duplicated the pressure subscription"
    );

    pressure.publish(20);
    shares.publish(7);
    assert!(
        pump_until(&mut composition, || seen_pressure.borrow().len() >= 2
            && !seen_shares.borrow().is_empty()),
        "events stopped arriving after recomposition: pressure {:?}, shares {:?}",
        seen_pressure.borrow(),
        seen_shares.borrow()
    );
    assert_eq!(
        *seen_pressure.borrow(),
        vec![10, 20],
        "one publish must deliver exactly once after recomposition"
    );
    assert_eq!(
        *seen_shares.borrow(),
        vec![7],
        "one publish must deliver exactly once after recomposition"
    );
}

#[test]
fn a_stream_mounting_later_does_not_steal_an_existing_streams_identity() {
    let mut composition = composition();
    let key = location_key(file!(), line!(), column!());
    let banner = PlatformService::default();
    let shares = PlatformService::default();
    let pressure = PlatformService::default();
    let show_banner = mutableStateOf(false);
    let seen_banner = Rc::new(RefCell::new(Vec::new()));
    let seen_shares = Rc::new(RefCell::new(Vec::new()));
    let seen_pressure = Rc::new(RefCell::new(Vec::new()));

    let render = |composition: &mut Composition<MemoryApplier>| {
        let banner = banner.clone();
        let shares = shares.clone();
        let pressure = pressure.clone();
        let banner_sink = Rc::clone(&seen_banner);
        let share_sink = Rc::clone(&seen_shares);
        let pressure_sink = Rc::clone(&seen_pressure);
        composition
            .render(key, move || {
                if show_banner.get() {
                    let banner = banner.clone();
                    let banner_sink = Rc::clone(&banner_sink);
                    let stream = rememberEventStream((), move |sender| {
                        banner.observe(move |e| sender.send(e))
                    });
                    CollectEvents(stream, (), move |event| {
                        banner_sink.borrow_mut().push(event)
                    });
                }
                let shares = shares.clone();
                let share_sink = Rc::clone(&share_sink);
                let share_stream =
                    rememberEventStream((), move |sender| shares.observe(move |e| sender.send(e)));
                CollectEvents(share_stream, (), move |event| {
                    share_sink.borrow_mut().push(event)
                });
                let pressure = pressure.clone();
                let pressure_sink = Rc::clone(&pressure_sink);
                let pressure_stream = rememberEventStream((), move |sender| {
                    pressure.observe(move |e| sender.send(e))
                });
                CollectEvents(pressure_stream, (), move |event| {
                    pressure_sink.borrow_mut().push(event)
                });
            })
            .expect("render succeeds");
    };

    render(&mut composition);
    show_banner.set(true);
    render(&mut composition);

    assert_eq!(
        banner.observer_count(),
        1,
        "the banner stream never subscribed to its service"
    );
    assert_eq!(shares.observer_count(), 1, "one subscription per stream");
    assert_eq!(pressure.observer_count(), 1, "one subscription per stream");

    banner.publish(1);
    shares.publish(2);
    pressure.publish(3);
    assert!(
        pump_until(&mut composition, || !seen_banner.borrow().is_empty()
            && !seen_shares.borrow().is_empty()
            && !seen_pressure.borrow().is_empty()),
        "an event never arrived: banner {:?}, shares {:?}, pressure {:?}",
        seen_banner.borrow(),
        seen_shares.borrow(),
        seen_pressure.borrow()
    );
    assert_eq!(
        *seen_banner.borrow(),
        vec![1],
        "one banner publish must deliver exactly once"
    );
    assert_eq!(
        *seen_shares.borrow(),
        vec![2],
        "one share publish must deliver exactly once"
    );
    assert_eq!(
        *seen_pressure.borrow(),
        vec![3],
        "one pressure publish must deliver exactly once"
    );
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
