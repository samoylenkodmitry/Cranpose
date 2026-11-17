// A derived state computes with the closure of the latest composition of its
// reader, also when the reader recomposed only for the derived value.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{
    Composition, MemoryApplier, MutableState, derivedStateOf, location_key, mutableStateOf,
};

// A reader of `derivedStateOf(source + offset)`, written as `#[composable]`
// expands it. The offset is a plain value that the body reads when it builds
// the closure, so only a new composition hands the derived state a new one.
fn reader(source: MutableState<u32>, offset: &Rc<Cell<u32>>, seen: &Rc<Cell<u32>>) {
    fn body(source: MutableState<u32>, offset: &Cell<u32>, seen: &Cell<u32>) {
        let offset = offset.get();
        let sum = derivedStateOf(move || source.get() + offset);
        seen.set(sum.get());
    }
    let key = location_key(file!(), line!(), column!());
    cranpose_core::with_current_composer(|composer| {
        composer.with_group(key, |composer| {
            let (again_offset, again_seen) = (Rc::clone(offset), Rc::clone(seen));
            composer.set_recompose_callback(move |_| body(source, &again_offset, &again_seen));
            body(source, offset, seen);
        });
    });
}

#[test]
fn a_derived_state_takes_the_closure_of_its_readers_latest_composition() {
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let source = mutableStateOf(0u32);
    let offset = Rc::new(Cell::new(0u32));
    let seen = Rc::new(Cell::new(u32::MAX));
    {
        let (offset, seen) = (Rc::clone(&offset), Rc::clone(&seen));
        composition
            .render(key, move || {
                reader(source, &offset, &seen);
            })
            .expect("render succeeds");
    }
    assert_eq!(seen.get(), 0);

    // The derived value changes with the old closure, so the reader
    // recomposes for it and builds a closure with the new offset.
    offset.set(100);
    source.set(1);
    composition
        .process_invalid_scopes()
        .expect("recomposition succeeds");
    assert_eq!(seen.get(), 1);

    source.set(2);
    composition
        .process_invalid_scopes()
        .expect("recomposition succeeds");
    assert_eq!(
        seen.get(),
        102,
        "the derived state computes with the closure of the reader's latest composition"
    );
}
