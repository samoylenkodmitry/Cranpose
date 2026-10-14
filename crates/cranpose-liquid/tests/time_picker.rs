//! `LiquidTimePicker` through the headless robot: the wheels show the
//! time, a drag turns a wheel to a whole row, a reader adjusts a wheel by
//! name, a 12-hour clock picks the half of the day, and `set_time` turns
//! the wheels.

use std::{cell::Cell, rc::Rc};

use cranpose_liquid::prelude::*;
use cranpose_testing::{
    PlacedSemanticsNode, RobotTestRule, TestRenderer, create_headless_robot_test,
    placed_semantics_from_shell,
};
use cranpose_ui::Modifier;

struct Screen {
    robot: RobotTestRule<TestRenderer>,
    state: Rc<Cell<Option<LiquidTimePickerState>>>,
}

fn screen(hour: u8, minute: u8, is_24_hour: bool) -> Screen {
    let state: Rc<Cell<Option<LiquidTimePickerState>>> = Rc::default();
    let captured = Rc::clone(&state);
    let mut robot = create_headless_robot_test(360, 300, move || {
        let captured = Rc::clone(&captured);
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let picker = rememberLiquidTimePickerState(hour, minute, Some(is_24_hour));
            captured.set(Some(picker));
            LiquidTimePicker(picker, Modifier::empty());
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    Screen { robot, state }
}

impl Screen {
    fn time(&self) -> (u8, u8) {
        let state = self.state.get().expect("composed");
        (state.hour(), state.minute())
    }

    /// The wheel `matches` picks among the controls with a value.
    fn find(&mut self, matches: impl Fn(&PlacedSemanticsNode) -> bool) -> PlacedSemanticsNode {
        let tree = placed_semantics_from_shell(self.robot.shell_mut()).expect("semantics");
        tree.flatten()
            .into_iter()
            .filter(|node| node.state_description.is_some())
            .find(|node| matches(node))
            .cloned()
            .expect("a wheel")
    }

    fn wheel(&mut self, name: &str) -> PlacedSemanticsNode {
        self.find(|node| node.label.as_deref() == Some(name))
    }

    /// The wheel of the half of the day, which shows `period`.
    fn period(&mut self, period: &str) -> PlacedSemanticsNode {
        self.find(|node| node.state_description.as_deref() == Some(period))
    }

    /// Adjusts `wheel` to `row` as a reader does.
    fn adjust(&mut self, wheel: PlacedSemanticsNode, row: f32) {
        let node = wheel.node_id;
        let tree = self.robot.shell_mut().semantics_tree().expect("semantics");
        let set = cranpose_testing::robot_assertions::find_semantics_node(tree.root(), &|found| {
            found.node_id == node
        })
        .and_then(|found| found.details().set_progress.clone())
        .expect("the wheel takes a value");
        assert!(set.invoke(row));
        self.robot.wait_for_motion();
    }
}

#[test]
fn the_wheels_show_the_time() {
    let mut screen = screen(9, 41, true);
    assert_eq!(screen.time(), (9, 41));
    assert_eq!(screen.wheel("Hour").state_description.as_deref(), Some("9"));
    assert_eq!(
        screen.wheel("Minute").state_description.as_deref(),
        Some("41")
    );
}

#[test]
fn a_drag_turns_a_wheel_to_a_whole_row() {
    let mut screen = screen(9, 41, true);
    let bounds = screen.wheel("Minute").target_bounds();
    let x = bounds.x + bounds.width / 2.0;
    let middle = bounds.y + bounds.height / 2.0;
    // Up by three rows and a bit, slowly, with a rest before the release.
    let shell = screen.robot.shell_mut();
    shell.set_cursor_at_time(x, middle, Some(0));
    shell.pointer_pressed_at_time(Some(0));
    for step in 1..=10_i64 {
        shell.set_cursor_at_time(x, middle - step as f32 * 13.0, Some(step * 30));
        shell.update();
    }
    shell.pointer_released_at_time(Some(800));
    screen.robot.wait_for_motion();
    assert_eq!(screen.time(), (9, 44));
}

#[test]
fn a_reader_adjusts_a_wheel_by_name() {
    let mut screen = screen(9, 41, true);
    let hour = screen.wheel("Hour");
    screen.adjust(hour, 17.0);
    assert_eq!(screen.time(), (17, 41));
}

#[test]
fn a_twelve_hour_clock_picks_the_half_of_the_day() {
    let mut screen = screen(21, 5, false);
    assert_eq!(screen.time(), (21, 5));
    assert_eq!(screen.wheel("Hour").state_description.as_deref(), Some("9"));
    let period = screen.period("PM");
    screen.adjust(period, 0.0);
    assert_eq!(screen.time(), (9, 5));
}

#[test]
fn set_time_turns_the_wheels() {
    let mut screen = screen(9, 41, false);
    screen.state.get().expect("composed").set_time(0, 30);
    screen.robot.wait_for_motion();
    assert_eq!(screen.time(), (0, 30));
    assert_eq!(
        screen.wheel("Hour").state_description.as_deref(),
        Some("12")
    );
}

#[cfg(feature = "localization-formatting")]
#[test]
fn the_clock_follows_the_app_s_language() {
    use cranpose_ui::{Catalog, Locale, ProvideLocalization};
    for (tag, twenty_four) in [("de", true), ("en", false)] {
        let state: Rc<Cell<Option<LiquidTimePickerState>>> = Rc::default();
        let captured = Rc::clone(&state);
        let catalog = Catalog::from_resources("en", &[]).expect("catalog");
        let mut robot = create_headless_robot_test(360, 300, move || {
            let captured = Rc::clone(&captured);
            ProvideLocalization(&catalog, Locale::parse(tag).expect("locale"), move || {
                let picker = rememberLiquidTimePickerState(21, 5, None);
                captured.set(Some(picker));
                LiquidTimePicker(picker, Modifier::empty());
            });
        });
        robot.wait_for_idle();
        let picker = state.get().expect("composed");
        assert_eq!(picker.is_24_hour(), twenty_four, "{tag}");
        assert_eq!((picker.hour(), picker.minute()), (21, 5), "{tag}");
        assert_eq!(robot.find_by_text("PM").exists(), !twenty_four, "{tag}");
    }
}
