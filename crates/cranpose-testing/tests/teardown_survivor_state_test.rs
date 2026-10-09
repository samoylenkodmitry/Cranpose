//! Controls that outlive a large teardown keep what the user was doing with
//! them. Leaving a screen of thousands of nodes, as the desktop demo's
//! Recursive Layout tab at depth 12 is, makes the applier move every
//! surviving node onto fresh storage. A field there keeps its focus, its
//! selection and its composition, a button pressed before keeps the press and
//! taps on release, and a scrolled list stays where it was.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, remember, rememberMutableStateOf};
use cranpose_testing::robot::create_headless_robot_test;
use cranpose_ui::{
    BasicTextField, Box, BoxSpec, Column, ColumnSpec, FocusRequester, KeyCode, KeyEvent, Modifier,
    PointerEventKind, PointerInputScope, ScrollState, Size, Spacer, Text, TextFieldState,
    TextStyle,
};

/// More nodes than the slot table drops before it asks the applier to
/// compact its storage.
const DROPPED_NODES: usize = 17 * 1024;
const BUTTON: (f32, f32) = (100.0, 60.0);
const SCROLLED_BY: f32 = 30.0;

#[derive(Clone, Copy)]
struct Handles {
    field: TextFieldState,
    scroll: ScrollState,
    deep: MutableState<bool>,
}

/// Counts a tap for each release of a press the gesture saw go down. The
/// press lives in the gesture's own coroutine, so a button whose gesture
/// started again between the press and the release taps nothing.
fn tap_counter(taps: Rc<Cell<usize>>) -> Modifier {
    Modifier::empty().pointer_input((), move |scope: PointerInputScope| {
        let taps = Rc::clone(&taps);
        async move {
            scope
                .await_pointer_event_scope(|events| async move {
                    let mut pressed = false;
                    loop {
                        match events.await_pointer_event().await.kind {
                            PointerEventKind::Down => pressed = true,
                            PointerEventKind::Up if pressed => {
                                pressed = false;
                                taps.set(taps.get() + 1);
                            }
                            _ => {}
                        }
                    }
                })
                .await;
        }
    })
}

#[test]
fn controls_keep_focus_press_selection_and_scroll_through_a_large_teardown() {
    let handles = Rc::new(Cell::new(None::<Handles>));
    let taps = Rc::new(Cell::new(0usize));
    let requester = FocusRequester::new();
    let content_handles = Rc::clone(&handles);
    let content_taps = Rc::clone(&taps);
    let content_requester = requester.clone();
    let mut robot = create_headless_robot_test(320, 200, move || {
        let handles = Rc::clone(&content_handles);
        let taps = Rc::clone(&content_taps);
        let requester = content_requester.clone();
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            let field = remember(|| TextFieldState::new("Receipt")).with(|state| *state);
            let scroll = remember(|| ScrollState::new(0.0)).with(|state| *state);
            let deep = rememberMutableStateOf(|| true);
            handles.set(Some(Handles {
                field,
                scroll,
                deep,
            }));
            BasicTextField(
                field,
                Modifier::empty()
                    .size(Size::new(200.0, 40.0))
                    .focus_requester(&requester),
                TextStyle::default(),
            );
            Box(
                Modifier::empty()
                    .size_points(200.0, 40.0)
                    .then(tap_counter(Rc::clone(&taps))),
                BoxSpec::default(),
                || {},
            );
            Column(
                Modifier::empty()
                    .size_points(200.0, 60.0)
                    .vertical_scroll(scroll, false),
                ColumnSpec::default(),
                || {
                    for row in 0..10 {
                        Text(
                            format!("Row {row}"),
                            Modifier::empty().height(20.0),
                            TextStyle::default(),
                        );
                    }
                },
            );
            if deep.get() {
                Column(Modifier::empty(), ColumnSpec::default(), || {
                    for _ in 0..DROPPED_NODES {
                        Spacer(Modifier::empty().size_points(4.0, 1.0));
                    }
                });
            }
        });
    });
    robot.wait_for_idle();
    let Handles {
        field,
        scroll,
        deep,
    } = handles.get().expect("the screen composed");

    // Focused as by the Tab key: a field pressed by a pointer opens a menu
    // over its selection, which takes the next press outside it to close.
    requester.request_focus().expect("the field takes focus");
    robot.wait_for_idle();
    let shell = robot.shell_mut();
    assert!(shell.on_ime_set_composing_region(3, 7));
    assert!(shell.on_ime_set_selection(0, 3));
    scroll.dispatch_raw_delta(SCROLLED_BY);
    robot.wait_for_idle();
    let editor = robot.shell_mut().ime_editor_state();
    assert_eq!(
        editor.as_ref().map(|editor| (
            editor.selection_start,
            editor.selection_end,
            editor.composition
        )),
        Some((0, 3, Some((3, 7)))),
        "the field is focused, with its selection and composition"
    );
    assert_eq!(scroll.value(), SCROLLED_BY);
    let row = robot.find_by_text("Row 3").bounds();
    assert!(row.is_some(), "the scrolled list shows its fourth row");

    let shell = robot.shell_mut();
    shell.set_cursor(BUTTON.0, BUTTON.1);
    assert!(shell.pointer_pressed(), "the press reaches the button");
    deep.set(false);
    robot.wait_for_idle();

    assert!(robot.shell_mut().pointer_released());
    robot.wait_for_idle();
    assert_eq!(taps.get(), 1, "releasing the held press taps the button");
    assert_eq!(
        robot.shell_mut().ime_editor_state(),
        editor,
        "the field keeps its focus, its selection and its composition"
    );
    assert_eq!(scroll.value(), SCROLLED_BY, "the list keeps its scroll");
    assert_eq!(
        robot.find_by_text("Row 3").bounds(),
        row,
        "the list shows its rows where it did"
    );

    let shell = robot.shell_mut();
    shell.on_ime_finish_composing();
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::X, "x")));
    assert_eq!(
        field.text(),
        "xeipt",
        "typing goes to the focused field and replaces its selection"
    );
}
