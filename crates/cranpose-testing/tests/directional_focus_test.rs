// Arrow keys and a remote's D-pad move focus to the nearest target in their
// direction on any screen, by the rules of Compose's two-dimensional focus
// search, after the focused control and its ancestors had the key.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{MutableState, key, mutableStateOf, remember};
use cranpose_testing::{RobotTestRule, TestRenderer, create_headless_robot_test};
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, FocusRequester, Key, KeyEvent, Modifier, Row, RowSpec, Size,
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

#[test]
fn a_held_arrow_moves_focus_once_a_frame() {
    let mut screen = Screen::new(|focused| {
        let focused = Rc::clone(focused);
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            for name in ["one", "two", "three", "four"] {
                cell(name, &focused);
            }
        });
    });
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
