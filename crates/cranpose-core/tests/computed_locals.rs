//! Composition locals whose values are computed where they are read, from
//! the other locals there, as Compose's computed locals are.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{
    Composition, CompositionLocal, CompositionLocalProvider, MemoryApplier, MutableState,
    StaticCompositionLocal, compositionLocalOf, compositionLocalWithComputedDefaultOf,
    location_key, staticCompositionLocalWithComputedDefaultOf,
};

type Seen = Rc<RefCell<Vec<(&'static str, i32)>>>;

thread_local! {
    static BASE: CompositionLocal<i32> = compositionLocalOf(|| 1);
    static LARGER: CompositionLocal<i32> =
        compositionLocalWithComputedDefaultOf(|scope| scope.current_value(&base()) + 10);
    static SCALED: StaticCompositionLocal<i32> =
        staticCompositionLocalWithComputedDefaultOf(|scope| scope.current_value(&base()) * 100);
}

fn base() -> CompositionLocal<i32> {
    BASE.with(Clone::clone)
}

fn larger() -> CompositionLocal<i32> {
    LARGER.with(Clone::clone)
}

fn scaled() -> StaticCompositionLocal<i32> {
    SCALED.with(Clone::clone)
}

#[cranpose_macros::composable]
fn Reader(label: &'static str, seen: Seen) {
    seen.borrow_mut().push((label, larger().current()));
    seen.borrow_mut().push((label, scaled().current()));
}

#[cranpose_macros::composable]
fn Root(value: MutableState<i32>, seen: Seen) {
    Reader("outside", Rc::clone(&seen));
    CompositionLocalProvider([base().provides(value.get())], || {
        Reader("inside", Rc::clone(&seen));
    });
}

fn settle(composition: &mut Composition<MemoryApplier>) {
    while composition.process_invalid_scopes().expect("recomposition") {}
}

#[test]
fn a_computed_default_follows_the_locals_where_it_is_read() {
    let mut composition = Composition::new(MemoryApplier::new());
    let value = MutableState::with_runtime(2, composition.runtime_handle());
    let seen: Seen = Rc::default();
    {
        let seen = Rc::clone(&seen);
        composition
            .render(location_key(file!(), line!(), column!()), move || {
                Root(value, Rc::clone(&seen));
            })
            .expect("initial composition");
    }
    assert_eq!(
        seen.take(),
        [
            ("outside", 11),
            ("outside", 100),
            ("inside", 12),
            ("inside", 200)
        ]
    );

    value.set(3);
    settle(&mut composition);
    assert_eq!(seen.take(), [("inside", 13), ("inside", 300)]);
}

#[cranpose_macros::composable]
fn DoubledBase(base_value: MutableState<i32>, seen: Seen) {
    CompositionLocalProvider([base().provides(base_value.get())], || {
        seen.borrow_mut().push(("doubled", larger().current()));
    });
}

#[test]
fn a_computed_value_reads_the_locals_below_its_provider() {
    let mut composition = Composition::new(MemoryApplier::new());
    let value = MutableState::with_runtime(5, composition.runtime_handle());
    let seen: Seen = Rc::default();
    {
        let seen = Rc::clone(&seen);
        composition
            .render(location_key(file!(), line!(), column!()), move || {
                CompositionLocalProvider(
                    [larger().provides_computed(|scope| scope.current_value(&base()) * 2)],
                    || DoubledBase(value, Rc::clone(&seen)),
                );
            })
            .expect("initial composition");
    }
    assert_eq!(
        seen.take(),
        [("doubled", 10)],
        "the base at the read, not the default where the value was provided"
    );

    value.set(7);
    settle(&mut composition);
    assert_eq!(seen.take(), [("doubled", 14)]);
}
