//! Scopes of one depth recompose in the order their states invalidated them,
//! whatever the depths of the scopes queued between them.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{
    Composition, MemoryApplier, MutableState, location_key, rememberMutableStateOf,
};

type Log = Rc<RefCell<Vec<usize>>>;

const ROWS: usize = 48;
const INNER: usize = 1_000;

#[cranpose_macros::composable]
fn Outer(index: usize, outer: MutableState<u32>, inner: MutableState<u32>, log: Log) {
    let _: u32 = outer.value();
    log.borrow_mut().push(index);
    Inner(index, inner, log);
}

#[cranpose_macros::composable]
fn Inner(index: usize, inner: MutableState<u32>, log: Log) {
    let _: u32 = inner.value();
    log.borrow_mut().push(INNER + index);
}

#[test]
fn scopes_of_one_depth_recompose_in_the_order_they_were_invalidated() {
    let mut composition = Composition::new(MemoryApplier::new());
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let states = Rc::new(RefCell::new(Vec::new()));
    let key = location_key(file!(), line!(), column!());
    let mut content = {
        let (log, states) = (Rc::clone(&log), Rc::clone(&states));
        move || {
            states.borrow_mut().clear();
            for index in 0..ROWS {
                cranpose_core::with_key(&index, || {
                    let outer = rememberMutableStateOf(|| 0u32);
                    let inner = rememberMutableStateOf(|| 0u32);
                    states.borrow_mut().push((outer, inner));
                    Outer(index, outer, inner, Rc::clone(&log));
                });
            }
        }
    };
    composition.render(key, &mut content).expect("first render");
    let states = states.borrow().clone();

    // Rows from the back invalidate an inner scope, rows from the front an
    // outer one, by turns, so the queue holds both depths mixed.
    let half = ROWS / 2;
    let mut outer_order = Vec::new();
    let mut inner_order = Vec::new();
    for step in 0..half {
        let inner_row = ROWS - 1 - step;
        states[inner_row].1.set(1);
        inner_order.push(inner_row);
        let outer_row = (step * 7) % half;
        states[outer_row].0.set(1);
        outer_order.push(outer_row);
    }
    log.borrow_mut().clear();
    composition
        .process_invalid_scopes()
        .expect("the rows recompose");

    // Outer scopes first, as ancestors go first, each depth in queue order.
    let log = log.borrow();
    let outers: Vec<usize> = log.iter().copied().filter(|&run| run < INNER).collect();
    assert_eq!(outers, outer_order);
    let inner_only = |run: &usize| *run >= INNER + half;
    let inners: Vec<usize> = log
        .iter()
        .copied()
        .filter(inner_only)
        .map(|run| run - INNER)
        .collect();
    assert_eq!(inners, inner_order);
    let first_inner = log.iter().position(inner_only).expect("inner scopes ran");
    assert!(log[first_inner..].iter().all(|&run| run >= INNER));
}

#[cranpose_macros::composable]
fn Reader(index: usize, shared: MutableState<u32>, log: Log) {
    let _: u32 = shared.value();
    log.borrow_mut().push(index);
}

// A state that more scopes read than its list on the stack holds
// invalidates each of them on every change.
#[test]
fn a_state_that_many_scopes_read_invalidates_each_on_every_change() {
    const READERS: usize = 12;
    let mut composition = Composition::new(MemoryApplier::new());
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let shared = Rc::new(RefCell::new(None));
    let key = location_key(file!(), line!(), column!());
    let mut content = {
        let (log, shared) = (Rc::clone(&log), Rc::clone(&shared));
        move || {
            let state = rememberMutableStateOf(|| 0u32);
            shared.replace(Some(state));
            for index in 0..READERS {
                cranpose_core::with_key(&index, || Reader(index, state, Rc::clone(&log)));
            }
        }
    };
    composition.render(key, &mut content).expect("first render");
    let state = shared.borrow().expect("the shared state");

    for change in 1..4 {
        log.borrow_mut().clear();
        state.set(change);
        composition
            .process_invalid_scopes()
            .expect("the readers recompose");
        let mut read = log.borrow().clone();
        read.sort_unstable();
        assert_eq!(read, (0..READERS).collect::<Vec<_>>(), "change {change}");
    }
}
