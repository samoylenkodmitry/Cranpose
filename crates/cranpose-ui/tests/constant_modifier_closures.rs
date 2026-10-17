//! `#[composable]` keeps modifier chains of constants found in closures, and
//! the constant tail of a chain that starts with a builder reading a
//! variable. A closure may run in the composition, as a widget's content,
//! or outside it: as an event handler, in an effect, in a slot's
//! initializer. Each place builds the modifier the code spells, and the
//! kept chains leave the remembered values around them where they were.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{MutableState, SideEffect, remember, rememberMutableStateOf};
use cranpose_ui::{Box, BoxSpec, Modifier, composable, run_test_composition};

type Captured<T> = Rc<RefCell<Option<MutableState<T>>>>;
type Handler = Rc<dyn Fn() -> Modifier>;

thread_local! {
    /// The click handler the last `Handlers` pass made.
    static HANDLER: RefCell<Option<Handler>> = const { RefCell::new(None) };
}
type Built = Rc<RefCell<Vec<Modifier>>>;

fn set<T: Clone + 'static>(state: &Captured<T>, value: T) {
    state
        .borrow()
        .as_ref()
        .expect("the composition remembered the state")
        .set(value);
}

fn bar(fraction: f32) -> Modifier {
    Modifier::empty()
        .fill_max_width_fraction(fraction)
        .height(16.0)
        .padding(2.0)
}

fn pad() -> Modifier {
    Modifier::empty().padding(1.0).width(2.0)
}

#[composable]
fn Bar(fraction: f32, built: Built) {
    Box(Modifier::empty(), BoxSpec::default(), move || {
        let modifier = Modifier::empty()
            .fill_max_width_fraction(fraction)
            .height(16.0)
            .padding(2.0);
        built.borrow_mut().push(modifier.clone());
        Box(modifier, BoxSpec::default(), || {});
    });
}

#[test]
fn a_content_closure_builds_its_chain_with_a_kept_tail_on_every_pass() {
    let fraction: Captured<f32> = Rc::default();
    let captured = Rc::clone(&fraction);
    let built = Built::default();
    let sink = Rc::clone(&built);
    let mut composition = run_test_composition(move || {
        let state = rememberMutableStateOf(|| 0.25f32);
        *captured.borrow_mut() = Some(state);
        Bar(state.value(), Rc::clone(&sink));
    });
    for next in [0.5, 0.75] {
        set(&fraction, next);
        composition
            .process_invalid_scopes()
            .expect("the bar recomposes");
    }
    let expected: Vec<Modifier> = [0.25, 0.5, 0.75].into_iter().map(bar).collect();
    assert!(
        *built.borrow() == expected,
        "each pass builds the head with that pass's fraction and the same tail"
    );
}

#[composable]
fn Handlers(built: Built) {
    let on_click: Handler = Rc::new(|| Modifier::empty().padding(1.0).width(2.0));
    HANDLER.with(|handler| *handler.borrow_mut() = Some(on_click));
    let initialized = remember(|| Modifier::empty().padding(1.0).width(2.0)).with(Clone::clone);
    built.borrow_mut().push(initialized);
    let effect_sink = Rc::clone(&built);
    SideEffect(move || {
        effect_sink
            .borrow_mut()
            .push(Modifier::empty().padding(1.0).width(2.0));
    });
}

#[test]
fn closures_run_outside_the_composition_build_their_chain() {
    let built = Built::default();
    let sink = Rc::clone(&built);
    let _composition = run_test_composition(move || {
        Handlers(Rc::clone(&sink));
    });
    let on_click = HANDLER
        .with(|handler| handler.borrow().clone())
        .expect("the composable handed out its handler");
    assert!(
        on_click() == pad(),
        "an event handler runs with no composition"
    );
    assert!(
        *built.borrow() == [pad(), pad()],
        "a slot's initializer and a side effect run while the slots take no \
         writes"
    );
}

#[composable]
fn Repeated(count: usize, ids: Rc<RefCell<Vec<(usize, u32)>>>, next: Rc<Cell<u32>>) {
    let built: Vec<Modifier> = (0..count)
        .map(|_| Modifier::empty().padding(1.0).width(2.0))
        .collect();
    let id = remember(|| {
        let id = next.get();
        next.set(id + 1);
        id
    })
    .with(|id| *id);
    assert!(built.iter().all(|modifier| *modifier == pad()));
    ids.borrow_mut().push((built.len(), id));
}

#[test]
fn a_closure_run_more_or_fewer_times_leaves_the_values_after_it_in_place() {
    let count: Captured<usize> = Rc::default();
    let captured = Rc::clone(&count);
    let ids: Rc<RefCell<Vec<(usize, u32)>>> = Rc::default();
    let seen = Rc::clone(&ids);
    let mut composition = run_test_composition(move || {
        let state = rememberMutableStateOf(|| 3usize);
        *captured.borrow_mut() = Some(state);
        let next = remember(|| Rc::new(Cell::new(0u32))).with(Rc::clone);
        Repeated(state.value(), Rc::clone(&seen), next);
    });
    for next in [1, 4, 0, 2] {
        set(&count, next);
        composition
            .process_invalid_scopes()
            .expect("the composable recomposes");
    }
    assert_eq!(
        *ids.borrow(),
        [(3, 0), (1, 0), (4, 0), (0, 0), (2, 0)],
        "the value remembered after the closure's kept chains survives every \
         change in how often the closure runs"
    );
}
