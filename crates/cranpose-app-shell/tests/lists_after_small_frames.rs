//! A screen that names hundreds of nodes in one frame, after many frames that
//! each name one, updates its scene as a scene built from scratch would be.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_macros::composable;
use cranpose_ui::{Box, BoxSpec, Column, ColumnSpec, Modifier, Row, RowSpec};
use cranpose_ui_graphics::Color;

mod support;
use support::{checking_shell, signature_of};

const ROWS: usize = 24;
const PER_ROW: usize = 30;
// Frames that change one box, well past the frames after which the shell's
// lists give back the room the first frame took.
const SMALL_FRAMES: usize = 60;

type Captured = Rc<RefCell<Option<(MutableState<bool>, MutableState<f32>)>>>;

#[composable]
fn Wall(captured: Captured) {
    let shown = rememberMutableStateOf(|| true);
    let width = rememberMutableStateOf(|| 4.0f32);
    *captured.borrow_mut() = Some((shown, width));
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Box(
            Modifier::empty()
                .size_points(width.get(), 2.0)
                .background(Color::BLACK),
            BoxSpec::default(),
            || {},
        );
        if shown.get() {
            for row in 0..ROWS {
                Row(Modifier::empty(), RowSpec::default(), move || {
                    for column in 0..PER_ROW {
                        let shade = (row * PER_ROW + column) as f32 / (ROWS * PER_ROW) as f32;
                        Box(
                            Modifier::empty().size_points(3.0, 3.0).background(Color(
                                shade,
                                1.0 - shade,
                                0.5,
                                1.0,
                            )),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                });
            }
        }
    });
}

#[test]
fn a_large_frame_after_many_small_ones_updates_as_a_fresh_scene() {
    let captured: Captured = Rc::new(RefCell::new(None));
    let (mut shell, checks) = checking_shell(true, {
        let captured = Rc::clone(&captured);
        move || Wall(Rc::clone(&captured))
    });
    shell.update();
    let first = without_node_ids(signature_of(shell.primary().scene().root_layer()));
    let rebuilds = checks.rebuilds.get();
    let (shown, width) = (*captured.borrow()).expect("the wall is composed");
    for frame in 0..SMALL_FRAMES {
        width.set(5.0 + (frame % 2) as f32);
        shell.update();
    }
    width.set(4.0);
    shown.set(false);
    shell.update();
    shown.set(true);
    shell.update();

    assert_eq!(
        checks.mismatches.get(),
        0,
        "a scoped update drew other than a scene built from scratch"
    );
    assert_eq!(
        checks.rebuilds.get(),
        rebuilds,
        "a frame rebuilt the whole scene instead of updating it"
    );
    assert_eq!(
        without_node_ids(signature_of(shell.primary().scene().root_layer())),
        first,
        "the wall shown again differs from the first frame's"
    );
}

// The scene's lines with the node ids left out: the wall shown again is made
// of new nodes.
fn without_node_ids(lines: Option<Vec<String>>) -> Vec<String> {
    lines
        .into_iter()
        .flatten()
        .map(|line| {
            let mut parts = line.split("NodeId(");
            let mut out = parts.next().unwrap_or_default().to_string();
            for part in parts {
                out.push_str(part.trim_start_matches(|c: char| c.is_ascii_digit()));
            }
            out
        })
        .collect()
}
