//! A container that resizes every frame, as a screen whose width animates,
//! keeps the scene layers of the children that did not change: their draws
//! run once, and the scene matches one built from scratch.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_macros::composable;
use cranpose_render_common::graph_scene::Scene;
use cranpose_ui::{Box, BoxSpec, Column, ColumnSpec, Modifier, Row, RowSpec, Text, TextStyle};
use cranpose_ui_graphics::{Brush, Color, Rect};

mod support;
use support::CheckingRenderer;

/// A count of draws, equal only to itself, so a composable that takes it
/// skips while nothing else changed.
#[derive(Clone)]
struct DrawCount(Rc<Cell<usize>>);

impl PartialEq for DrawCount {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

type Captured = Rc<RefCell<Option<MutableState<f32>>>>;

#[composable]
fn ResizingScreen(captured: Captured, draws: DrawCount) {
    let width = rememberMutableStateOf(|| 200.0f32);
    *captured.borrow_mut() = Some(width);
    Column(
        Modifier::empty().width(width.get()).padding(4.0),
        ColumnSpec::default(),
        move || {
            let draws = draws.clone();
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::default(),
                move || {
                    Swatch(draws.clone());
                    Text("Fixed label", Modifier::empty(), TextStyle::default());
                },
            );
            Box(
                Modifier::empty()
                    .fill_max_width()
                    .height(12.0)
                    .background(Color::BLACK),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

/// A fixed-size swatch that counts the times its draw runs.
#[composable]
fn Swatch(draws: DrawCount) {
    Box(
        Modifier::empty()
            .size_points(24.0, 24.0)
            .draw_behind(move |scope| {
                draws.0.set(draws.0.get() + 1);
                scope.draw_rect_at(
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 24.0,
                        height: 24.0,
                    },
                    Brush::solid(Color::WHITE),
                );
            }),
        BoxSpec::default(),
        || {},
    );
}

/// Runs the screen through `frames` widths with `renderer`'s checks.
fn resize(check: bool, frames: usize) -> (usize, usize, usize) {
    let captured: Captured = Rc::new(RefCell::new(None));
    let draws = DrawCount(Rc::new(Cell::new(0)));
    let rebuilds = Rc::new(Cell::new(0));
    let mismatches = Rc::new(Cell::new(0));
    let mut shell = AppShell::new_with_size(
        CheckingRenderer {
            scene: Scene::new(),
            rebuilds: Rc::clone(&rebuilds),
            mismatches: check.then(|| Rc::clone(&mismatches)),
        },
        location_key(file!(), line!(), column!()),
        {
            let captured = Rc::clone(&captured);
            let draws = draws.clone();
            move || ResizingScreen(Rc::clone(&captured), draws.clone())
        },
        (320, 240),
        (320.0, 240.0),
    );
    shell.update();
    let width = (*captured.borrow()).expect("the screen is composed");
    // The first update after the scene is built refreshes every node once.
    width.set(199.0);
    shell.update();
    let (rebuilds_before, draws_before) = (rebuilds.get(), draws.0.get());
    for frame in 1..=frames {
        width.set(200.0 + frame as f32 * 1.5);
        shell.update();
    }
    (
        rebuilds.get() - rebuilds_before,
        mismatches.get(),
        draws.0.get() - draws_before,
    )
}

#[test]
fn a_resizing_container_keeps_the_children_that_did_not_change() {
    let (rebuilds, _, draws) = resize(false, 12);
    assert_eq!(rebuilds, 0, "a resize rebuilt the whole scene");
    assert_eq!(
        draws, 0,
        "the fixed swatch drew again while only its parent resized"
    );
}

#[test]
fn a_resizing_container_draws_what_a_scene_built_from_scratch_draws() {
    let (rebuilds, mismatches, _) = resize(true, 12);
    assert_eq!(rebuilds, 0, "a resize rebuilt the whole scene");
    assert_eq!(
        mismatches, 0,
        "a scoped update drew other than a scene built from scratch"
    );
}
