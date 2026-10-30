// A call whose callbacks changed, and never ran outside an event handler,
// skips its body: its forwarders call the new callbacks. A callback that ran
// during composition, layout or drawing makes a new one a change.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{
    Composition, MemoryApplier, MutableState, location_key, run_in_mutable_snapshot,
};

type Clicks = RefCell<Option<Rc<dyn Fn(i32)>>>;

// What a test reads back from the composables under test.
#[derive(Default)]
struct Probe {
    runs: Cell<u32>,
    clicks: Clicks,
    shown: Cell<i32>,
}

impl PartialEq for Probe {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Probe {
    fn click(&self, value: i32) {
        let clicks = self.clicks.borrow().clone().expect("the clicker composed");
        clicks(value);
    }
}

#[cranpose_macros::composable]
fn Clicker(_label: i32, on_click: impl Fn(i32) + 'static, probe: Rc<Probe>) {
    probe.runs.set(probe.runs.get() + 1);
    *probe.clicks.borrow_mut() = Some(Rc::new(on_click));
}

#[cranpose_macros::composable]
fn Shown(show: impl Fn(i32) + 'static, probe: Rc<Probe>) {
    probe.runs.set(probe.runs.get() + 1);
    show(10);
}

// A clicker whose callback multiplies by `factor` into `total`.
#[cranpose_macros::composable]
fn ClickerHost(factor: MutableState<i32>, total: Rc<Cell<i32>>, probe: Rc<Probe>) {
    let factor = factor.get();
    Clicker(1, move |value| total.set(value * factor), probe);
}

#[cranpose_macros::composable]
fn ShownHost(base: MutableState<i32>, probe: Rc<Probe>) {
    let base = base.get();
    let shown = Rc::clone(&probe);
    Shown(move |scale| shown.shown.set(base * scale), probe);
}

fn settle(composition: &mut Composition<MemoryApplier>) {
    while composition.process_invalid_scopes().expect("recomposition") {}
}

fn compose_clicker(
    composition: &mut Composition<MemoryApplier>,
    factor: MutableState<i32>,
    total: &Rc<Cell<i32>>,
    probe: &Rc<Probe>,
) {
    let (total, probe) = (Rc::clone(total), Rc::clone(probe));
    composition
        .render(location_key(file!(), line!(), column!()), move || {
            ClickerHost(factor, Rc::clone(&total), Rc::clone(&probe));
        })
        .expect("initial composition");
}

#[test]
fn a_call_whose_new_callback_only_ran_in_event_handlers_skips_and_events_call_the_new_one() {
    let mut composition = Composition::new(MemoryApplier::new());
    let factor = MutableState::with_runtime(2, composition.runtime_handle());
    let total = Rc::new(Cell::new(0));
    let probe = Rc::new(Probe::default());
    compose_clicker(&mut composition, factor, &total, &probe);
    run_in_mutable_snapshot(|| probe.click(5)).expect("click");
    assert_eq!(total.get(), 10);

    factor.set(3);
    settle(&mut composition);
    assert_eq!(
        probe.runs.get(),
        1,
        "only the callback changed, and it ran in an event"
    );
    run_in_mutable_snapshot(|| probe.click(5)).expect("click");
    assert_eq!(total.get(), 15, "the event calls the callback passed last");
}

#[test]
fn a_call_whose_callback_ran_outside_an_event_handler_runs_again_for_a_new_one() {
    let mut composition = Composition::new(MemoryApplier::new());
    let factor = MutableState::with_runtime(2, composition.runtime_handle());
    let total = Rc::new(Cell::new(0));
    let probe = Rc::new(Probe::default());
    compose_clicker(&mut composition, factor, &total, &probe);
    // As a draw or a measure would, outside any event handler.
    probe.click(5);
    assert_eq!(total.get(), 10);

    factor.set(3);
    settle(&mut composition);
    assert_eq!(probe.runs.get(), 2);
}

#[test]
fn a_call_whose_callback_ran_while_it_composed_runs_again_for_a_new_one() {
    let mut composition = Composition::new(MemoryApplier::new());
    let base = MutableState::with_runtime(4, composition.runtime_handle());
    let probe = Rc::new(Probe::default());
    {
        let probe = Rc::clone(&probe);
        composition
            .render(location_key(file!(), line!(), column!()), move || {
                ShownHost(base, Rc::clone(&probe));
            })
            .expect("initial composition");
    }
    assert_eq!(probe.shown.get(), 40);

    base.set(7);
    settle(&mut composition);
    assert_eq!(probe.runs.get(), 2);
    assert_eq!(probe.shown.get(), 70);
}
