use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{MutableState, NodeId};

use super::*;
use crate::{
    Box, BoxSpec, LazyColumn, LazyColumnSpec, Modifier, TestComposition, composable,
    run_test_composition,
};

fn may_be_live(composition: &mut TestComposition, count: bool) -> bool {
    let mut applier = composition.applier_mut();
    modal_node_may_be_live(&mut applier, count)
}

fn counting_contexts() -> usize {
    COUNTING_CONTEXTS.with(Cell::get)
}

fn toggled(modal: MutableState<bool>) -> Modifier {
    let is_modal = modal.get();
    Modifier::empty()
        .size_points(40.0, 40.0)
        .semantics(move |config| config.is_modal = is_modal)
}

#[composable]
fn ToggledBox(modal: MutableState<bool>) {
    Box(toggled(modal), BoxSpec::default(), || {});
}

#[composable]
fn ToggledLazyColumn(modal: MutableState<bool>) {
    let state = cranpose_foundation::lazy::rememberLazyListState();
    LazyColumn(toggled(modal), state, LazyColumnSpec::default(), |_| {});
}

#[composable]
fn ComposedWhile(modal: MutableState<bool>) {
    if modal.get() {
        Box(
            Modifier::empty().semantics(|config| config.is_modal = true),
            BoxSpec::default(),
            || {},
        );
    }
}

/// A composition of `widget` driven by a remembered flag, starting unset.
fn composition_with(widget: fn(MutableState<bool>)) -> (TestComposition, MutableState<bool>) {
    let slot = Rc::new(RefCell::new(None));
    let remembered = Rc::clone(&slot);
    let composition = run_test_composition(move || {
        let modal = cranpose_core::rememberMutableStateOf(|| false);
        *remembered.borrow_mut() = Some(modal);
        widget(modal);
    });
    let modal = slot.borrow().expect("state remembered");
    (composition, modal)
}

fn assert_count_follows(widget: fn(MutableState<bool>)) {
    let (mut composition, modal) = composition_with(widget);
    assert!(
        !may_be_live(&mut composition, true),
        "counted with no modal"
    );
    modal.set_value(true);
    composition.process_invalid_scopes().expect("recompose");
    assert!(may_be_live(&mut composition, true), "the modal is counted");
    modal.set_value(false);
    composition.process_invalid_scopes().expect("recompose");
    assert!(!may_be_live(&mut composition, true), "the modal is gone");
}

#[test]
fn the_count_follows_a_layout_nodes_modal_modifier() {
    assert_count_follows(ToggledBox);
}

#[test]
fn the_count_follows_a_subcompose_nodes_modal_modifier() {
    assert_count_follows(ToggledLazyColumn);
}

#[test]
fn the_count_follows_a_modal_node_in_and_out_of_the_tree() {
    assert_count_follows(ComposedWhile);
}

fn plain_box(modifier: Modifier) -> NodeId {
    Box(modifier, BoxSpec::default(), || {})
}

fn lazy_column(modifier: Modifier) -> NodeId {
    let state = cranpose_foundation::lazy::rememberLazyListState();
    LazyColumn(modifier, state, LazyColumnSpec::default(), |_| {})
}

/// A node whose semantics read a flag no recomposition follows is read
/// again once marked for semantics, as a semantics requester marks it.
fn assert_marking_reads_again(node: fn(Modifier) -> NodeId) {
    let flag = Rc::new(Cell::new(false));
    let id = Rc::new(Cell::new(None));
    let (read, slot) = (Rc::clone(&flag), Rc::clone(&id));
    let mut composition = run_test_composition(move || {
        let read = Rc::clone(&read);
        slot.set(Some(node(
            Modifier::empty().semantics(move |config| config.is_modal = read.get()),
        )));
    });
    let id = id.get().expect("node id");
    assert!(!may_be_live(&mut composition, true));
    flag.set(true);
    assert!(
        !may_be_live(&mut composition, true),
        "an unmarked node is not read again"
    );
    cranpose_core::Applier::get_mut(&mut *composition.applier_mut(), id)
        .expect("node")
        .mark_needs_semantics();
    assert!(
        may_be_live(&mut composition, true),
        "a marked node is read again"
    );
}

#[test]
fn a_layout_node_marked_for_semantics_is_read_again() {
    assert_marking_reads_again(plain_box);
}

#[test]
fn a_subcompose_node_marked_for_semantics_is_read_again() {
    assert_marking_reads_again(lazy_column);
}

#[test]
fn without_a_count_a_modal_may_always_be_live() {
    let (mut composition, modal) = composition_with(ToggledBox);
    assert!(may_be_live(&mut composition, false));
    assert_eq!(counting_contexts(), 0);
    assert!(!may_be_live(&mut composition, true));
    assert_eq!(counting_contexts(), 1);
    assert!(may_be_live(&mut composition, false), "stopping forgets");
    assert_eq!(counting_contexts(), 0);
    modal.set_value(true);
    composition.process_invalid_scopes().expect("recompose");
    assert!(
        may_be_live(&mut composition, true),
        "restarting reads every node again"
    );
    drop(composition);
    assert_eq!(counting_contexts(), 0, "a dropped context stops counting");
}
