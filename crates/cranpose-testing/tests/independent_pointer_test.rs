// Pointers that act on their own, as the two controllers of a headset do: each has its own place,
// hover, buttons and gesture, so two presses on two nodes, two drags of two scrolling lists and
// two presses on one node run together, and every event carries the pointer that made it.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::remember;
use cranpose_testing::{PointerLog, RobotTestRule, TestRenderer, create_headless_robot_test};
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, Modifier, Point, PointerButton, PointerButtons,
    PointerEventKind, PointerId, Row, RowSpec, ScrollState, Size, ZoomState,
};

const LEFT: PointerId = PointerId(1);
const RIGHT: PointerId = PointerId(2);

type Log = PointerLog;

fn seen_by(log: &Log) -> Vec<(PointerEventKind, PointerId)> {
    log.events()
        .iter()
        .map(|seen| (seen.kind, seen.id))
        .collect()
}

// A box of `size` that records every pointer event that reaches it.
fn recording(log: &Log, size: Size) {
    Box(
        Modifier::empty().size(size).then(log.recorder()),
        BoxSpec::default(),
        || {},
    );
}

// Two boxes of 100 by 100 dp side by side, and what each saw.
fn two_boxes() -> (RobotTestRule<TestRenderer>, Log, Log) {
    let (left, right) = (Log::default(), Log::default());
    let (first, second) = (left.clone(), right.clone());
    let mut robot = create_headless_robot_test(200, 100, move || {
        let (first, second) = (first.clone(), second.clone());
        Row(Modifier::empty(), RowSpec::default(), move || {
            recording(&first, Size::new(100.0, 100.0));
            recording(&second, Size::new(100.0, 100.0));
        });
    });
    robot.wait_for_idle();
    (robot, left, right)
}

fn at(x: f32, y: f32) -> Point {
    Point { x, y }
}

#[test]
fn two_pointers_press_and_drag_two_boxes_at_once() {
    let (mut robot, left, right) = two_boxes();
    let shell = robot.shell_mut();
    shell.independent_pointer_moved(LEFT, 20.0, 20.0);
    shell.independent_pointer_moved(RIGHT, 120.0, 30.0);
    assert!(shell.independent_pointer_pressed(LEFT, PointerButton::Primary));
    assert!(shell.independent_pointer_pressed(RIGHT, PointerButton::Primary));
    // The pointers drag in turn, and each box follows its own pointer
    shell.independent_pointer_moved(LEFT, 40.0, 20.0);
    shell.independent_pointer_moved(RIGHT, 150.0, 60.0);
    shell.independent_pointer_moved(LEFT, 60.0, 25.0);
    assert!(shell.independent_pointer_released(LEFT, PointerButton::Primary));
    shell.independent_pointer_moved(RIGHT, 170.0, 70.0);
    assert!(shell.independent_pointer_released(RIGHT, PointerButton::Primary));

    use PointerEventKind::{Down, Enter, Move, Up};
    assert_eq!(
        seen_by(&left),
        [
            (Enter, LEFT),
            (Move, LEFT),
            (Down, LEFT),
            (Move, LEFT),
            (Move, LEFT),
            (Up, LEFT)
        ]
    );
    assert_eq!(
        seen_by(&right),
        [
            (Enter, RIGHT),
            (Move, RIGHT),
            (Down, RIGHT),
            (Move, RIGHT),
            (Move, RIGHT),
            (Up, RIGHT)
        ]
    );
    let moves: Vec<Point> = right
        .events()
        .iter()
        .filter(|seen| seen.kind == Move)
        .map(|seen| seen.position)
        .collect();
    // Positions are in the box's own dp, and the right box starts at 100
    assert_eq!(moves, [at(20.0, 30.0), at(50.0, 60.0), at(70.0, 70.0)]);
}

#[test]
fn a_drag_that_leaves_its_box_keeps_its_gesture_while_the_other_pointer_hovers() {
    let (mut robot, left, right) = two_boxes();
    let shell = robot.shell_mut();
    shell.independent_pointer_moved(LEFT, 20.0, 20.0);
    shell.independent_pointer_pressed(LEFT, PointerButton::Primary);
    // The left pointer drags over the right box, which the right pointer hovers
    shell.independent_pointer_moved(RIGHT, 150.0, 50.0);
    shell.independent_pointer_moved(LEFT, 150.0, 50.0);
    shell.independent_pointer_released(LEFT, PointerButton::Primary);

    assert!(
        left.events()
            .iter()
            .any(|seen| seen.kind == PointerEventKind::Move && seen.position == at(150.0, 50.0)),
        "the press keeps its box, so the drag reaches it past the box's edge"
    );
    assert!(
        right.events().iter().all(|seen| seen.id == RIGHT),
        "the right box sees only the right pointer, as the left one's press belongs to the left box"
    );
}

