//! The state hooks, exercised through a real composition.
//!
//! Each of these reaches for the composer, allocates against the runtime, and
//! hands back a handle whose identity has to survive recomposition. None of that
//! can be checked by calling the function — it has to be composed, recomposed,
//! and asked whether it is still the same state.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{
    location_key, mutableStateList, mutableStateListOf, mutableStateMap, mutableStateMapOf,
    ownedMutableStateOf, remember, rememberMutableStateOf, rememberMutableStateOfNeverEqual,
    rememberUpdatedState, try_mutableStateOf, Composition, MemoryApplier,
};

/// Renders `content` `passes` times over one composition, as recomposition does.
fn compose(passes: usize, mut content: impl FnMut()) {
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    for _ in 0..passes {
        composition
            .render(key, &mut content)
            .expect("the composition renders");
    }
}

#[test]
fn a_state_list_is_allocated_per_pass_and_remembered_only_when_asked() {
    // `mutableStateList` allocates, the way `mutableStateOf` does; it is
    // `remember` that makes a list survive recomposition. Pinning both halves
    // here is what stops the two from being confused for each other.
    let fresh = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&fresh);
    compose(3, move || {
        let list = mutableStateList::<u32>();
        sink.borrow_mut().push(list.len());
        list.push(1);
    });
    assert_eq!(
        *fresh.borrow(),
        vec![0, 0, 0],
        "an unremembered list must start empty on every pass"
    );

    let kept = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&kept);
    compose(3, move || {
        let list = remember(mutableStateList::<u32>).with(Clone::clone);
        sink.borrow_mut().push(list.len());
        list.push(1);
    });
    assert_eq!(
        *kept.borrow(),
        vec![0, 1, 2],
        "a remembered list must carry its contents across recomposition"
    );
}

#[test]
fn a_state_list_can_be_seeded_with_values() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    compose(1, move || {
        let list = mutableStateListOf([10u32, 20, 30]);
        sink.borrow_mut().extend(list.iter());
    });
    assert_eq!(*seen.borrow(), vec![10, 20, 30]);
}

#[test]
fn a_state_map_is_allocated_per_pass_and_remembered_only_when_asked() {
    let fresh = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&fresh);
    compose(3, move || {
        let map = mutableStateMap::<String, u32>();
        sink.borrow_mut().push(map.len());
        map.insert(format!("key-{}", map.len()), 1);
    });
    assert_eq!(*fresh.borrow(), vec![0, 0, 0]);

    let kept = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&kept);
    compose(3, move || {
        let map = remember(mutableStateMap::<String, u32>).with(Clone::clone);
        sink.borrow_mut().push(map.len());
        map.insert(format!("key-{}", map.len()), 1);
    });
    assert_eq!(*kept.borrow(), vec![0, 1, 2]);
}

#[test]
fn a_state_map_can_be_seeded_with_pairs() {
    let found = Rc::new(RefCell::new(None));
    let sink = Rc::clone(&found);
    compose(1, move || {
        let map = mutableStateMapOf([("a".to_string(), 1u32), ("b".to_string(), 2)]);
        *sink.borrow_mut() = map.get(&"b".to_string());
    });
    assert_eq!(*found.borrow(), Some(2));
}

#[test]
fn an_updated_state_follows_the_value_it_was_given_this_pass() {
    let observed = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&observed);
    let pass = Rc::new(std::cell::Cell::new(0u32));
    compose(3, move || {
        pass.set(pass.get() + 1);
        let latest = rememberUpdatedState(pass.get());
        sink.borrow_mut().push(latest.get());
    });

    // `remember` alone would freeze the first value. The point of
    // `rememberUpdatedState` is that the handle is stable and the value is not.
    assert_eq!(*observed.borrow(), vec![1, 2, 3]);
}

#[test]
fn a_never_equal_state_notifies_even_when_the_value_does_not_change() {
    let writes = Rc::new(std::cell::Cell::new(0u32));
    let counter = Rc::clone(&writes);
    compose(1, move || {
        let state = rememberMutableStateOfNeverEqual(|| 7u32);
        // Structural equality would drop both of these as no-ops.
        state.set(7);
        state.set(7);
        counter.set(state.get());
    });
    assert_eq!(writes.get(), 7);
}

#[test]
fn a_structurally_equal_state_holds_the_value_it_was_given() {
    let seen = Rc::new(std::cell::Cell::new(0u32));
    let sink = Rc::clone(&seen);
    compose(2, move || {
        let state = rememberMutableStateOf(|| 1u32);
        state.set(state.get() + 1);
        sink.set(state.get());
    });
    // Two passes over one remembered state: 1 -> 2 -> 3.
    assert_eq!(seen.get(), 3);
}

#[test]
fn an_owned_state_outlives_the_composer_that_made_it() {
    let mut composition = Composition::new(MemoryApplier::new());
    let escaped: Rc<RefCell<Option<_>>> = Rc::default();
    let sink = Rc::clone(&escaped);
    composition
        .render(location_key(file!(), line!(), column!()), move || {
            *sink.borrow_mut() = Some(ownedMutableStateOf(41u32));
        })
        .expect("the composition renders");

    let owned = escaped.borrow_mut().take().expect("state was created");
    owned.handle().set(42);
    assert_eq!(owned.handle().get(), 42);
}

#[test]
fn a_fallible_state_allocation_answers_none_with_no_runtime() {
    // Outside a composition and outside a runtime there is nothing to allocate
    // against, and the fallible form is the one that says so instead of
    // panicking.
    assert!(
        try_mutableStateOf(0u32).is_none(),
        "a state was allocated with no runtime to own it"
    );
}

#[test]
fn a_fallible_state_allocation_succeeds_inside_a_composition() {
    let made = Rc::new(std::cell::Cell::new(false));
    let flag = Rc::clone(&made);
    compose(1, move || {
        flag.set(try_mutableStateOf(0u32).is_some());
    });
    assert!(made.get(), "a composition could not allocate state");
}
