// A derived state kept by a group that leaves the composition and returns
// keeps following its source.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{
    Composition, Key, MemoryApplier, MutableState, RecomposeOptions, derivedStateOf, location_key,
    mutableStateOf,
};

const BRANCH: Key = Key(7);

// A reader of `derivedStateOf(source >= 10)` in a group retained while it is
// not shown, written as `#[composable]` expands it: the group's recompose
// callback runs the body again. Each run records what it read.
fn branch(source: MutableState<u32>, seen: &Rc<Cell<Option<bool>>>) {
    fn body(source: MutableState<u32>, seen: &Cell<Option<bool>>) {
        let big = derivedStateOf(move || source.get() >= 10);
        seen.set(Some(big.get()));
    }
    let key = location_key(file!(), line!(), column!());
    cranpose_core::with_current_composer(|composer| {
        composer.cranpose_with_reuse(BRANCH, RecomposeOptions::default(), |composer| {
            composer.with_group(key, |composer| {
                let again = Rc::clone(seen);
                composer.set_recompose_callback(move |_| body(source, &again));
                body(source, seen);
            });
        });
    });
}

#[test]
fn a_derived_state_of_a_returning_group_follows_its_source() {
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let source = mutableStateOf(1u32);
    let seen = Rc::new(Cell::new(None));
    let render = |composition: &mut Composition<MemoryApplier>, shown: bool| {
        let seen = Rc::clone(&seen);
        composition
            .render(key, move || {
                if shown {
                    branch(source, &seen);
                }
            })
            .expect("render succeeds");
    };

    render(&mut composition, true);
    assert_eq!(seen.get(), Some(false));
    render(&mut composition, false);
    render(&mut composition, true);
    assert_eq!(seen.get(), Some(false));

    seen.set(None);
    source.set(12);
    composition
        .process_invalid_scopes()
        .expect("recomposition succeeds");
    assert_eq!(
        seen.get(),
        Some(true),
        "the returned group's derived state recomputes and its reader sees the new value"
    );
}
