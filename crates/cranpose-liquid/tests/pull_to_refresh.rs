//! `LiquidPullToRefreshBox` through the headless robot: a pull past the
//! threshold refreshes once, a short pull does not, a pull leaves the list at
//! its top, and a screen reader refreshes through the box's named action.

use std::{cell::Cell, rc::Rc};

use cranpose_liquid::prelude::*;
use cranpose_testing::{RobotTestRule, TestRenderer, create_headless_robot_test};
use cranpose_ui::{Box, BoxSpec, Column, ColumnSpec, Modifier, ScrollState, SemanticsNode, Size};

struct Screen {
    robot: RobotTestRule<TestRenderer>,
    refreshes: Rc<Cell<u32>>,
    scroll: ScrollState,
}

fn screen() -> Screen {
    let refreshes = Rc::new(Cell::new(0));
    let counted = Rc::clone(&refreshes);
    let captured: Rc<std::cell::RefCell<Option<ScrollState>>> = Rc::default();
    let installed = Rc::clone(&captured);
    let robot = create_headless_robot_test(240, 400, move || {
        let counted = Rc::clone(&counted);
        let installed = Rc::clone(&installed);
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let scroll = cranpose_ui::rememberScrollState!(0.0);
            *installed.borrow_mut() = Some(scroll);
            let counted = Rc::clone(&counted);
            LiquidPullToRefreshBox(
                Modifier::empty().fill_max_size(),
                false,
                "Refresh",
                move || counted.set(counted.get() + 1),
                move || {
                    Column(
                        Modifier::empty()
                            .fill_max_size()
                            .vertical_scroll(scroll, false),
                        ColumnSpec::default(),
                        || {
                            for _ in 0..30 {
                                Box(
                                    Modifier::empty().size(Size::new(240.0, 40.0)),
                                    BoxSpec::default(),
                                    || {},
                                );
                            }
                        },
                    );
                },
            );
        });
    });
    let scroll = (*captured.borrow()).expect("the list is composed");
    Screen {
        robot,
        refreshes,
        scroll,
    }
}

#[test]
fn a_pull_past_the_threshold_refreshes_once() {
    let mut screen = screen();
    screen.robot.drag(120.0, 80.0, 120.0, 320.0);
    assert_eq!(screen.refreshes.get(), 1);
    assert_eq!(screen.scroll.value(), 0.0, "the list stays at its top");
}

#[test]
fn a_short_pull_does_not_refresh() {
    let mut screen = screen();
    screen.robot.drag(120.0, 80.0, 120.0, 140.0);
    assert_eq!(screen.refreshes.get(), 0);
}

#[test]
fn a_scrolled_list_scrolls_back_before_it_pulls() {
    let mut screen = screen();
    screen.robot.drag(120.0, 300.0, 120.0, 100.0);
    let scrolled = screen.scroll.value();
    assert!(scrolled > 100.0, "the list scrolled down: {scrolled}");
    screen.robot.drag(120.0, 100.0, 120.0, 180.0);
    assert_eq!(screen.refreshes.get(), 0, "the drag scrolled the list back");
    assert!(screen.scroll.value() < scrolled);
}

fn find_action(node: &SemanticsNode, label: &str) -> Option<cranpose_ui::SemanticsCustomAction> {
    node.details()
        .custom_actions
        .iter()
        .find(|action| action.label == label)
        .cloned()
        .or_else(|| {
            node.children
                .iter()
                .find_map(|child| find_action(child, label))
        })
}

#[test]
fn a_screen_reader_refreshes_through_the_named_action() {
    let mut screen = screen();
    screen.robot.shell_mut().set_semantics_enabled(true);
    screen.robot.wait_for_idle();
    let action = screen
        .robot
        .shell_mut()
        .semantics_tree()
        .and_then(|tree| find_action(tree.root(), "Refresh"))
        .expect("the box offers a Refresh action");
    action.invoke();
    screen.robot.wait_for_idle();
    assert_eq!(screen.refreshes.get(), 1);
}
