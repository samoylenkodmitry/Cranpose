use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{
    Composition, ComputedState, MemoryApplier, MutableState, computedStateOf, location_key,
    mutableStateOf,
};

fn composition() -> Composition<MemoryApplier> {
    Composition::new(MemoryApplier::new())
}

/// A composable reading `computed`, spelled out the way `#[composable]`
/// expands it: a group whose recompose callback runs the body again.
fn computed_reader(
    computed: ComputedState<bool>,
    reads: Rc<Cell<u32>>,
    seen: Rc<RefCell<Vec<bool>>>,
) {
    fn body(computed: &ComputedState<bool>, reads: &Cell<u32>, seen: &RefCell<Vec<bool>>) {
        reads.set(reads.get() + 1);
        seen.borrow_mut().push(computed.get());
    }
    let key = location_key(file!(), line!(), column!());
    cranpose_core::with_current_composer(|composer| {
        composer.with_group(key, |composer| {
            let again = computed.clone();
            let (again_reads, again_seen) = (Rc::clone(&reads), Rc::clone(&seen));
            composer.set_recompose_callback(move |_| body(&again, &again_reads, &again_seen));
            body(&computed, &reads, &seen);
        });
    });
}

fn big(source: MutableState<u32>, runs: &Rc<Cell<u32>>) -> ComputedState<bool> {
    let runs = Rc::clone(runs);
    computedStateOf(move || {
        runs.set(runs.get() + 1);
        source.get() >= 10
    })
}

#[test]
fn a_computed_state_works_its_value_out_on_every_read() {
    let _composition = composition();
    let source = mutableStateOf(2u32);
    let runs = Rc::new(Cell::new(0));
    let computed = big(source, &runs);
    assert!(!computed.get());
    assert!(!computed.get());
    assert_eq!(runs.get(), 2, "every read runs the calculation");
    source.set(12);
    assert!(
        computed.get(),
        "a read sees the state the calculation reads"
    );
}

#[test]
fn a_computed_state_recomposes_its_readers_only_when_its_value_changes() {
    let mut composition = composition();
    let key = location_key(file!(), line!(), column!());
    let source = mutableStateOf(1u32);
    let runs = Rc::new(Cell::new(0));
    let computed = big(source, &runs);
    let readers = [(); 2].map(|()| (Rc::new(Cell::new(0u32)), Rc::new(RefCell::new(Vec::new()))));
    {
        let readers = readers.clone();
        composition
            .render(key, move || {
                for (reads, seen) in &readers {
                    computed_reader(computed.clone(), Rc::clone(reads), Rc::clone(seen));
                }
            })
            .expect("render succeeds");
    }

    for value in [2, 3, 4, 5] {
        source.set(value);
        let changed = composition
            .process_invalid_scopes()
            .expect("recomposition succeeds");
        assert!(!changed, "an unchanged value recomposes nothing");
    }
    for (reads, _) in &readers {
        assert_eq!(
            reads.get(),
            1,
            "a change that leaves the value alone must not recompose a reader"
        );
    }

    source.set(12);
    let changed = composition
        .process_invalid_scopes()
        .expect("recomposition succeeds");
    assert!(changed, "a changed value recomposes");
    for (reads, seen) in &readers {
        assert_eq!(reads.get(), 2, "a changed value recomposes every reader");
        assert_eq!(*seen.borrow(), vec![false, true]);
    }
}

#[test]
fn a_dropped_computed_state_leaves_writes_to_what_it_read_alone() {
    let mut composition = composition();
    let key = location_key(file!(), line!(), column!());
    let source = mutableStateOf(1u32);
    let runs = Rc::new(Cell::new(0));
    let reads = Rc::new(Cell::new(0u32));
    let seen = Rc::new(RefCell::new(Vec::new()));
    {
        let computed = big(source, &runs);
        let (reads, seen) = (Rc::clone(&reads), Rc::clone(&seen));
        composition
            .render(key, move || {
                computed_reader(computed.clone(), Rc::clone(&reads), Rc::clone(&seen));
            })
            .expect("render succeeds");
    }
    composition
        .render(key, || {})
        .expect("render without the reader succeeds");
    let before = runs.get();
    source.set(20);
    composition
        .process_invalid_scopes()
        .expect("recomposition succeeds");
    assert_eq!(
        runs.get(),
        before,
        "a computed state no one holds is not worked out again"
    );
}

#[test]
fn a_change_undone_before_the_frame_recomposes_nothing() {
    let mut composition = composition();
    let key = location_key(file!(), line!(), column!());
    let source = mutableStateOf(1u32);
    let runs = Rc::new(Cell::new(0));
    let computed = big(source, &runs);
    let reads = Rc::new(Cell::new(0u32));
    let seen = Rc::new(RefCell::new(Vec::new()));
    {
        let (reads, seen) = (Rc::clone(&reads), Rc::clone(&seen));
        composition
            .render(key, move || {
                computed_reader(computed.clone(), Rc::clone(&reads), Rc::clone(&seen));
            })
            .expect("render succeeds");
    }
    source.set(12);
    source.set(3);
    let changed = composition
        .process_invalid_scopes()
        .expect("recomposition succeeds");
    assert!(!changed, "the value the reader read is the value again");
    assert_eq!(reads.get(), 1);
}

#[test]
fn a_computed_state_that_reads_another_follows_what_that_one_reads() {
    let mut composition = composition();
    let key = location_key(file!(), line!(), column!());
    let source = mutableStateOf(1u32);
    let enabled = mutableStateOf(true);
    let runs = Rc::new(Cell::new(0));
    let inner = big(source, &runs);
    let outer = computedStateOf(move || enabled.get() && inner.get());
    let reads = Rc::new(Cell::new(0u32));
    let seen = Rc::new(RefCell::new(Vec::new()));
    {
        let (reads, seen) = (Rc::clone(&reads), Rc::clone(&seen));
        composition
            .render(key, move || {
                computed_reader(outer.clone(), Rc::clone(&reads), Rc::clone(&seen));
            })
            .expect("render succeeds");
    }
    source.set(15);
    composition
        .process_invalid_scopes()
        .expect("recomposition succeeds");
    enabled.set(false);
    composition
        .process_invalid_scopes()
        .expect("recomposition succeeds");
    assert_eq!(*seen.borrow(), vec![false, true, false]);
}
