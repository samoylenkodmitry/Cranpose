use std::{cell::RefCell, rc::Rc};

use cranpose_core::{mutableStateOf, remember};
use cranpose_liquid::prelude::*;
use cranpose_render_common::text_fonts::TextFonts;
use cranpose_testing::{RobotTestRule, TestRenderer, create_headless_robot_test_with_fonts};
use cranpose_ui::Modifier;
use cranpose_ui_graphics::ColorFilter;

use crate::segmented_style::{advance_frames, find_label};

type Robot = RobotTestRule<TestRenderer>;

fn filter_of(robot: &mut Robot, label: &str) -> Option<ColorFilter> {
    let root = robot.shell_mut().scene().root_layer().expect("a scene");
    find_label(root, label, None)
        .expect("the label is drawn")
        .1
        .cloned()
}

// A control of three segments, the first chosen, that records each index
// it commits.
fn robot() -> (Robot, Rc<RefCell<Vec<usize>>>) {
    let fonts = TextFonts::from_fonts_or_default(&[include_bytes!(
        "../../cranpose-render/common/assets/NotoSansMerged.ttf"
    )]);
    let committed = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&committed);
    let mut robot = create_headless_robot_test_with_fonts(320, 80, fonts, move || {
        let log = Rc::clone(&log);
        LiquidTheme(LiquidThemeSpec::system(SchemeMode::Light), move || {
            let selected = remember(|| mutableStateOf(0_usize)).with(|state| *state);
            let log = Rc::clone(&log);
            LiquidSegmentedControl(
                Modifier::empty().width(300.0),
                selected.get(),
                move |index| {
                    log.borrow_mut().push(index);
                    selected.set(index);
                },
                |scope| {
                    scope.segment("All");
                    scope.segment("Unread");
                    scope.segment("Saved");
                },
            );
        });
    });
    robot.wait_for_idle();
    (robot, committed)
}

fn press(robot: &mut Robot, label: &str) {
    let center = robot.find_by_text(label).center().expect("a segment");
    robot.mouse_move(center.x, center.y);
    robot.mouse_down();
}

#[test]
fn pressing_another_segment_dims_its_label_until_the_finger_lifts() {
    let (mut robot, committed) = robot();
    press(&mut robot, "Saved");
    advance_frames(&mut robot, 15);
    assert!(committed.borrow().is_empty(), "a press commits nothing");
    let filter = filter_of(&mut robot, "Saved");
    let Some(ColorFilter::Modulate(dim)) = filter else {
        panic!("the pressed label is not dimmed: {filter:?}");
    };
    assert!(
        (dim.a() - 0.65).abs() < 0.02,
        "the pressed label's alpha is {}",
        dim.a()
    );
    assert_eq!(filter_of(&mut robot, "Unread"), None);
    assert_eq!(filter_of(&mut robot, "All"), None);

    robot.mouse_up();
    assert_eq!(*committed.borrow(), [2]);
    advance_frames(&mut robot, 40);
    assert_eq!(filter_of(&mut robot, "Saved"), None, "the dim lifts");
}

#[test]
fn the_highlight_follows_the_finger_and_the_release_commits_where_it_ends() {
    let (mut robot, committed) = robot();
    press(&mut robot, "Unread");
    advance_frames(&mut robot, 4);
    let saved = robot.find_by_text("Saved").center().expect("a segment");
    robot.mouse_move(saved.x, saved.y);
    advance_frames(&mut robot, 15);
    assert!(matches!(
        filter_of(&mut robot, "Saved"),
        Some(ColorFilter::Modulate(_))
    ));
    assert_eq!(filter_of(&mut robot, "Unread"), None);
    robot.mouse_up();
    assert_eq!(*committed.borrow(), [2]);
}

#[test]
fn pressing_the_chosen_segment_highlights_no_label() {
    let (mut robot, committed) = robot();
    press(&mut robot, "All");
    advance_frames(&mut robot, 15);
    for label in ["Unread", "Saved"] {
        assert_eq!(filter_of(&mut robot, label), None, "{label} dimmed");
    }
    robot.mouse_up();
    assert_eq!(*committed.borrow(), [0]);
}
