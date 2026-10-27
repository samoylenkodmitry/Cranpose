// Arrow keys and a remote's D-pad move focus to the nearest target in their
// direction on any screen, by the rules of Compose's two-dimensional focus
// search, after the focused control and its ancestors had the key.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{MutableState, key, mutableStateOf, remember};
use cranpose_testing::{RobotTestRule, TestRenderer, create_headless_robot_test};
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, FocusRequester, Key, KeyEvent, LazyColumn, LazyColumnSpec,
    LazyItems, LazyListScope, LazyRow, LazyRowSpec, Modifier, Row, RowSpec, Size,
    rememberLazyListState,
};

type Focused = Rc<RefCell<Option<&'static str>>>;

// A focusable box of `size` at the layout's place, that writes its name
// into `focused` when it takes focus.
fn target(name: &'static str, size: Size, focused: &Focused) -> Modifier {
    let focused = Rc::clone(focused);
    Modifier::empty().size(size).on_focus_changed(move |state| {
        if state.is_focused() {
            *focused.borrow_mut() = Some(name);
        }
    })
}

// A 60 by 40 target that keeps its identity by its name, so removing one
// cell before it leaves it the same node.
fn cell(name: &'static str, focused: &Focused) {
    key(name, || {
        Box(
            target(name, Size::new(60.0, 40.0), focused),
            BoxSpec::default(),
            || {},
        );
    });
}

struct Screen {
    robot: RobotTestRule<TestRenderer>,
    focused: Focused,
}

impl Screen {
    fn new(content: impl Fn(&Focused) + 'static) -> Self {
        let focused = Focused::default();
        let built = Rc::clone(&focused);
        let mut robot = create_headless_robot_test(400, 400, move || content(&built));
        robot.wait_for_idle();
        Self { robot, focused }
    }

    // Sends one press of `key`, repeated `repeat_count` times, without
    // letting a frame run. Answers whether the shell took it.
    fn press_held(&mut self, key: Key, repeat_count: u32) -> bool {
        let shell = self.robot.shell_mut();
        let event = KeyEvent::key_down(key, "").with_repeat_count(repeat_count);
        shell.on_key_event(&event)
    }

    fn press(&mut self, key: Key) -> bool {
        let taken = self.press_held(key, 0);
        self.robot.wait_for_idle();
        taken
    }

    fn focused(&self) -> Option<&'static str> {
        *self.focused.borrow()
    }
}

fn grid(focused: &Focused) {
    let focused = Rc::clone(focused);
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        for row in [["a1", "a2", "a3"], ["b1", "b2", "b3"], ["c1", "c2", "c3"]] {
            let focused = Rc::clone(&focused);
            Row(Modifier::empty(), RowSpec::default(), move || {
                for name in row {
                    cell(name, &focused);
                }
            });
        }
    });
}

#[test]
fn arrows_walk_a_grid_and_stop_at_its_edges() {
    let mut screen = Screen::new(grid);
    assert!(screen.press(Key::DirectionDown));
    assert_eq!(screen.focused(), Some("a1"), "Down enters at the top left");
    for (key, expected) in [
        (Key::DirectionRight, "a2"),
        (Key::DirectionDown, "b2"),
        (Key::DirectionDown, "c2"),
        (Key::DirectionLeft, "c1"),
        (Key::DirectionUp, "b1"),
        (Key::DirectionRight, "b2"),
        (Key::DirectionRight, "b3"),
    ] {
        assert!(screen.press(key), "{key:?} toward {expected}");
        assert_eq!(screen.focused(), Some(expected));
    }
    assert!(
        !screen.press(Key::DirectionRight),
        "past the last column the key goes on to the app"
    );
    assert_eq!(screen.focused(), Some("b3"));
}

#[test]
fn up_or_left_with_nothing_focused_enters_at_the_bottom_right() {
    let mut screen = Screen::new(grid);
    assert!(screen.press(Key::DirectionUp));
    assert_eq!(screen.focused(), Some("c3"));
}

