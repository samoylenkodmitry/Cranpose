//! `ScrollState::is_scroll_in_progress` through the headless robot: a drag
//! and the fling after it keep the scroll in progress, and it ends when the
//! column stops.

use std::{cell::RefCell, rc::Rc};

use cranpose_foundation::lazy::{LazyListScope, LazyListState, rememberLazyListState};
use cranpose_testing::create_headless_robot_test;
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, LazyColumn, LazyColumnSpec, Modifier, ScrollState, Size,
};

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

// A reversed list stands at its start with its first item at the bottom, and
// a pull down brings the next items in.
#[test]
fn a_reversed_list_at_its_start_takes_a_pull_toward_its_items_as_a_drag() {
    let captured: Rc<RefCell<Option<LazyListState>>> = Rc::default();
    let installed = Rc::clone(&captured);
    let mut robot = create_headless_robot_test(200, 400, move || {
        let list = rememberLazyListState();
        *installed.borrow_mut() = Some(list);
        LazyColumn(
            Modifier::empty().fill_max_size(),
            list,
            LazyColumnSpec::new().reverse_layout(true),
            |scope| {
                scope.items(40, |_| {
                    Box(
                        Modifier::empty().fill_max_width().height(40.0),
                        BoxSpec::default(),
                        || {},
                    );
                });
            },
        );
    });
    let list = (*captured.borrow()).expect("the list is composed");
    assert!(!list.is_dragged(), "no finger is down yet");

    robot.mouse_move(100.0, 100.0);
    robot.mouse_down();
    for step in 1..=10 {
        robot.mouse_move(100.0, 100.0 + 20.0 * step as f32);
        robot.advance_time(16_000_000);
        let scrolled =
            list.first_visible_item_index() > 0 || list.first_visible_item_scroll_offset() > 0.0;
        assert_eq!(
            list.is_dragged(),
            scrolled,
            "the list is dragged from the move that scrolls it (move {step})"
        );
    }
    assert!(
        list.first_visible_item_index() >= 3,
        "the pull scrolled the list: its item {} is at the bottom",
        list.first_visible_item_index()
    );

    robot.mouse_up();
    robot.advance_time(16_000_000);
    assert!(!list.is_dragged(), "the finger is up");
}
