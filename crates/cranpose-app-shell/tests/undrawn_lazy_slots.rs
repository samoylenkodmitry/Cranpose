//! A scrolled lazy list keeps a slot it no longer shows for reuse, until a
//! row of its kind comes into view. Its content still changes with the state
//! it reads, though the scene draws none of it. A scene update has to leave
//! those nodes out, not build the whole scene again.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_macros::composable;
use cranpose_render_common::graph_scene::Scene;
use cranpose_ui::{
    Canvas, Column, ColumnSpec, LazyColumn, LazyColumnSpec, Modifier, Text, TextStyle,
};
use cranpose_ui_graphics::{Brush, Color, Rect};

mod support;
use support::CheckingRenderer;

type Captured = Rc<RefCell<Option<(LazyListState, MutableState<usize>)>>>;

#[composable]
fn TickingList(captured: Captured) {
    let state = rememberLazyListState();
    let tick = rememberMutableStateOf(|| 0usize);
    *captured.borrow_mut() = Some((state, tick));
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            Text(
                "Header".to_string(),
                Modifier::empty().fill_max_width().height(40.0),
                TextStyle::default(),
            );
            LazyColumn(
                Modifier::empty().fill_max_width().weight(1.0),
                state,
                LazyColumnSpec::default(),
                move |scope| {
                    let rows = LazyItems::new(200)
                        .key(|row| row as u64)
                        .content_type(|row| u64::from(row % 10 == 0));
                    scope.items(rows, move |row| {
                        if row % 10 == 0 {
                            TickingBanner(row, tick);
                        } else {
                            TickingRow(row, tick);
                        }
                    });
                },
            );
        },
    );
}

/// A row that reads the tick in a scope of its own, below its item's.
#[composable]
fn TickingRow(row: usize, tick: MutableState<usize>) {
    Text(
        format!("Row {row} at tick {}", tick.get()),
        Modifier::empty().fill_max_width().height(40.0),
        TextStyle::default(),
    );
}

/// A rarer kind of row: the list keeps one it scrolled past for reuse while
/// only rows of the other kind come into view.
#[composable]
fn TickingBanner(row: usize, tick: MutableState<usize>) {
    Text(
        format!("Banner {row}"),
        Modifier::empty().fill_max_width().height(30.0),
        TextStyle::default(),
    );
    Canvas(
        Modifier::empty().fill_max_width().height(30.0),
        move |scope| {
            let width = scope.size().width * (tick.get() % 10 + 1) as f32 / 10.0;
            scope.draw_rect_at(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width,
                    height: 30.0,
                },
                Brush::solid(Color::BLACK),
            );
        },
    );
}

#[test]
fn a_list_scrolling_past_changing_rows_updates_its_scene_without_rebuilding_it() {
    let captured: Captured = Rc::new(RefCell::new(None));
    let rebuilds = Rc::new(Cell::new(0));
    let mismatches = Rc::new(Cell::new(0));
    let mut shell = AppShell::new_with_size(
        CheckingRenderer {
            scene: Scene::new(),
            rebuilds: Rc::clone(&rebuilds),
            mismatches: Some(Rc::clone(&mismatches)),
        },
        location_key(file!(), line!(), column!()),
        {
            let captured = Rc::clone(&captured);
            move || TickingList(Rc::clone(&captured))
        },
        (320, 240),
        (320.0, 240.0),
    );
    shell.update();
    let (state, tick) = (*captured.borrow()).expect("the list is composed");
    for _ in 0..4 {
        let _ = state.dispatch_scroll_delta(-100.0);
        shell.update();
    }

    let rebuilds_before = rebuilds.get();
    for frame in 1..=20 {
        tick.set(frame);
        let _ = state.dispatch_scroll_delta(-30.0);
        shell.update();
    }

    assert_eq!(
        rebuilds.get(),
        rebuilds_before,
        "rows the list composes but does not draw rebuilt the whole scene"
    );
    assert_eq!(
        mismatches.get(),
        0,
        "a scoped update drew other than a scene built from scratch"
    );
}
