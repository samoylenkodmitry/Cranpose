// A modifier local reaches a node composed below its provider, as Compose's
// `ModifierLocalConsumer` reads the nearest provider above it.

use std::{cell::Cell, rc::Rc};

use cranpose_core::MutableState;
use cranpose_ui::{Box, BoxSpec, Modifier, ModifierLocalKey, run_test_composition};

#[test]
fn a_child_composed_under_a_live_provider_reads_its_value() {
    let key = ModifierLocalKey::new(|| 0u32);
    let read: Rc<Cell<Option<u32>>> = Rc::default();
    let shown: Rc<Cell<Option<MutableState<bool>>>> = Rc::default();
    let (seen, show_handle) = (Rc::clone(&read), Rc::clone(&shown));
    let mut composition = run_test_composition(move || {
        let show =
            cranpose_core::remember(|| cranpose_core::mutableStateOf(false)).with(|state| *state);
        show_handle.set(Some(show));
        let key = key.clone();
        let seen = Rc::clone(&seen);
        Box(
            Modifier::empty().modifier_local_provider(key.clone(), || 7u32),
            BoxSpec::default(),
            move || {
                if !show.value() {
                    return;
                }
                let key = key.clone();
                let seen = Rc::clone(&seen);
                Box(
                    Modifier::empty()
                        .modifier_local_consumer(move |scope| seen.set(Some(*scope.get(&key)))),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
    });
    assert_eq!(read.get(), None, "no child composed yet");
    shown.get().expect("the show state").set(true);
    while composition.process_invalid_scopes().expect("recompose") {}
    assert_eq!(
        read.get(),
        Some(7),
        "the new child read the default, not its parent's value"
    );
}

#[test]
fn a_child_composed_with_its_provider_reads_its_value() {
    let key = ModifierLocalKey::new(|| 0u32);
    let read: Rc<Cell<Option<u32>>> = Rc::default();
    let seen = Rc::clone(&read);
    let _composition = run_test_composition(move || {
        let key = key.clone();
        let seen = Rc::clone(&seen);
        Box(
            Modifier::empty().modifier_local_provider(key.clone(), || 7u32),
            BoxSpec::default(),
            move || {
                let key = key.clone();
                let seen = Rc::clone(&seen);
                Box(
                    Modifier::empty()
                        .modifier_local_consumer(move |scope| seen.set(Some(*scope.get(&key)))),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
    });
    assert_eq!(
        read.get(),
        Some(7),
        "the child made in its provider's pass read the default, not its parent's value"
    );
}
