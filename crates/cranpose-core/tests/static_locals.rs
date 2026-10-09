//! A static composition local's provider whose value changes recomposes
//! everything it provides to, as Compose's `staticCompositionLocalOf` does,
//! while a read of it subscribes nothing.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{
    Composition, CompositionLocalProvider, MemoryApplier, MutableState, StaticCompositionLocal,
    location_key, staticCompositionLocalOf,
};

type Seen = Rc<RefCell<Vec<(&'static str, i32)>>>;

thread_local! {
    static LOCAL: StaticCompositionLocal<i32> = staticCompositionLocalOf(|| 0);
}

fn local() -> StaticCompositionLocal<i32> {
    LOCAL.with(Clone::clone)
}

#[cranpose_macros::composable]
fn Reader(label: &'static str, seen: Seen) {
    seen.borrow_mut().push((label, local().current()));
}

/// Its parameters never change: a call skips unless its body is forced.
#[cranpose_macros::composable]
fn Wrapper(seen: Seen, runs: Rc<Cell<u32>>) {
    runs.set(runs.get() + 1);
    Reader("wrapped", seen);
}

#[cranpose_macros::composable]
fn Root(value: MutableState<i32>, tick: MutableState<u32>, seen: Seen, runs: Rc<Cell<u32>>) {
    let _ = tick.get();
    Reader("outside", Rc::clone(&seen));
    CompositionLocalProvider([local().provides(value.get())], || {
        Wrapper(Rc::clone(&seen), Rc::clone(&runs));
    });
}

fn settle(composition: &mut Composition<MemoryApplier>) {
    while composition.process_invalid_scopes().expect("recomposition") {}
}

#[test]
fn a_changed_static_value_recomposes_what_its_provider_holds() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let value = MutableState::with_runtime(1, runtime.clone());
    let tick = MutableState::with_runtime(0_u32, runtime);
    let seen: Seen = Rc::default();
    let runs = Rc::new(Cell::new(0));
    {
        let (seen, runs) = (Rc::clone(&seen), Rc::clone(&runs));
        composition
            .render(location_key(file!(), line!(), column!()), move || {
                Root(value, tick, Rc::clone(&seen), Rc::clone(&runs));
            })
            .expect("initial composition");
    }
    assert_eq!(seen.take(), [("outside", 0), ("wrapped", 1)]);

    // The provider runs again with the value it had: its content skips.
    tick.set(1);
    settle(&mut composition);
    assert_eq!(seen.take(), []);
    assert_eq!(runs.get(), 1);

    // A new value runs every body under the provider, even a call whose
    // parameters did not change; the reader outside it still skips.
    value.set(2);
    settle(&mut composition);
    assert_eq!(seen.take(), [("wrapped", 2)]);
    assert_eq!(runs.get(), 2);

    // Its content skips again once the value holds.
    tick.set(2);
    settle(&mut composition);
    assert_eq!(seen.take(), []);
    assert_eq!(runs.get(), 2);
}