#[test]
fn a_target_level_with_the_focused_one_beats_a_nearer_one_beside_it() {
    let mut screen = Screen::new(|focused| {
        let focused = Rc::clone(focused);
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            let row = Rc::clone(&focused);
            Row(Modifier::empty(), RowSpec::default(), move || {
                cell("start", &row);
                Box(
                    Modifier::empty().size(Size::new(240.0, 40.0)),
                    BoxSpec::default(),
                    || {},
                );
                cell("level", &row);
            });
            let below = Rc::clone(&focused);
            Row(Modifier::empty(), RowSpec::default(), move || {
                Box(
                    Modifier::empty().size(Size::new(70.0, 40.0)),
                    BoxSpec::default(),
                    || {},
                );
                cell("near", &below);
            });
        });
    });
    assert!(screen.press(Key::DirectionDown));
    assert_eq!(screen.focused(), Some("start"));
    assert!(screen.press(Key::DirectionRight));
    assert_eq!(
        screen.focused(),
        Some("level"),
        "Right keeps to the row it starts in"
    );
}

#[test]
fn down_from_a_wide_banner_takes_the_card_under_its_middle() {
    let mut screen = Screen::new(|focused| {
        let focused = Rc::clone(focused);
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            Box(
                target("banner", Size::new(300.0, 80.0), &focused),
                BoxSpec::default(),
                || {},
            );
            let cards = Rc::clone(&focused);
            Row(Modifier::empty(), RowSpec::default(), move || {
                for name in ["card 1", "card 2", "card 3", "card 4", "card 5"] {
                    cell(name, &cards);
                }
            });
        });
    });
    assert!(screen.press(Key::DirectionDown));
    assert_eq!(screen.focused(), Some("banner"));
    assert!(screen.press(Key::DirectionDown));
    assert_eq!(screen.focused(), Some("card 3"));
}

#[test]
fn an_arrow_after_the_focused_target_went_away_starts_where_it_was() {
    let shown: Rc<RefCell<Option<MutableState<bool>>>> = Rc::default();
    let captured = Rc::clone(&shown);
    let mut screen = Screen::new(move |focused| {
        let state = remember(|| mutableStateOf(true)).with(|state| *state);
        captured.replace(Some(state));
        let focused = Rc::clone(focused);
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            for name in ["one", "two"] {
                cell(name, &focused);
            }
            if state.get() {
                cell("three", &focused);
            }
            for name in ["four", "five"] {
                cell(name, &focused);
            }
        });
    });
    for _ in 0..3 {
        assert!(screen.press(Key::DirectionDown));
    }
    assert_eq!(screen.focused(), Some("three"));

    shown.borrow().expect("composed").set(false);
    screen.robot.wait_for_idle();
    assert!(screen.press(Key::DirectionDown));
    assert_eq!(
        screen.focused(),
        Some("four"),
        "the arrow takes the target that moved into the removed one's place"
    );
    assert!(screen.press(Key::DirectionDown));
    assert_eq!(screen.focused(), Some("five"));
}

#[test]
fn an_arrow_after_the_focused_target_went_away_searches_from_its_place() {
    let shown: Rc<RefCell<Option<MutableState<bool>>>> = Rc::default();
    let captured = Rc::clone(&shown);
    let mut screen = Screen::new(move |focused| {
        let state = remember(|| mutableStateOf(true)).with(|state| *state);
        captured.replace(Some(state));
        let focused = Rc::clone(focused);
        Row(Modifier::empty(), RowSpec::default(), move || {
            let column = Rc::clone(&focused);
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                for name in ["one", "two", "three"] {
                    cell(name, &column);
                }
            });
            let column = Rc::clone(&focused);
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                Box(
                    Modifier::empty().size(Size::new(60.0, 40.0)),
                    BoxSpec::default(),
                    || {},
                );
                if state.get() {
                    cell("beside two", &column);
                }
            });
        });
    });
    for key in [Key::DirectionDown, Key::DirectionDown, Key::DirectionRight] {
        assert!(screen.press(key));
    }
    assert_eq!(screen.focused(), Some("beside two"));

    shown.borrow().expect("composed").set(false);
    screen.robot.wait_for_idle();
    assert!(screen.press(Key::DirectionLeft));
    assert_eq!(
        screen.focused(),
        Some("two"),
        "Left from the removed target's place, not from the bottom right"
    );
}

