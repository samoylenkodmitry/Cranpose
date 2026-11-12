// A screen that showed hundreds of nodes and hid them again holds, once its
// frames come to rest, about the scene heap of a screen that never showed
// them.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_macros::composable;
use cranpose_ui::{Box, BoxSpec, Column, ColumnSpec, Modifier, Row, RowSpec};
use cranpose_ui_graphics::Color;
use web_time::Instant;

mod support;
use support::checking_shell;

const ROWS: usize = 24;
const PER_ROW: usize = 30;

type Shown = Rc<RefCell<Option<MutableState<bool>>>>;

// A bar above a wall of boxes, shown while `shown` holds.
#[composable]
fn Wall(captured: Shown, first: bool) {
    let shown = rememberMutableStateOf(|| first);
    *captured.borrow_mut() = Some(shown);
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Box(
            Modifier::empty()
                .size_points(4.0, 2.0)
                .background(Color::BLACK),
            BoxSpec::default(),
            || {},
        );
        if shown.get() {
            for _ in 0..ROWS {
                Row(Modifier::empty(), RowSpec::default(), || {
                    for _ in 0..PER_ROW {
                        Box(
                            Modifier::empty()
                                .size_points(3.0, 3.0)
                                .background(Color::WHITE),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                });
            }
        }
    });
}

// The scene heap of a wall shown first when `first` holds, after it hides
// the boxes and comes to rest.
fn heap_at_rest(first: bool) -> usize {
    let captured: Shown = Rc::new(RefCell::new(None));
    let (mut shell, _) = checking_shell(false, {
        let captured = Rc::clone(&captured);
        move || Wall(Rc::clone(&captured), first)
    });
    shell.update();
    let shown = (*captured.borrow()).expect("the wall is composed");
    shown.set(false);
    shell.update();
    // The platform wakes the resting shell at the time of its trim.
    let due = (shell.frame_schedule().next_deadline).expect("a resting shell trims later");
    std::thread::sleep(due.saturating_duration_since(Instant::now()));
    assert!(
        shell.frame_schedule().needs_update,
        "the time of the trim asks for an update"
    );
    shell.update();
    assert!(
        shell.frame_schedule().is_idle(),
        "the trimmed shell asks for nothing"
    );
    shell.primary().scene().store().heap_bytes()
}

#[test]
fn a_wall_shown_and_hidden_holds_at_rest_the_heap_of_one_never_shown() {
    let shown = heap_at_rest(true);
    let never = heap_at_rest(false);
    assert!(
        shown * 100 <= never * 110,
        "the wall shown and hidden holds {shown} bytes, one never shown {never}"
    );
}
