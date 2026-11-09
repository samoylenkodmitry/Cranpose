//! A composable whose body reads no state keeps no restart scope, and makes
//! one when a body of it first reads a state: from then on a change of the
//! state recomposes it alone, as when it read the state from its first body.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{
    Composition, MemoryApplier, MutableState, location_key, remember, rememberMutableStateOf,
};

#[cranpose_macros::composable]
fn Reader(reads: bool, state: MutableState<u32>, runs: Rc<Cell<usize>>, seen: Rc<Cell<u32>>) {
    runs.set(runs.get() + 1);
    if reads {
        seen.set(state.value());
    }
}

#[cranpose_macros::composable]
fn Parent(
    reads: MutableState<bool>,
    value: MutableState<u32>,
    runs: Rc<Cell<usize>>,
    (reader_runs, seen): (Rc<Cell<usize>>, Rc<Cell<u32>>),
) {
    runs.set(runs.get() + 1);
    Reader(reads.value(), value, reader_runs, seen);
}

#[cranpose_macros::composable]
fn RememberingReader(state: MutableState<u32>, runs: Rc<Cell<usize>>, seen: Rc<Cell<u32>>) {
    runs.set(runs.get() + 1);
    // A read while the slot table is open for a write.
    let first = remember(|| state.value()).with(|first| *first);
    seen.set(state.value().max(first));
}

#[test]
fn a_composable_that_starts_reading_a_state_recomposes_alone_when_it_changes() {
    let mut composition = Composition::new(MemoryApplier::new());
    let parent_runs = Rc::new(Cell::new(0));
    let reader_runs = Rc::new(Cell::new(0));
    let seen = Rc::new(Cell::new(0));
    let states = Rc::new(Cell::new(None));
    let key = location_key(file!(), line!(), column!());
    let mut content = {
        let (parent_runs, reader_runs, seen, states) = (
            Rc::clone(&parent_runs),
            Rc::clone(&reader_runs),
            Rc::clone(&seen),
            Rc::clone(&states),
        );
        move || {
            let reads = rememberMutableStateOf(|| false);
            let value = rememberMutableStateOf(|| 1_u32);
            states.set(Some((reads, value)));
            Parent(
                reads,
                value,
                Rc::clone(&parent_runs),
                (Rc::clone(&reader_runs), Rc::clone(&seen)),
            );
        }
    };
    composition.render(key, &mut content).expect("first render");
    let (reads, value) = states.get().expect("the states");
    assert_eq!(
        (parent_runs.get(), reader_runs.get(), seen.get()),
        (1, 1, 0)
    );

    value.set(2);
    while composition.process_invalid_scopes().expect("recomposes") {}
    assert_eq!(
        (parent_runs.get(), reader_runs.get()),
        (1, 1),
        "nothing read the state yet"
    );

    reads.set(true);
    while composition.process_invalid_scopes().expect("recomposes") {}
    assert_eq!(
        (parent_runs.get(), reader_runs.get(), seen.get()),
        (2, 2, 2)
    );

    value.set(3);
    while composition.process_invalid_scopes().expect("recomposes") {}
    assert_eq!(
        (parent_runs.get(), reader_runs.get(), seen.get()),
        (2, 3, 3),
        "the reader recomposes alone"
    );
}

#[test]
fn a_composable_that_reads_a_state_in_a_remember_recomposes_when_it_changes() {
    let mut composition = Composition::new(MemoryApplier::new());
    let reader_runs = Rc::new(Cell::new(0));
    let seen = Rc::new(Cell::new(0));
    let states = Rc::new(Cell::new(None));
    let key = location_key(file!(), line!(), column!());
    let mut content = {
        let (reader_runs, seen, states) = (
            Rc::clone(&reader_runs),
            Rc::clone(&seen),
            Rc::clone(&states),
        );
        move || {
            let value = rememberMutableStateOf(|| 4_u32);
            states.set(Some(value));
            RememberingReader(value, Rc::clone(&reader_runs), Rc::clone(&seen));
        }
    };
    composition.render(key, &mut content).expect("first render");
    let value = states.get().expect("the state");
    assert_eq!((reader_runs.get(), seen.get()), (1, 4));

    value.set(9);
    while composition.process_invalid_scopes().expect("recomposes") {}
    assert_eq!((reader_runs.get(), seen.get()), (2, 9));
}