#[test]
fn a_key_handler_takes_an_arrow_before_focus_moves() {
    let requester = FocusRequester::new();
    let built = requester.clone();
    let mut screen = Screen::new(move |focused| {
        let requester = built.clone();
        let focused = Rc::clone(focused);
        Row(
            Modifier::empty().on_key_event(|event| event.key() == Key::DirectionLeft),
            RowSpec::default(),
            move || {
                cell("left", &focused);
                Box(
                    target("player", Size::new(60.0, 40.0), &focused).focus_requester(&requester),
                    BoxSpec::default(),
                    || {},
                );
                cell("right", &focused);
            },
        );
    });
    requester.request_focus().expect("the player is composed");
    screen.robot.wait_for_idle();

    assert!(screen.press(Key::DirectionLeft), "the row seeks with Left");
    assert_eq!(screen.focused(), Some("player"));
    assert!(screen.press(Key::DirectionRight));
    assert_eq!(screen.focused(), Some("right"), "Right is left to focus");
}

fn four_cells(focused: &Focused) {
    let focused = Rc::clone(focused);
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        for name in ["one", "two", "three", "four"] {
            cell(name, &focused);
        }
    });
}

#[test]
fn a_held_arrow_moves_focus_once_a_frame() {
    let mut screen = Screen::new(four_cells);
    assert!(screen.press(Key::DirectionDown));
    assert_eq!(screen.focused(), Some("one"));

    assert!(screen.press_held(Key::DirectionDown, 1));
    assert!(
        screen.press_held(Key::DirectionDown, 2),
        "a repeat before the frame is taken and dropped"
    );
    screen.robot.wait_for_idle();
    assert_eq!(screen.focused(), Some("two"));

    assert!(screen.press_held(Key::DirectionDown, 3));
    assert!(
        screen.press_held(Key::DirectionDown, 0),
        "a new press moves even before the frame"
    );
    screen.robot.wait_for_idle();
    assert_eq!(screen.focused(), Some("four"));
}

// Android draws its frames with `update_without_frame`, which runs no frame
// callbacks and leaves the frame clock where it was. A held D-pad key still
// moves once each frame drawn: on the Mi TV Stick a held Right moved focus
// once and then took 40 repeats without moving.
#[test]
fn a_held_arrow_moves_once_each_frame_drawn_without_the_frame_clock() {
    let mut screen = Screen::new(four_cells);
    assert!(screen.press(Key::DirectionDown));
    assert_eq!(screen.focused(), Some("one"));
    for (repeat, name) in [(1, "two"), (2, "three"), (3, "four")] {
        assert!(screen.press_held(Key::DirectionDown, repeat));
        screen.robot.shell_mut().update_without_frame();
        assert_eq!(screen.focused(), Some(name), "after repeat {repeat}");
    }
}

// A row of 60 by 40 cells named by `names`, with the `group` modifier.
fn row_of(names: &'static [&'static str], group: Modifier, focused: &Focused) {
    let focused = Rc::clone(focused);
    Row(group, RowSpec::default(), move || {
        for name in names {
            cell(name, &focused);
        }
    });
}

#[test]
fn an_arrow_searches_its_focus_group_before_the_screen() {
    let mut screen = Screen::new(|focused| {
        let focused = Rc::clone(focused);
        Box(Modifier::empty(), BoxSpec::default(), move || {
            let inside = Rc::clone(&focused);
            Row(
                Modifier::empty().focus_group(),
                RowSpec::default(),
                move || {
                    cell("first", &inside);
                    Box(
                        Modifier::empty().size(Size::new(180.0, 40.0)),
                        BoxSpec::default(),
                        || {},
                    );
                    cell("far", &inside);
                },
            );
            Box(
                target("between", Size::new(60.0, 40.0), &focused).offset(120.0, 0.0),
                BoxSpec::default(),
                || {},
            );
        });
    });
    assert!(screen.press(Key::DirectionDown));
    assert_eq!(screen.focused(), Some("first"));
    assert!(screen.press(Key::DirectionRight));
    assert_eq!(
        screen.focused(),
        Some("far"),
        "Right stays in the group though a control outside it lies nearer"
    );
    assert!(screen.press(Key::DirectionLeft));
    assert_eq!(
        screen.focused(),
        Some("first"),
        "Left from the far cell skips the control outside the group too"
    );
}

