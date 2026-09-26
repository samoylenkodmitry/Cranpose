use std::cell::{Cell, RefCell};

use cranpose_core::{
    DisposableEffect, DisposableEffectResult, MutableState, location_key, mutableStateOf, remember,
};
use cranpose_foundation::lazy::rememberLazyListState;
use cranpose_macros::composable;
use cranpose_ui::{LazyColumn, LazyColumnSpec, Modifier, Text, TextStyle};

use super::*;

thread_local! {
    static SCREEN_SHOWN: RefCell<Option<MutableState<bool>>> = const { RefCell::new(None) };
    static DISPOSES: Cell<usize> = const { Cell::new(0) };
}

#[composable]
fn DisposalRoot() {
    let shown = remember(|| mutableStateOf(true)).with(|state| *state);
    SCREEN_SHOWN.with(|slot| *slot.borrow_mut() = Some(shown));
    if shown.get() {
        DisposalScreen();
    } else {
        OtherListScreen();
    }
}

#[composable]
fn OtherListScreen() {
    LazyColumn(
        Modifier::empty().fill_max_size(),
        rememberLazyListState(),
        LazyColumnSpec::new(),
        move |scope| {
            scope.items(20, move |index| {
                Text(
                    format!("Other {index}"),
                    Modifier::empty().height(40.0),
                    TextStyle::default(),
                );
            });
        },
    );
}

#[composable]
fn DisposalScreen() {
    let disposes = remember(|| mutableStateOf(0usize)).with(|state| *state);
    LazyColumn(
        Modifier::empty().fill_max_size(),
        rememberLazyListState(),
        LazyColumnSpec::new(),
        move |scope| {
            scope.items(20, move |index| DisposalItem(index, disposes));
        },
    );
}

#[composable]
fn DisposalItem(index: usize, disposes: MutableState<usize>) {
    DisposableEffect(index, move |_| {
        DisposableEffectResult::new(move || {
            disposes.update(|count| *count += 1);
            DISPOSES.with(|count| count.set(count.get() + 1));
        })
    });
    Text(
        format!("Item {index}"),
        Modifier::empty().height(40.0),
        TextStyle::default(),
    );
}

fn disposal_shell() -> AppShell<TestRenderer> {
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, DisposalRoot);
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    shell.update();
    shell
}

#[test]
fn a_lazy_item_disposal_may_write_state_its_removed_screen_remembered() {
    let _guard = test_guard();
    let mut shell = disposal_shell();
    DISPOSES.with(|count| count.set(0));

    let shown = SCREEN_SHOWN
        .with(|slot| *slot.borrow())
        .expect("the root publishes its switch");
    for visible in [false, true, false, true, false] {
        shown.set(visible);
        for _ in 0..4 {
            shell.update();
        }
    }
    assert!(
        DISPOSES.with(Cell::get) > 0,
        "hiding the screen disposes its visible items"
    );
    drop(shell);
}

#[test]
fn a_lazy_item_disposal_may_write_state_at_final_teardown() {
    let _guard = test_guard();
    let shell = disposal_shell();
    DISPOSES.with(|count| count.set(0));
    drop(shell);
    assert!(
        DISPOSES.with(Cell::get) > 0,
        "dropping the app disposes the visible items"
    );
}

thread_local! {
    static SCROLL_LIST: RefCell<Option<cranpose_foundation::lazy::LazyListState>> = const { RefCell::new(None) };
}

#[composable]
fn SelfWritingList() {
    let list = rememberLazyListState();
    SCROLL_LIST.with(|slot| *slot.borrow_mut() = Some(list));
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list,
        LazyColumnSpec::new(),
        |scope| {
            scope.items(200, SelfWritingItem);
        },
    );
}

#[composable]
fn SelfWritingItem(index: usize) {
    let own = remember(|| mutableStateOf(0usize)).with(|state| *state);
    DisposableEffect(index, move |_| {
        DisposableEffectResult::new(move || own.update(|count| *count += 1))
    });
    Text(
        format!("Row {index}"),
        Modifier::empty().height(40.0),
        TextStyle::default(),
    );
}

#[test]
fn an_item_scrolled_away_may_write_its_own_state_while_it_is_disposed() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, SelfWritingList);
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    shell.update();
    let list = SCROLL_LIST
        .with(|slot| *slot.borrow())
        .expect("the list publishes its state");
    for index in [60, 120, 190, 0] {
        list.scroll_to_item(index, 0.0);
        for _ in 0..3 {
            shell.update();
        }
    }
    drop(shell);
}
