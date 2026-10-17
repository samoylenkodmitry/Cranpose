//! `Box`, `Row` and `Column` compose in their caller's group, as Compose's
//! inline widgets do. The node tree stays as the code nests them while a
//! composable around them skips, and a loop of them that shrinks and grows
//! again keeps the nodes and remembered values of the items that stay.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{Applier, MutableState, NodeId, remember, rememberMutableStateOf};
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, Modifier, TestComposition, Text, TextStyle, composable,
    run_test_composition,
};
use smallvec::SmallVec;

thread_local! {
    /// The outer box, inner box and text the last `Card` pass composed.
    static CARD: Cell<Option<(NodeId, NodeId, NodeId)>> = const { Cell::new(None) };
    static CARD_PASSES: Cell<usize> = const { Cell::new(0) };
}

#[composable]
fn Card(label: String) {
    CARD_PASSES.with(|passes| passes.set(passes.get() + 1));
    let nested = Rc::new(Cell::new(None));
    let seen = Rc::clone(&nested);
    let outer = Box(Modifier::empty(), BoxSpec::default(), move || {
        let text = Rc::new(Cell::new(None));
        let text_seen = Rc::clone(&text);
        let label = label.clone();
        let inner = Box(Modifier::empty(), BoxSpec::default(), move || {
            text_seen.set(Some(Text(
                label.clone(),
                Modifier::empty(),
                TextStyle::default(),
            )));
        });
        seen.set(text.get().map(|text| (inner, text)));
    });
    let (inner, text) = nested.get().expect("the card composed its inner box");
    CARD.with(|card| card.set(Some((outer, inner, text))));
}

fn children(composition: &mut TestComposition, id: NodeId) -> Vec<NodeId> {
    let mut children = SmallVec::new();
    composition
        .applier_mut()
        .get_mut(id)
        .expect("the node is in the tree")
        .collect_children_into(&mut children);
    children.to_vec()
}

type Captured<T> = Rc<RefCell<Option<MutableState<T>>>>;

fn set<T: Clone + 'static>(state: &Captured<T>, value: T) {
    state
        .borrow()
        .as_ref()
        .expect("the composition remembered the state")
        .set(value);
}

#[test]
fn a_skipped_composable_keeps_the_inline_widgets_in_it_under_their_parents() {
    let count: Captured<u32> = Rc::default();
    let captured = Rc::clone(&count);
    let mut composition = run_test_composition(move || {
        let state = rememberMutableStateOf(|| 0u32);
        *captured.borrow_mut() = Some(state);
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            Text(
                format!("Count {}", state.value()),
                Modifier::empty(),
                TextStyle::default(),
            );
            Card("card".to_owned());
        });
    });
    let column = composition.root().expect("the column is the root");
    let (outer, inner, text) = CARD.with(Cell::get).expect("the card composed");
    let passes = CARD_PASSES.with(Cell::get);

    set(&count, 1);
    composition
        .process_invalid_scopes()
        .expect("the column recomposes");

    assert_eq!(
        CARD_PASSES.with(Cell::get),
        passes,
        "the card's argument is the same, so it skips"
    );
    let column_children = children(&mut composition, column);
    assert_eq!(column_children.len(), 2);
    assert_eq!(column_children[1], outer);
    assert_eq!(children(&mut composition, outer), [inner]);
    assert_eq!(children(&mut composition, inner), [text]);
}

#[test]
fn a_loop_of_inline_widgets_keeps_the_items_that_stay_as_it_shrinks_and_grows() {
    let count: Captured<usize> = Rc::default();
    let captured = Rc::clone(&count);
    // Each item remembers the pass that first composed it.
    let items = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&items);
    let passes = Rc::new(Cell::new(0));
    let mut composition = run_test_composition(move || {
        let state = rememberMutableStateOf(|| 3usize);
        *captured.borrow_mut() = Some(state);
        let (seen, pass) = (Rc::clone(&seen), Rc::clone(&passes));
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            seen.borrow_mut().clear();
            for index in 0..state.value() {
                let seen = Rc::clone(&seen);
                let pass = Rc::clone(&pass);
                Box(Modifier::empty(), BoxSpec::default(), move || {
                    let made = remember(|| pass.get()).with(|made| *made);
                    seen.borrow_mut().push((index, made));
                });
            }
            pass.set(pass.get() + 1);
        });
    });
    let column = composition.root().expect("the column is the root");
    let first = children(&mut composition, column);
    assert_eq!(*items.borrow(), [(0, 0), (1, 0), (2, 0)]);

    set(&count, 2);
    composition.process_invalid_scopes().expect("shrinks");
    assert_eq!(children(&mut composition, column), first[..2]);
    assert_eq!(*items.borrow(), [(0, 0), (1, 0)]);

    set(&count, 3);
    composition.process_invalid_scopes().expect("grows");
    let grown = children(&mut composition, column);
    assert_eq!(grown.len(), 3);
    assert_eq!(
        grown[..2],
        first[..2],
        "the items that stayed keep their nodes"
    );
    assert_eq!(
        *items.borrow(),
        [(0, 0), (1, 0), (2, 2)],
        "the items that stayed keep their values; the new one makes its own"
    );
}