#[test]
fn two_pointers_on_one_box_arrive_as_two_pointers() {
    let log = Log::default();
    let shown = log.clone();
    let mut robot = create_headless_robot_test(100, 100, move || {
        recording(&shown, Size::new(100.0, 100.0));
    });
    robot.wait_for_idle();
    let shell = robot.shell_mut();
    shell.independent_pointer_moved(LEFT, 10.0, 10.0);
    shell.independent_pointer_moved(RIGHT, 90.0, 90.0);
    assert!(shell.independent_pointer_pressed(LEFT, PointerButton::Primary));
    assert!(shell.independent_pointer_pressed(RIGHT, PointerButton::Primary));
    shell.independent_pointer_released(RIGHT, PointerButton::Primary);
    shell.independent_pointer_released(LEFT, PointerButton::Primary);

    let downs: Vec<(PointerId, Point)> = log
        .events()
        .iter()
        .filter(|seen| seen.kind == PointerEventKind::Down)
        .map(|seen| (seen.id, seen.position))
        .collect();
    assert_eq!(downs, [(LEFT, at(10.0, 10.0)), (RIGHT, at(90.0, 90.0))]);
    let ups: Vec<PointerId> = log
        .events()
        .iter()
        .filter(|seen| seen.kind == PointerEventKind::Up)
        .map(|seen| seen.id)
        .collect();
    assert_eq!(ups, [RIGHT, LEFT]);
}

#[test]
fn a_secondary_press_reaches_the_node_as_a_button_down_with_the_button_held() {
    let (mut robot, left, _) = two_boxes();
    let shell = robot.shell_mut();
    shell.independent_pointer_moved(LEFT, 20.0, 20.0);
    assert!(shell.independent_pointer_pressed(LEFT, PointerButton::Secondary));
    shell.independent_pointer_moved(LEFT, 30.0, 20.0);
    assert!(shell.independent_pointer_released(LEFT, PointerButton::Secondary));

    let log = left.events();
    let down = log
        .iter()
        .find(|seen| seen.kind == PointerEventKind::ButtonDown(PointerButton::Secondary))
        .expect("the node sees the button go down");
    assert_eq!(down.id, LEFT);
    assert!(down.buttons.contains(PointerButton::Secondary));
    assert!(
        log.iter()
            .any(|seen| seen.kind == PointerEventKind::Move && seen.position == at(30.0, 20.0)),
        "the press holds, so the drag reaches the node"
    );
    let up = log
        .iter()
        .find(|seen| seen.kind == PointerEventKind::ButtonUp(PointerButton::Secondary))
        .expect("the node sees the button go up");
    assert!(!up.buttons.contains(PointerButton::Secondary));
    assert!(
        log.iter().all(|seen| seen.kind != PointerEventKind::Down),
        "a secondary button is no primary press"
    );
}

#[test]
fn a_second_button_joins_the_gesture_of_the_first() {
    let (mut robot, left, _) = two_boxes();
    let shell = robot.shell_mut();
    shell.independent_pointer_moved(LEFT, 20.0, 20.0);
    shell.independent_pointer_pressed(LEFT, PointerButton::Primary);
    shell.independent_pointer_moved(LEFT, 150.0, 20.0);
    assert!(shell.independent_pointer_pressed(LEFT, PointerButton::Secondary));
    shell.independent_pointer_released(LEFT, PointerButton::Primary);
    assert!(
        shell.independent_pointer_released(LEFT, PointerButton::Secondary),
        "the last button's release still reaches the node that took the first press"
    );
    let buttons: Vec<PointerButtons> = left
        .events()
        .iter()
        .filter(|seen| matches!(seen.kind, PointerEventKind::ButtonDown(_)))
        .map(|seen| seen.buttons)
        .collect();
    assert_eq!(
        buttons,
        [PointerButtons::new()
            .with(PointerButton::Primary)
            .with(PointerButton::Secondary)]
    );
}

