// The expansion of `#[composable]` runs inside the body, where the body's own
// bindings are in scope. A local, a loop variable, a parameter or a closure
// parameter named as a function the expansion calls, such as `drop`, must not
// change what the expansion calls.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{Composition, LaunchedEffect, MemoryApplier, location_key};

type Out = Rc<Cell<i32>>;

fn compose(content: impl FnMut()) -> Composition<MemoryApplier> {
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), content)
        .expect("initial composition");
    composition
}

#[cranpose_macros::composable]
fn LocalDrop(base: i32, out: Out) {
    let drop = base * 2;
    let (file, line) = (drop + 1, drop + 2);
    out.set(file * line);
}

#[cranpose_macros::composable]
fn ClosureDrop(out: Out) {
    let drop = |value: i32| value + 1;
    let total = drop(2);
    out.set(total);
}

#[cranpose_macros::composable]
fn LoopDrop(out: Out) {
    for drop in 1..=3 {
        let step = drop * 10;
        out.set(out.get() + step);
    }
}

#[cranpose_macros::composable]
fn ParameterDrop(drop: i32, out: Out) {
    let doubled = drop * 2;
    out.set(doubled);
}

#[cranpose_macros::composable]
fn CallbackDrop(drop: impl Fn(i32) + 'static) {
    let value = 5;
    drop(value);
}

#[cranpose_macros::composable]
fn ClosureParameterDrop(out: Out) {
    let apply = |drop: i32| {
        let doubled = drop * 2;
        out.set(doubled);
    };
    apply(4);
}

#[cranpose_macros::composable(no_skip)]
fn InlineDrop(out: Out) {
    let drop = 7;
    let seven = drop;
    out.set(seven);
}

#[cranpose_macros::composable]
fn ReturnedDrop(base: i32) -> i32 {
    let drop = base + 1;
    drop * 2
}

#[cranpose_macros::composable]
fn EffectDrop(out: Out) {
    LaunchedEffect((), async move |_| {
        let drop = 6;
        let total = drop + 1;
        std::future::ready(()).await;
        out.set(total);
    });
}

#[test]
fn a_local_named_drop_composes() {
    let out = Out::default();
    let sink = Rc::clone(&out);
    compose(move || LocalDrop(4, Rc::clone(&sink)));
    assert_eq!(out.get(), 9 * 10);
}

#[test]
fn a_closure_named_drop_composes() {
    let out = Out::default();
    let sink = Rc::clone(&out);
    compose(move || ClosureDrop(Rc::clone(&sink)));
    assert_eq!(out.get(), 3);
}

#[test]
fn a_loop_variable_named_drop_composes() {
    let out = Out::default();
    let sink = Rc::clone(&out);
    compose(move || LoopDrop(Rc::clone(&sink)));
    assert_eq!(out.get(), 60);
}

#[test]
fn a_parameter_named_drop_composes() {
    let out = Out::default();
    let sink = Rc::clone(&out);
    compose(move || ParameterDrop(21, Rc::clone(&sink)));
    assert_eq!(out.get(), 42);
}

#[test]
fn a_callback_parameter_named_drop_is_called_with_its_argument() {
    let out = Out::default();
    let sink = Rc::clone(&out);
    compose(move || {
        let sink = Rc::clone(&sink);
        CallbackDrop(move |value| sink.set(value));
    });
    assert_eq!(out.get(), 5);
}

#[test]
fn a_closure_parameter_named_drop_composes() {
    let out = Out::default();
    let sink = Rc::clone(&out);
    compose(move || ClosureParameterDrop(Rc::clone(&sink)));
    assert_eq!(out.get(), 8);
}

#[test]
fn an_inline_composable_with_a_local_named_drop_composes() {
    let out = Out::default();
    let sink = Rc::clone(&out);
    compose(move || InlineDrop(Rc::clone(&sink)));
    assert_eq!(out.get(), 7);
}

#[test]
fn a_composable_that_returns_a_value_may_bind_a_local_named_drop() {
    let returned = Out::default();
    let sink = Rc::clone(&returned);
    compose(move || sink.set(ReturnedDrop(4)));
    assert_eq!(returned.get(), 10);
}

#[test]
fn an_effect_with_a_local_named_drop_runs_its_statements() {
    let out = Out::default();
    let sink = Rc::clone(&out);
    let composition = compose(move || EffectDrop(Rc::clone(&sink)));
    let runtime = composition.runtime_handle();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while out.get() == 0 && std::time::Instant::now() < until {
        runtime.drain_ui();
        std::thread::yield_now();
    }
    assert_eq!(out.get(), 7);
}

// A glob import of a variant named `None` shadows the prelude's. The helper of
// a composable that returns a value and holds a callback names `None` too.
mod variant_named_none {
    use std::cell::Cell;

    use cranpose_core::{Composition, MemoryApplier, location_key};

    enum Mode {
        None,
        Edit,
    }

    use Mode::*;

    #[cranpose_macros::composable]
    fn Tapped(base: i32, on_tap: impl Fn() + 'static) -> i32 {
        on_tap();
        match if base > 0 { Edit } else { None } {
            None => 0,
            Edit => base + 100,
        }
    }

    #[test]
    fn a_variant_named_none_does_not_change_the_expansion() {
        let mut composition = Composition::new(MemoryApplier::new());
        let returned = Cell::new(0);
        composition
            .render(location_key(file!(), line!(), column!()), || {
                returned.set(Tapped(4, || {}));
            })
            .expect("initial composition");
        assert_eq!(returned.get(), 104);
    }
}
