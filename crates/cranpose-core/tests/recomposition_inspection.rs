use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use cranpose_core::{
    Composition, MemoryApplier, MutableState,
    source_trace::{SourceLocation, current_source_trace, set_recomposition_tracking},
};
use cranpose_macros::composable;

thread_local! {
    static TRACES: RefCell<BTreeMap<u32, Rc<[SourceLocation]>>> = RefCell::default();
}

struct Tracking;
impl Tracking {
    fn enable() -> Self {
        set_recomposition_tracking(true);
        TRACES.with(|traces| traces.borrow_mut().clear());
        Self
    }
}
impl Drop for Tracking {
    fn drop(&mut self) {
        set_recomposition_tracking(false);
        TRACES.with(|traces| traces.borrow_mut().clear());
    }
}

#[composable]
fn Counted(id: u32, state: MutableState<u32>) {
    let _ = state.get();
    let _call = cranpose_core::__source_scope("__cranpose_call:Leaf", file!(), line!(), "test");
    TRACES.with(|traces| traces.borrow_mut().insert(id, current_source_trace()));
}

#[composable]
fn Parent(parent: MutableState<u32>, first: MutableState<u32>, second: MutableState<u32>) {
    let _ = parent.get();
    Counted(1, first);
    Counted(2, second);
}

fn trace(id: u32) -> Rc<[SourceLocation]> {
    TRACES.with(|traces| Rc::clone(traces.borrow().get(&id).expect("instance composed")))
}

fn counts(trace: &[SourceLocation]) -> Vec<Option<u64>> {
    trace.iter().map(SourceLocation::recompositions).collect()
}

fn expected(values: &[Option<u64>]) -> Vec<Option<u64>> {
    values
        .iter()
        .map(|value| if cfg!(debug_assertions) { *value } else { None })
        .collect()
}

#[test]
fn recomposition_inspection_counts_instances_and_keeps_retained_ancestor_counts_live() {
    let _tracking = Tracking::enable();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let parent = MutableState::with_runtime(0, runtime.clone());
    let first = MutableState::with_runtime(0, runtime.clone());
    let second = MutableState::with_runtime(0, runtime);
    let mut render = || Parent(parent, first, second);
    composition
        .render(101, &mut render)
        .expect("initial composition");
    let retained = trace(1);
    assert_eq!(counts(&retained), expected(&[Some(0), Some(0), None]));
    composition
        .render(101, &mut render)
        .expect("unchanged composition");
    assert_eq!(counts(&retained), expected(&[Some(0), Some(0), None]));

    first.set(1);
    composition
        .process_invalid_scopes()
        .expect("first child updates");
    assert_eq!(counts(&retained), expected(&[Some(0), Some(1), None]));
    assert_eq!(counts(&trace(2)), expected(&[Some(0), Some(0), None]));

    parent.set(1);
    composition
        .process_invalid_scopes()
        .expect("parent updates");
    assert_eq!(counts(&retained), expected(&[Some(1), Some(1), None]));
    assert_eq!(counts(&trace(2)), expected(&[Some(1), Some(0), None]));
    for value in 1..=2 {
        second.set(value);
        composition
            .process_invalid_scopes()
            .expect("second child updates");
    }
    assert_eq!(counts(&trace(2)), expected(&[Some(1), Some(2), None]));
    assert_eq!(counts(&retained), expected(&[Some(1), Some(1), None]));
}

#[composable]
fn Conditional(show: MutableState<bool>, value: MutableState<u32>) {
    if show.get() {
        Counted(3, value);
    }
}

#[test]
fn recomposition_inspection_starts_a_new_count_when_an_instance_is_recreated() {
    let _tracking = Tracking::enable();
    let mut composition = Composition::new(MemoryApplier::new());
    let show = MutableState::with_runtime(true, composition.runtime_handle());
    let value = MutableState::with_runtime(0, composition.runtime_handle());
    composition
        .render(102, || Conditional(show, value))
        .expect("initial composition");
    let old = trace(3);
    value.set(1);
    composition.process_invalid_scopes().expect("child updates");
    assert_eq!(old[1].recompositions(), expected(&[Some(1)])[0]);
    show.set(false);
    composition.process_invalid_scopes().expect("child removed");
    show.set(true);
    composition
        .process_invalid_scopes()
        .expect("child recreated");
    assert_eq!(trace(3)[1].recompositions(), expected(&[Some(0)])[0]);
    assert_eq!(old[1].recompositions(), expected(&[Some(1)])[0]);
}

#[test]
fn recomposition_inspection_is_disabled_by_default() {
    set_recomposition_tracking(false);
    let mut composition = Composition::new(MemoryApplier::new());
    let value = MutableState::with_runtime(0, composition.runtime_handle());
    composition
        .render(103, || Counted(4, value))
        .expect("initial composition");
    value.set(1);
    composition.process_invalid_scopes().expect("child updates");
    assert!(
        trace(4)
            .iter()
            .all(|source| source.recompositions().is_none())
    );
    TRACES.with(|traces| traces.borrow_mut().clear());
}