#[test]
fn a_scroll_reaches_the_node_under_the_pointer_with_its_id() {
    let (mut robot, left, right) = two_boxes();
    let shell = robot.shell_mut();
    shell.independent_pointer_moved(LEFT, 20.0, 20.0);
    shell.independent_pointer_moved(RIGHT, 120.0, 20.0);
    shell.independent_pointer_scrolled(RIGHT, 0.0, 12.0);
    let right = right.events();
    let scroll = right
        .iter()
        .find(|seen| seen.kind == PointerEventKind::Scroll)
        .expect("the right box sees the scroll");
    assert_eq!(
        (scroll.id, scroll.scroll_delta),
        (RIGHT, Point { x: 0.0, y: 12.0 })
    );
    assert!(
        left.events()
            .iter()
            .all(|seen| seen.kind != PointerEventKind::Scroll),
        "the left box is not under the scrolling pointer"
    );
}

#[test]
fn a_pointer_that_exits_cancels_its_press_and_leaves_its_hover() {
    let (mut robot, left, _) = two_boxes();
    let shell = robot.shell_mut();
    shell.independent_pointer_moved(LEFT, 20.0, 20.0);
    shell.independent_pointer_pressed(LEFT, PointerButton::Primary);
    assert!(shell.independent_pointer_exited(LEFT));
    use PointerEventKind::{Cancel, Down, Enter, Exit, Move};
    assert_eq!(
        seen_by(&left),
        [
            (Enter, LEFT),
            (Move, LEFT),
            (Down, LEFT),
            (Cancel, LEFT),
            (Exit, LEFT)
        ]
    );
    assert!(
        !shell.independent_pointer_exited(LEFT),
        "a pointer that is gone exits no more"
    );
}

#[test]
fn the_primary_pointer_is_not_independent() {
    let (mut robot, left, _) = two_boxes();
    let shell = robot.shell_mut();
    assert!(!shell.independent_pointer_moved(PointerId::PRIMARY, 20.0, 20.0));
    assert!(left.events().is_empty());
}

#[test]
fn an_independent_pointer_leaves_the_primary_pointers_press_alone() {
    let (mut robot, left, right) = two_boxes();
    robot.mouse_move(20.0, 20.0);
    robot.mouse_down();
    let shell = robot.shell_mut();
    shell.independent_pointer_moved(RIGHT, 120.0, 20.0);
    shell.independent_pointer_pressed(RIGHT, PointerButton::Primary);
    shell.independent_pointer_released(RIGHT, PointerButton::Primary);
    robot.mouse_up();

    let primary = |log: &Log| {
        log.events()
            .iter()
            .filter(|seen| seen.id == PointerId::PRIMARY)
            .map(|seen| seen.kind)
            .collect::<Vec<_>>()
    };
    assert!(primary(&left).contains(&PointerEventKind::Down));
    assert!(primary(&left).contains(&PointerEventKind::Up));
    assert!(primary(&right).is_empty());
}

// A column of 100 by 100 dp boxes inside a scroll area of 100 by 100 dp.
fn list(scroll: ScrollState) {
    Column(
        Modifier::empty()
            .size(Size::new(100.0, 100.0))
            .vertical_scroll(scroll, false),
        ColumnSpec::default(),
        || {
            for _ in 0..10 {
                Box(
                    Modifier::empty().size(Size::new(100.0, 100.0)),
                    BoxSpec::default(),
                    || {},
                );
            }
        },
    );
}

fn two_lists() -> (RobotTestRule<TestRenderer>, ScrollState, ScrollState) {
    let states = Rc::new(RefCell::new(Vec::<ScrollState>::new()));
    let made = Rc::clone(&states);
    let mut robot = create_headless_robot_test(200, 100, move || {
        let made = Rc::clone(&made);
        let first = remember(|| ScrollState::new(0.0)).with(ScrollState::clone);
        let second = remember(|| ScrollState::new(0.0)).with(ScrollState::clone);
        made.replace(vec![first, second]);
        Row(Modifier::empty(), RowSpec::default(), move || {
            list(first);
            list(second);
        });
    });
    robot.wait_for_idle();
    let states = states.borrow();
    (robot, states[0], states[1])
}

#[test]
fn two_pointers_drag_two_lists_at_once() {
    let (mut robot, left, right) = two_lists();
    let shell = robot.shell_mut();
    shell.independent_pointer_moved(LEFT, 50.0, 80.0);
    shell.independent_pointer_moved(RIGHT, 150.0, 90.0);
    shell.independent_pointer_pressed(LEFT, PointerButton::Primary);
    shell.independent_pointer_pressed(RIGHT, PointerButton::Primary);
    for step in 1..=6_u8 {
        let step = f32::from(step);
        shell.independent_pointer_moved(LEFT, 50.0, 80.0 - 5.0 * step);
        shell.independent_pointer_moved(RIGHT, 150.0, 90.0 - 8.0 * step);
    }
    shell.independent_pointer_released(LEFT, PointerButton::Primary);
    shell.independent_pointer_released(RIGHT, PointerButton::Primary);
    robot.wait_for_idle();

    assert!(
        left.value() > 20.0 && right.value() > 35.0,
        "both lists follow their pointers: {} and {}",
        left.value(),
        right.value()
    );
    assert!(
        right.value() > left.value(),
        "the right pointer dragged farther, so its list went farther: {} and {}",
        left.value(),
        right.value()
    );
}

