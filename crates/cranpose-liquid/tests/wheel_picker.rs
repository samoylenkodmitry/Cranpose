//! `LiquidWheelPicker` through the headless robot: the wheel reads its
//! option, a drag selects the option it comes to rest on, a reader adjusts
//! it, and a new selection from the caller turns it.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_liquid::prelude::*;
use cranpose_testing::{
    PlacedSemanticsNode, RobotTestRule, TestRenderer, create_headless_robot_test,
    placed_semantics_from_shell,
};
use cranpose_ui::Modifier;

const FRUITS: [&str; 7] = [
    "Apple", "Banana", "Cherry", "Grape", "Lemon", "Mango", "Orange",
];

struct Screen {
    robot: RobotTestRule<TestRenderer>,
    selected: MutableState<usize>,
    reported: Rc<Cell<usize>>,
}

fn screen(initial: usize) -> Screen {
    let slot: Rc<Cell<Option<MutableState<usize>>>> = Rc::default();
    let captured = Rc::clone(&slot);
    let reported = Rc::new(Cell::new(0));
    let counter = Rc::clone(&reported);
    let mut robot = create_headless_robot_test(360, 300, move || {
        let captured = Rc::clone(&captured);
        let counter = Rc::clone(&counter);
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let selected = rememberMutableStateOf(move || initial);
            captured.set(Some(selected));
            let counter = Rc::clone(&counter);
            LiquidWheelPicker(
                Modifier::empty().width(300.0),
                "Fruit",
                &FRUITS,
                selected.get(),
                move |next| {
                    counter.set(counter.get() + 1);
                    selected.set(next);
                },
            );
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    let selected = slot
        .take()
        .expect("the composition remembered the selection");
    Screen {
        robot,
        selected,
        reported,
    }
}

impl Screen {
    fn wheel(&mut self) -> PlacedSemanticsNode {
        let tree = placed_semantics_from_shell(self.robot.shell_mut()).expect("semantics");
        tree.flatten()
            .into_iter()
            .find(|node| node.label.as_deref() == Some("Fruit"))
            .cloned()
            .expect("the wheel named Fruit")
    }

    fn showing(&mut self) -> Option<String> {
        self.wheel().state_description
    }
}

#[test]
fn the_wheel_reads_its_option_and_reports_nothing_at_rest() {
    let mut screen = screen(3);
    assert_eq!(screen.showing().as_deref(), Some("Grape"));
    assert_eq!(
        screen.reported.get(),
        0,
        "a wheel at rest on the selection reports nothing"
    );
}

#[test]
fn a_drag_selects_the_option_the_wheel_rests_on() {
    let mut screen = screen(3);
    let bounds = screen.wheel().target_bounds();
    let x = bounds.x + bounds.width / 2.0;
    let middle = bounds.y + bounds.height / 2.0;
    // Up by two 32 point rows and a bit, slowly, with a rest before the
    // release.
    let shell = screen.robot.shell_mut();
    shell.set_cursor_at_time(x, middle, Some(0));
    shell.pointer_pressed_at_time(Some(0));
    for step in 1..=10_i64 {
        shell.set_cursor_at_time(x, middle - step as f32 * 7.0, Some(step * 30));
        shell.update();
    }
    shell.pointer_released_at_time(Some(800));
    screen.robot.wait_for_motion();
    assert_eq!(screen.selected.get(), 5);
    assert_eq!(screen.showing().as_deref(), Some("Mango"));
    assert_eq!(screen.reported.get(), 1, "one rest reports once");
}

#[test]
fn a_reader_adjusts_the_wheel() {
    let mut screen = screen(3);
    let node = screen.wheel().node_id;
    let tree = screen
        .robot
        .shell_mut()
        .semantics_tree()
        .expect("semantics");
    let set = cranpose_testing::robot_assertions::find_semantics_node(tree.root(), &|found| {
        found.node_id == node
    })
    .and_then(|found| found.details().set_progress.clone())
    .expect("the wheel takes a value");
    assert!(set.invoke(1.0));
    screen.robot.wait_for_motion();
    assert_eq!(screen.selected.get(), 1);
    assert_eq!(screen.showing().as_deref(), Some("Banana"));
}

#[test]
fn a_new_selection_from_the_caller_turns_the_wheel() {
    let mut screen = screen(3);
    screen.selected.set(6);
    screen.robot.wait_for_motion();
    assert_eq!(screen.showing().as_deref(), Some("Orange"));
    assert_eq!(
        screen.reported.get(),
        0,
        "the caller's own change is not reported back"
    );
}
