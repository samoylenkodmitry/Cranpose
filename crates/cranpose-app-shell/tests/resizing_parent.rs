//! A container that resizes every frame, as a screen whose width animates,
//! keeps the scene layers of the children that did not change: their draws
//! run once, and the scene matches one built from scratch. A child that
//! becomes the root of its own window leaves the resized container's scene,
//! and comes back into it when it docks again.

use std::{
    any::Any,
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_macros::composable;
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, Modifier, Row, RowSpec, Text, TextStyle, WindowRootDescriptor,
};
use cranpose_ui_graphics::{Brush, Color, Rect, Size};

mod support;
use support::checking_shell;

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
    let (mut shell, checks) = checking_shell(check, {
        let captured = Rc::clone(&captured);
        let draws = draws.clone();
        move || ResizingScreen(Rc::clone(&captured), draws.clone())
    });
    shell.update();
    let width = (*captured.borrow()).expect("the screen is composed");
    // The first update after the scene is built refreshes every node once.
    width.set(199.0);
    shell.update();
    let (rebuilds_before, draws_before) = (checks.rebuilds.get(), draws.0.get());
    for frame in 1..=frames {
        width.set(200.0 + frame as f32 * 1.5);
        shell.update();
    }
    (
        checks.rebuilds.get() - rebuilds_before,
        checks.mismatches.get(),
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

type TornState = Rc<RefCell<Option<MutableState<bool>>>>;

/// The window a torn pane opens: the pane's own size.
#[derive(PartialEq)]
struct PaneWindow;

impl WindowRootDescriptor for PaneWindow {
    fn layout_size(&self) -> Size {
        Size::new(90.0, 30.0)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A stack of a fixed bar and a pane, as a player whose equalizer tears off
/// into its own window: the stack is as tall as the panes it holds.
#[composable]
fn DockingStack(torn: TornState, window: Rc<PaneWindow>) {
    let is_torn = rememberMutableStateOf(|| false);
    *torn.borrow_mut() = Some(is_torn);
    let pane = if is_torn.get() {
        Modifier::empty().window_root(window)
    } else {
        Modifier::empty()
    };
    Column(
        Modifier::empty().width(90.0),
        ColumnSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .size_points(90.0, 30.0)
                    .background(Color::BLACK),
                BoxSpec::default(),
                || {},
            );
            Box(pane.clone(), BoxSpec::default(), || {
                Box(
                    Modifier::empty()
                        .size_points(90.0, 30.0)
                        .background(Color::WHITE),
                    BoxSpec::default(),
                    || {},
                );
            });
        },
    );
}

#[test]
fn a_resizing_container_draws_no_child_torn_into_its_own_window() {
    let torn: TornState = Rc::new(RefCell::new(None));
    let (mut shell, checks) = checking_shell(true, {
        let torn = Rc::clone(&torn);
        let window = Rc::new(PaneWindow);
        move || DockingStack(Rc::clone(&torn), Rc::clone(&window))
    });
    shell.update();
    let is_torn = (*torn.borrow()).expect("the stack is composed");
    let rebuilds_before = checks.rebuilds.get();

    is_torn.set(true);
    shell.update();
    assert_eq!(
        checks.mismatches.get(),
        0,
        "the stack still draws the pane torn into its own window"
    );

    is_torn.set(false);
    shell.update();
    assert_eq!(
        checks.mismatches.get(),
        0,
        "the stack does not draw the pane docked back into it"
    );
    assert_eq!(
        checks.rebuilds.get(),
        rebuilds_before,
        "tearing or docking the pane rebuilt the whole scene"
    );
}