#[test]
fn a_restoring_row_gives_focus_back_to_the_cell_it_had() {
    let mut screen = Screen::new(|focused| {
        let focused = Rc::clone(focused);
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            row_of(
                &["a1", "a2", "a3", "a4"],
                Modifier::empty().focus_restorer(None),
                &focused,
            );
            row_of(
                &["b1", "b2", "b3", "b4"],
                Modifier::empty().focus_restorer(None),
                &focused,
            );
        });
    });
    for key in [Key::DirectionDown, Key::DirectionRight, Key::DirectionRight] {
        assert!(screen.press(key));
    }
    assert_eq!(screen.focused(), Some("a3"));
    assert!(screen.press(Key::DirectionDown));
    assert_eq!(
        screen.focused(),
        Some("b3"),
        "the first entry follows the arrow"
    );
    for _ in 0..2 {
        assert!(screen.press(Key::DirectionLeft));
    }
    assert_eq!(screen.focused(), Some("b1"));
    assert!(screen.press(Key::DirectionUp));
    assert_eq!(
        screen.focused(),
        Some("a3"),
        "the upper row gives focus back to the cell it had, not the one above b1"
    );
}

#[test]
fn a_restoring_row_entered_the_first_time_gives_focus_to_its_fallback() {
    let built = FocusRequester::new();
    let mut screen = Screen::new(move |focused| {
        let focused = Rc::clone(focused);
        let fallback = built.clone();
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            cell("above", &focused);
            let row = Rc::clone(&focused);
            let requester = fallback.clone();
            Row(
                Modifier::empty().focus_restorer(Some(fallback.clone())),
                RowSpec::default(),
                move || {
                    cell("left", &row);
                    Box(
                        target("chosen", Size::new(60.0, 40.0), &row).focus_requester(&requester),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        });
    });
    for key in [Key::DirectionDown, Key::DirectionDown] {
        assert!(screen.press(key));
    }
    assert_eq!(
        screen.focused(),
        Some("chosen"),
        "Down from above reaches the left cell, and the row gives focus to its fallback"
    );
}

#[test]
fn rows_of_lazy_cards_walk_past_their_viewports_and_keep_their_place() {
    let focused: Rc<RefCell<Option<(usize, usize)>>> = Rc::default();
    let seen = Rc::clone(&focused);
    let mut robot = create_headless_robot_test(400, 300, move || {
        let seen = Rc::clone(&seen);
        LazyColumn(
            Modifier::empty().size(Size::new(400.0, 300.0)),
            rememberLazyListState(),
            LazyColumnSpec::default(),
            move |scope| {
                let seen = Rc::clone(&seen);
                scope.items(LazyItems::new(20), move |row| {
                    let seen = Rc::clone(&seen);
                    LazyRow(
                        Modifier::empty()
                            .size(Size::new(400.0, 100.0))
                            .focus_restorer(None),
                        rememberLazyListState(),
                        LazyRowSpec::default(),
                        move |scope| {
                            let seen = Rc::clone(&seen);
                            scope.items(LazyItems::new(20), move |card| {
                                let seen = Rc::clone(&seen);
                                Box(
                                    Modifier::empty()
                                        .size(Size::new(120.0, 90.0))
                                        .on_focus_changed(move |state| {
                                            if state.is_focused() {
                                                *seen.borrow_mut() = Some((row, card));
                                            }
                                        }),
                                    BoxSpec::default(),
                                    || {},
                                );
                            });
                        },
                    );
                });
            },
        );
    });
    robot.wait_for_idle();
    let mut press = |key| {
        let taken = robot.shell_mut().on_key_event(&KeyEvent::key_down(key, ""));
        robot.shell_mut().update();
        assert!(taken, "{key:?}");
        *focused.borrow()
    };
    press(Key::DirectionDown);
    for _ in 0..5 {
        press(Key::DirectionRight);
    }
    assert_eq!(
        *focused.borrow(),
        Some((0, 5)),
        "Right walks past the three cards the row shows"
    );
    press(Key::DirectionDown);
    assert_eq!(focused.borrow().map(|(row, _)| row), Some(1));
    press(Key::DirectionUp);
    assert_eq!(
        *focused.borrow(),
        Some((0, 5)),
        "the first row gives focus back to the card it had"
    );
    for _ in 0..4 {
        press(Key::DirectionDown);
    }
    assert_eq!(
        focused.borrow().map(|(row, _)| row),
        Some(4),
        "Down walks past the three rows the column shows"
    );
}

#[test]
fn a_dpad_window_gives_focus_to_a_target_after_each_layout() {
    use cranpose_app_shell::AppShell;
    use cranpose_core::location_key;

    let shown: Rc<RefCell<Option<MutableState<bool>>>> = Rc::default();
    let captured = Rc::clone(&shown);
    let focused = Focused::default();
    let seen = Rc::clone(&focused);
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let state = remember(|| mutableStateOf(true)).with(|state| *state);
            captured.replace(Some(state));
            let focused = Rc::clone(&seen);
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                for name in ["one", "two"] {
                    cell(name, &focused);
                }
                if state.get() {
                    cell("three", &focused);
                }
                cell("four", &focused);
            });
        },
    );
    shell.set_viewport(200.0, 200.0);
    shell.set_buffer_size(200, 200);
    shell.set_dpad_navigation(true);
    shell.update();
    assert_eq!(
        *focused.borrow(),
        Some("one"),
        "the first layout focuses a target"
    );

    for _ in 0..2 {
        assert!(shell.on_key_event(&KeyEvent::key_down(Key::DirectionDown, "")));
        shell.update();
    }
    assert_eq!(*focused.borrow(), Some("three"));
    shown.borrow().expect("composed").set(false);
    shell.update();
    assert_eq!(
        *focused.borrow(),
        Some("four"),
        "the target that took the removed one's place takes focus"
    );
}