#[test]
fn a_second_pointer_on_a_dragged_list_passes_by_until_the_first_lifts() {
    let (mut robot, left, _) = two_lists();
    let shell = robot.shell_mut();
    shell.independent_pointer_moved(LEFT, 50.0, 90.0);
    shell.independent_pointer_pressed(LEFT, PointerButton::Primary);
    shell.independent_pointer_moved(LEFT, 50.0, 60.0);
    let held = Cell::new(0.0);
    held.set(left.value());
    // The right pointer drags the same list in the other direction. The list ignores it.
    shell.independent_pointer_moved(RIGHT, 50.0, 20.0);
    shell.independent_pointer_pressed(RIGHT, PointerButton::Primary);
    shell.independent_pointer_moved(RIGHT, 50.0, 80.0);
    shell.independent_pointer_released(RIGHT, PointerButton::Primary);
    shell.independent_pointer_moved(LEFT, 50.0, 40.0);
    shell.independent_pointer_released(LEFT, PointerButton::Primary);
    robot.wait_for_idle();
    assert!(held.get() > 0.0);
    assert!(
        left.value() > held.get(),
        "the first pointer goes on, and the second one's drag did not pull the list back: {} then {}",
        held.get(),
        left.value()
    );
}

#[test]
fn a_scroll_of_a_pointer_scrolls_the_list_under_it() {
    let (mut robot, left, right) = two_lists();
    let shell = robot.shell_mut();
    shell.independent_pointer_moved(RIGHT, 150.0, 50.0);
    assert!(shell.independent_pointer_scrolled(RIGHT, 0.0, -30.0));
    robot.wait_for_idle();
    assert!(right.value() > 0.0, "the list under the pointer scrolls");
    assert_eq!(left.value(), 0.0, "the other list stays");
}

// The features that read the primary pointer of a window in a phone work with a pointer of another
// id, as the pointers of a headset's two hands have. The long press is in `long_press_test.rs`.

#[test]
fn an_independent_pointer_taps_and_two_of_them_pinch_a_zoomable_node() {
    let state = Rc::new(Cell::new(None));
    let made = Rc::clone(&state);
    let mut robot = create_headless_robot_test(200, 200, move || {
        let zoom = remember(ZoomState::new).with(|zoom| *zoom);
        made.set(Some(zoom));
        Box(
            Modifier::empty()
                .size(Size::new(200.0, 200.0))
                .zoomable(zoom),
            BoxSpec::default(),
            || {},
        );
    });
    robot.wait_for_idle();
    let zoom = state.get().expect("the zoom state");
    // Two hands pinch: they start 40 dp apart and end 120 dp apart
    let shell = robot.shell_mut();
    shell.independent_pointer_moved(LEFT, 80.0, 100.0);
    shell.independent_pointer_moved(RIGHT, 120.0, 100.0);
    shell.independent_pointer_pressed(LEFT, PointerButton::Primary);
    shell.independent_pointer_pressed(RIGHT, PointerButton::Primary);
    for step in 1..=8_u8 {
        let spread = f32::from(step) * 5.0;
        shell.independent_pointer_moved(LEFT, 80.0 - spread, 100.0);
        shell.independent_pointer_moved(RIGHT, 120.0 + spread, 100.0);
    }
    shell.independent_pointer_released(LEFT, PointerButton::Primary);
    shell.independent_pointer_released(RIGHT, PointerButton::Primary);
    robot.wait_for_idle();
    assert!(
        zoom.scale_non_reactive() > 1.5,
        "{}",
        zoom.scale_non_reactive()
    );
    // A double tap of one pointer resets the zoom
    let shell = robot.shell_mut();
    for _ in 0..2 {
        shell.independent_pointer_moved(LEFT, 100.0, 100.0);
        shell.independent_pointer_pressed(LEFT, PointerButton::Primary);
        shell.independent_pointer_released(LEFT, PointerButton::Primary);
    }
    robot.wait_for_idle();
    assert!(!zoom.is_transformed(), "a double tap resets the zoom");
}
