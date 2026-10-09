//! A composable that returns a value keeps what its body returned: a call
//! that skips its body returns it, and a recomposition of the composable's
//! own scope replaces it.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{Composition, MemoryApplier, MutableState, location_key};

#[cranpose_macros::composable]
fn Doubled(base: i32, extra: MutableState<i32>, runs: Rc<Cell<u32>>) -> i32 {
    runs.set(runs.get() + 1);
    (base + extra.get()) * 2
}

#[cranpose_macros::composable]
fn Tapped(base: i32, on_tap: impl Fn() + 'static) -> i32 {
    on_tap();
    base + 100
}

#[cranpose_macros::composable]
fn Consumer(
    base: MutableState<i32>,
    extra: MutableState<i32>,
    tick: MutableState<u32>,
    runs: Rc<Cell<u32>>,
    seen: Rc<RefCell<Vec<(i32, i32)>>>,
) {
    let _ = tick.get();
    let doubled = Doubled(base.get(), extra, Rc::clone(&runs));
    let tapped = Tapped(base.get(), || {});
    seen.borrow_mut().push((doubled, tapped));
}

fn settle(composition: &mut Composition<MemoryApplier>) {
    while composition.process_invalid_scopes().expect("recomposition") {}
}

#[test]
fn a_returned_value_follows_skips_and_own_scope_recompositions() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let base = MutableState::with_runtime(1, runtime.clone());
    let extra = MutableState::with_runtime(0, runtime.clone());
    let tick = MutableState::with_runtime(0u32, runtime);
    let runs = Rc::new(Cell::new(0));
    let seen = Rc::new(RefCell::new(Vec::new()));
    {
        let (runs, seen) = (Rc::clone(&runs), Rc::clone(&seen));
        composition
            .render(location_key(file!(), line!(), column!()), move || {
                Consumer(base, extra, tick, Rc::clone(&runs), Rc::clone(&seen));
            })
            .expect("initial composition");
    }
    assert_eq!(seen.borrow().last(), Some(&(2, 101)));
    assert_eq!(runs.get(), 1);

    // The consumer recomposes; the call skips and returns what it returned.
    tick.set(1);
    settle(&mut composition);
    assert_eq!(seen.borrow().last(), Some(&(2, 101)));
    assert_eq!(runs.get(), 1, "an unchanged call skips its body");

    // Only the composable's own scope reads `extra`: it recomposes alone,
    // and the consumer then reads the value it returned there.
    extra.set(5);
    settle(&mut composition);
    assert_eq!(runs.get(), 2);
    assert_eq!(seen.borrow().last(), Some(&(12, 101)));

    tick.set(2);
    settle(&mut composition);
    assert_eq!(runs.get(), 2, "the call skips again");
    assert_eq!(
        seen.borrow().last(),
        Some(&(12, 101)),
        "a skipped call returns what its own scope's recomposition returned"
    );

    base.set(2);
    settle(&mut composition);
    assert_eq!(runs.get(), 3);
    assert_eq!(seen.borrow().last(), Some(&(14, 102)));
}