#[test]
fn an_arrow_with_no_target_that_way_scrolls_the_page_by_half() {
    use std::cell::Cell;

    use cranpose_ui::ScrollState;

    let held: Rc<Cell<Option<ScrollState>>> = Rc::default();
    let keep = Rc::clone(&held);
    let mut screen = Screen::new(move |focused| {
        let focused = Rc::clone(focused);
        let scroll = remember(|| ScrollState::new(0.0)).with(|scroll| *scroll);
        keep.set(Some(scroll));
        Column(
            Modifier::empty()
                .size(Size::new(400.0, 400.0))
                .vertical_scroll(scroll, false),
            ColumnSpec::default(),
            move || {
                cell("top", &focused);
                // A long text with nothing to focus.
                Box(
                    Modifier::empty().size(Size::new(400.0, 1200.0)),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
    });
    let scroll = held.get().expect("the page composed");
    assert!(screen.press(Key::DirectionDown));
    assert_eq!(screen.focused(), Some("top"));
    assert!(screen.press(Key::DirectionDown), "Down scrolls the page");
    assert_eq!(scroll.value(), 200.0, "by half the 400 high viewport");
    for _ in 0..4 {
        screen.press(Key::DirectionDown);
    }
    assert_eq!(scroll.value(), 840.0, "to the end of the 1240 high content");
    assert!(
        !screen.press(Key::DirectionDown),
        "at the end Down has nothing to do"
    );
    assert!(screen.press(Key::DirectionUp), "Up scrolls back");
    assert_eq!(scroll.value(), 640.0);
    assert_eq!(screen.focused(), Some("top"), "focus stays where it was");
}
