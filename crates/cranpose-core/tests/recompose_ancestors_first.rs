//! A pass recomposes a scope before the scopes below it, as Compose
//! recomposes in composition order: a child the parent removes never runs
//! again with the arguments it had, and its side effects stay with it.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{
    Composition, MemoryApplier, MutableState, SideEffect, location_key, rememberMutableStateOf,
};

#[cranpose_macros::composable]
fn Host(open: MutableState<bool>, tick: MutableState<u32>, runs: Rc<Cell<usize>>) {
    let _ = tick.value();
    Gate(open, Rc::clone(&runs));
}

#[cranpose_macros::composable(no_skip)]
fn Gate(open: MutableState<bool>, runs: Rc<Cell<usize>>) {
    if open.value() {
        Shown(open, runs);
    }
}

#[cranpose_macros::composable]
fn Shown(open: MutableState<bool>, runs: Rc<Cell<usize>>) {
    let is_open = open.value();
    SideEffect(move || {
        if !is_open {
            runs.set(runs.get() + 1);
        }
    });
}

#[cranpose_macros::composable]
fn Panel(visible: MutableState<bool>, open: MutableState<bool>, runs: Rc<Cell<usize>>) {
    if visible.value() {
        Shown(open, runs);
    }
    // Read after the child's own read: the child watches `open` first.
    let _ = open.value();
}

#[test]
fn a_child_that_watches_a_state_first_still_recomposes_after_its_parent() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runs = Rc::new(Cell::new(0));
    let states = Rc::new(Cell::new(None));
    let key = location_key(file!(), line!(), column!());
    let mut content = {
        let (runs, states) = (Rc::clone(&runs), Rc::clone(&states));
        move || {
            let visible = rememberMutableStateOf(|| true);
            let open = rememberMutableStateOf(|| true);
            states.set(Some((visible, open)));
            Panel(visible, open, Rc::clone(&runs));
        }
    };
    composition.render(key, &mut content).expect("first render");
    let (visible, open) = states.get().expect("the states");

    cranpose_core::run_in_mutable_snapshot(|| {
        open.set(false);
        visible.set(false);
    })
    .expect("one change");
    composition
        .reconcile(key, &mut content)
        .expect("the panel hides the child");

    assert_eq!(
        runs.get(),
        0,
        "the child the panel removes did not compose with `open` false"
    );
}

#[test]
fn a_child_its_parent_removes_does_not_run_again_with_the_arguments_it_had() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runs = Rc::new(Cell::new(0));
    let states = Rc::new(Cell::new(None));
    let key = location_key(file!(), line!(), column!());
    let mut content = {
        let (runs, states) = (Rc::clone(&runs), Rc::clone(&states));
        move || {
            let open = rememberMutableStateOf(|| true);
            let tick = rememberMutableStateOf(|| 0u32);
            states.set(Some((open, tick)));
            Host(open, tick, Rc::clone(&runs));
        }
    };
    composition.render(key, &mut content).expect("first render");
    let (open, tick) = states.get().expect("the states");

    // The host recomposes alone: the gate's read of `open` happens in that
    // pass, and belongs to the host as it did in the first composition.
    tick.set(1);
    composition
        .reconcile(key, &mut content)
        .expect("the host recomposes");

    open.set(false);
    composition
        .reconcile(key, &mut content)
        .expect("the gate closes");

    assert_eq!(
        runs.get(),
        0,
        "the child the closed gate removes did not compose with `open` false"
    );
}
