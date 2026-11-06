use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use cranpose_core::{Composition, Key, LaunchedEffect, MemoryApplier, delay, location_key};

const WAIT: Duration = Duration::from_millis(20);

type Log = Rc<RefCell<Vec<String>>>;

// An effect keyed by `key` that waits, as does a task it launches, and notes
// each step in `log`. The effect waits longer than its task, so only the
// effect's cancellation can stop the task.
fn waiting_effect(log: &Log, key: u32) {
    let log = Rc::clone(log);
    LaunchedEffect(key, async move |scope| {
        log.borrow_mut().push(format!("{key} waits"));
        let child = Rc::clone(&log);
        scope.launch(async move {
            delay(WAIT).await;
            child.borrow_mut().push(format!("{key} child woke"));
        });
        delay(WAIT * 2).await;
        log.borrow_mut().push(format!("{key} woke"));
    });
}

fn render(composition: &mut Composition<MemoryApplier>, root: Key, log: &Log, key: Option<u32>) {
    let log = Rc::clone(log);
    composition
        .render(root, move || {
            if let Some(key) = key {
                waiting_effect(&log, key);
            }
        })
        .expect("render succeeds");
    composition.runtime_handle().drain_ui();
}

// Drains the runtime until every delay started before has resolved.
fn wait_out_delays(composition: &Composition<MemoryApplier>) {
    let runtime = composition.runtime_handle();
    let until = Instant::now() + WAIT * 5;
    while Instant::now() < until {
        runtime.drain_ui();
        std::thread::yield_now();
    }
}

fn noted(log: &Log, line: &str) -> bool {
    log.borrow().iter().any(|entry| entry == line)
}

#[test]
fn a_new_key_stops_the_effect_where_it_awaits() {
    let mut composition = Composition::new(MemoryApplier::new());
    let root = location_key(file!(), line!(), column!());
    let log = Log::default();
    render(&mut composition, root, &log, Some(0));
    render(&mut composition, root, &log, Some(1));
    wait_out_delays(&composition);

    assert!(noted(&log, "0 waits") && noted(&log, "1 woke"), "{log:?}");
    assert!(
        !noted(&log, "0 woke"),
        "a line after an await ran once its effect was cancelled: {log:?}"
    );
}

#[test]
fn leaving_the_composition_stops_the_effect_where_it_awaits() {
    let mut composition = Composition::new(MemoryApplier::new());
    let root = location_key(file!(), line!(), column!());
    let log = Log::default();
    render(&mut composition, root, &log, Some(0));
    render(&mut composition, root, &log, None);
    wait_out_delays(&composition);

    assert_eq!(*log.borrow(), ["0 waits"]);
}

#[test]
fn a_task_an_effect_launched_stops_with_the_effect() {
    let mut composition = Composition::new(MemoryApplier::new());
    let root = location_key(file!(), line!(), column!());
    let log = Log::default();
    render(&mut composition, root, &log, Some(0));
    render(&mut composition, root, &log, Some(1));
    wait_out_delays(&composition);

    assert!(noted(&log, "1 child woke"), "{log:?}");
    assert!(
        !noted(&log, "0 child woke"),
        "a task of a cancelled effect went on: {log:?}"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn background_work_reports_to_an_effect_whose_block_has_returned() {
    let mut composition = Composition::new(MemoryApplier::new());
    let root = location_key(file!(), line!(), column!());
    let got = Rc::new(std::cell::Cell::new(None));
    let sink = Rc::clone(&got);
    composition
        .render(root, move || {
            let sink = Rc::clone(&sink);
            LaunchedEffect((), async move |scope| {
                scope.launch_background(|_| async { 7 }, move |value| sink.set(Some(value)));
            });
        })
        .expect("render succeeds");
    let runtime = composition.runtime_handle();
    let until = Instant::now() + Duration::from_secs(5);
    while got.get().is_none() && Instant::now() < until {
        runtime.drain_ui();
        std::thread::yield_now();
    }

    assert_eq!(got.get(), Some(7));
}

// Notes in its cell when the future that holds it is dropped.
struct DropNote(Rc<std::cell::Cell<bool>>);

impl Drop for DropNote {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

// An effect that ticks forever, with a note of its future's drop.
fn ticking_effect(dropped: &Rc<std::cell::Cell<bool>>, ticks: &Rc<std::cell::Cell<u32>>) {
    let dropped = Rc::clone(dropped);
    let ticks = Rc::clone(ticks);
    LaunchedEffect((), async move |_| {
        let _note = DropNote(dropped);
        loop {
            delay(WAIT).await;
            ticks.set(ticks.get() + 1);
        }
    });
}

// A composition whose looping effect has ticked.
struct Ticked {
    composition: Composition<MemoryApplier>,
    root: Key,
    dropped: Rc<std::cell::Cell<bool>>,
    ticks: Rc<std::cell::Cell<u32>>,
}

fn ticked() -> Ticked {
    let mut composition = Composition::new(MemoryApplier::new());
    let root = location_key(file!(), line!(), column!());
    let dropped = Rc::new(std::cell::Cell::new(false));
    let ticks = Rc::new(std::cell::Cell::new(0));
    let (note, count) = (Rc::clone(&dropped), Rc::clone(&ticks));
    composition
        .render(root, move || ticking_effect(&note, &count))
        .expect("render succeeds");
    wait_out_delays(&composition);
    assert!(ticks.get() > 0 && !dropped.get(), "the loop never ran");
    Ticked {
        composition,
        root,
        dropped,
        ticks,
    }
}

#[test]
fn leaving_the_composition_drops_a_looping_effect_at_once() {
    let Ticked {
        mut composition,
        root,
        dropped,
        ticks,
    } = ticked();

    composition.render(root, || {}).expect("render succeeds");

    assert!(dropped.get(), "the loop's future outlived its call site");
    let seen = ticks.get();
    wait_out_delays(&composition);
    assert_eq!(ticks.get(), seen, "the loop ticked after it left");
}

#[test]
fn dropping_the_composition_drops_a_looping_effect() {
    let Ticked {
        composition,
        dropped,
        ..
    } = ticked();

    drop(composition);

    assert!(dropped.get(), "the loop's future outlived its composition");
}
