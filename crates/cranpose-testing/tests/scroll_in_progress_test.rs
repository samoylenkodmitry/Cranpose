//! `ScrollState::is_scroll_in_progress` through the headless robot: a drag
//! and the fling after it keep the scroll in progress, and it ends when the
//! column stops.

use std::{cell::RefCell, rc::Rc};

use cranpose_testing::create_headless_robot_test;
use cranpose_ui::{Box, BoxSpec, Column, ColumnSpec, Modifier, ScrollState, Size};

#[test]
fn a_drag_and_its_fling_keep_the_scroll_in_progress_until_the_column_stops() {
    let captured: Rc<RefCell<Option<ScrollState>>> = Rc::default();
    let installed = Rc::clone(&captured);
    let mut robot = create_headless_robot_test(200, 400, move || {
        let scroll = cranpose_ui::rememberScrollState!(0.0);
        *installed.borrow_mut() = Some(scroll);
        Column(
            Modifier::empty()
                .fill_max_size()
                .vertical_scroll(scroll, false),
            ColumnSpec::default(),
            || {
                for _ in 0..40 {
                    Box(
                        Modifier::empty().size(Size::new(200.0, 40.0)),
                        BoxSpec::default(),
                        || {},
                    );
                }
            },
        );
    });
    let scroll = (*captured.borrow()).expect("the column is composed");
    assert!(!scroll.is_scroll_in_progress(), "nothing moves it yet");

    robot.drag(100.0, 300.0, 100.0, 100.0);
    assert!(scroll.is_scroll_in_progress(), "the fling moves it on");
    for _ in 0..120 {
        robot.advance_time(16_000_000);
        robot.wait_for_idle();
    }
    assert!(scroll.value() > 200.0, "the drag and fling scrolled it");
    assert!(!scroll.is_scroll_in_progress(), "the fling came to rest");
}
