//! `LiquidPullToRefreshBox` through the headless robot: a pull past the
//! threshold refreshes once, a short pull does not, a pull leaves the list at
//! its top, a refreshing list rests 60 points down with its spinner in the
//! gap, and a screen reader refreshes through the box's named action.

use std::{cell::Cell, rc::Rc};

use cranpose_liquid::prelude::*;
use cranpose_testing::{RobotTestRule, TestRenderer, create_headless_robot_test};
use cranpose_ui::{Box, BoxSpec, Column, ColumnSpec, Modifier, ScrollState, Size};

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

#[test]
fn a_screen_reader_refreshes_through_the_named_action() {
    let mut screen = screen();
    screen.robot.shell_mut().set_semantics_enabled(true);
    screen.robot.wait_for_idle();
    crate::reader::custom_action(&mut screen.robot, "Refresh").invoke();
    screen.robot.wait_for_idle();
    assert_eq!(screen.refreshes.get(), 1);
}

// A list of 40 point rows, refreshing, whose first two rows log their taps.
fn refreshing_list() -> (crate::reader::Robot, crate::reader::Log) {
    crate::reader::liquid_screen(|log| {
        let record = Rc::clone(log);
        let scroll = cranpose_ui::rememberScrollState!(0.0);
        LiquidPullToRefreshBox(
            Modifier::empty().fill_max_size(),
            true,
            "Refresh",
            || {},
            move || {
                let record = Rc::clone(&record);
                Column(
                    Modifier::empty()
                        .fill_max_size()
                        .vertical_scroll(scroll, false),
                    ColumnSpec::default(),
                    move || {
                        for row in ["row 0", "row 1", "row 2", "row 3"] {
                            let record = Rc::clone(&record);
                            Box(
                                Modifier::empty()
                                    .size(Size::new(400.0, 40.0))
                                    .clickable(move |_| record.borrow_mut().push(row)),
                                BoxSpec::default(),
                                || {},
                            );
                        }
                    },
                );
            },
        );
    })
}

// Half a second of frames: the spinner turns for as long as the list
// refreshes, so the screen never idles.
fn frames(robot: &mut crate::reader::Robot) {
    for _ in 0..30 {
        robot.advance_time(16_666_667);
    }
}

fn tap_turning(robot: &mut crate::reader::Robot, x: f32, y: f32) {
    robot.mouse_move(x, y);
    robot.mouse_down();
    robot.mouse_up();
    frames(robot);
}

#[test]
fn a_refreshing_list_rests_60_points_down() {
    let (mut robot, log) = refreshing_list();
    frames(&mut robot);
    tap_turning(&mut robot, 200.0, 20.0);
    tap_turning(&mut robot, 200.0, 70.0);
    assert_eq!(
        *log.borrow(),
        ["row 0"],
        "the gap above the list takes no tap, and the first row sits at 60 to 100"
    );
}
