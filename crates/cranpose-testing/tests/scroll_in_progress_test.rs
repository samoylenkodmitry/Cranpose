//! Scrolls through the headless robot. A drag and the fling after it keep the
//! scroll in progress until the content stops. A jump or an animation that the
//! app starts stops the fling, as Compose's `scrollToItem` and `scrollTo` do.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_testing::{RobotTestRule, TestRenderer, create_headless_robot_test};
use cranpose_ui::{
    AnimateScroll, Box, BoxSpec, Column, ColumnSpec, LazyColumn, LazyColumnSpec, Modifier,
    ScrollState, Size,
    round_scaling_list::CentreAnchor,
    widgets::wear::{
        WearScalingLazyColumn, WearScalingLazyColumnSpec, WearScalingListState,
        rememberWearScalingListState,
    },
};

const FRAME_NANOS: u64 = 16_000_000;

type Robot = RobotTestRule<TestRenderer>;

// A column of `rows` rows, 40 points tall each, that scrolls in a 200 by 400
// window.
fn scrolling_column(rows: usize) -> (Robot, ScrollState) {
    let captured: Rc<RefCell<Option<ScrollState>>> = Rc::default();
    let installed = Rc::clone(&captured);
    let robot = create_headless_robot_test(200, 400, move || {
        let scroll = cranpose_ui::rememberScrollState!(0.0);
        *installed.borrow_mut() = Some(scroll);
        Column(
            Modifier::empty()
                .fill_max_size()
                .vertical_scroll(scroll, false),
            ColumnSpec::default(),
            move || {
                for _ in 0..rows {
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
    (robot, scroll)
}

// `items` items of `list`, 40 points tall each, in a lazy column that
// fills its window.
fn lazy_rows(list: LazyListState, items: usize, spec: LazyColumnSpec) {
    LazyColumn(Modifier::empty().fill_max_size(), list, spec, |scope| {
        scope.items(items, |_| {
            Box(
                Modifier::empty().fill_max_width().height(40.0),
                BoxSpec::default(),
                || {},
            );
        });
    });
}

// A lazy column of `items` items in a 200 by 400 window.
fn lazy_column(items: usize, spec: LazyColumnSpec) -> (Robot, LazyListState) {
    let captured: Rc<RefCell<Option<LazyListState>>> = Rc::default();
    let installed = Rc::clone(&captured);
    let robot = create_headless_robot_test(200, 400, move || {
        let list = rememberLazyListState();
        *installed.borrow_mut() = Some(list);
        lazy_rows(list, items, spec.clone());
    });
    let list = (*captured.borrow()).expect("the list is composed");
    (robot, list)
}

fn run_frames(robot: &mut Robot, frames: usize) {
    for _ in 0..frames {
        robot.advance_time(FRAME_NANOS);
        robot.wait_for_idle();
    }
}

// A quick drag up that lets go into a fling toward the end. Two frames of
// the fling follow.
fn fling_toward_the_end(robot: &mut Robot) {
    robot.drag(100.0, 300.0, 100.0, 100.0);
    run_frames(robot, 2);
}

// Where the list stands: its first visible item and how far it is scrolled.
fn position(list: LazyListState) -> (usize, f32) {
    (
        list.first_visible_item_index(),
        list.first_visible_item_scroll_offset(),
    )
}

#[test]
fn a_drag_and_its_fling_keep_the_scroll_in_progress_until_the_column_stops() {
    let (mut robot, scroll) = scrolling_column(40);
    assert!(!scroll.is_scroll_in_progress(), "nothing moves it yet");

    robot.drag(100.0, 300.0, 100.0, 100.0);
    assert!(scroll.is_scroll_in_progress(), "the fling moves it on");
    run_frames(&mut robot, 120);
    assert!(scroll.value() > 200.0, "the drag and fling scrolled it");
    assert!(!scroll.is_scroll_in_progress(), "the fling came to rest");
}

// A reversed list stands at its start with its first item at the bottom, and
// a pull down brings the next items in.
#[test]
fn a_reversed_list_at_its_start_takes_a_pull_toward_its_items_as_a_drag() {
    let (mut robot, list) = lazy_column(40, LazyColumnSpec::new().reverse_layout(true));
    assert!(!list.is_dragged(), "no finger is down yet");

    robot.mouse_move(100.0, 100.0);
    robot.mouse_down();
    for step in 1..=10 {
        robot.mouse_move(100.0, 100.0 + 20.0 * step as f32);
        robot.advance_time(FRAME_NANOS);
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
    robot.advance_time(FRAME_NANOS);
    assert!(!list.is_dragged(), "the finger is up");
}

#[test]
fn scroll_to_item_stops_a_fling() {
    let (mut robot, list) = lazy_column(400, LazyColumnSpec::new());
    fling_toward_the_end(&mut robot);
    assert!(list.is_scroll_in_progress(), "the fling moves the list");

    list.scroll_to_item(0, 0.0);
    run_frames(&mut robot, 120);

    assert_eq!(
        position(list),
        (0, 0.0),
        "the list stays where the jump put it"
    );
    assert!(!list.is_scroll_in_progress(), "nothing moves the list");
}

#[test]
fn stop_scroll_animation_stops_a_fling() {
    let (mut robot, list) = lazy_column(400, LazyColumnSpec::new());
    fling_toward_the_end(&mut robot);

    assert!(list.stop_scroll_animation(), "the fling ran");
    run_frames(&mut robot, 1);
    let stopped_at = position(list);
    run_frames(&mut robot, 120);

    assert_eq!(
        position(list),
        stopped_at,
        "the list stays where the fling stopped"
    );
    assert!(!list.is_scroll_in_progress(), "nothing moves the list");
}

#[test]
fn an_animated_scroll_takes_the_list_from_a_fling() {
    let (mut robot, list) = lazy_column(400, LazyColumnSpec::new());
    fling_toward_the_end(&mut robot);

    list.animate_scroll_to_item(2, 0.0);
    run_frames(&mut robot, 240);

    assert_eq!(position(list), (2, 0.0), "the animation lands on its item");
    assert!(!list.is_scroll_in_progress(), "nothing moves the list");
}

#[test]
fn scroll_to_stops_a_fling() {
    let (mut robot, scroll) = scrolling_column(400);
    fling_toward_the_end(&mut robot);
    assert!(scroll.is_scroll_in_progress(), "the fling moves the column");

    scroll.scroll_to(0.0);
    run_frames(&mut robot, 120);

    assert_eq!(
        scroll.value(),
        0.0,
        "the column stays where the jump put it"
    );
    assert!(!scroll.is_scroll_in_progress(), "nothing moves the column");
}

// The state outlives its list: a list that leaves the screen during its fling
// leaves the state at rest.
#[test]
fn a_list_that_leaves_during_its_fling_leaves_its_state_at_rest() {
    // The list's state and whether the list shows, both kept above the list.
    #[derive(Clone, Copy)]
    struct Above {
        list: LazyListState,
        shown: MutableState<bool>,
    }
    let captured: Rc<Cell<Option<Above>>> = Rc::default();
    let installed = Rc::clone(&captured);
    let mut robot = create_headless_robot_test(200, 400, move || {
        let list = rememberLazyListState();
        let shown = rememberMutableStateOf(|| true);
        installed.set(Some(Above { list, shown }));
        if shown.get() {
            lazy_rows(list, 400, LazyColumnSpec::new());
        }
    });
    let Above { list, shown } = captured.get().expect("the list is composed");
    fling_toward_the_end(&mut robot);
    assert!(list.is_scroll_in_progress(), "the fling moves the list");

    shown.set(false);
    run_frames(&mut robot, 2);
    assert!(
        !list.is_scroll_in_progress(),
        "the fling ended with its list"
    );
}

// A Wear scaling list of 400 rows, 40 points tall each, in a 200 by 400
// window.
fn wear_list() -> (Robot, WearScalingListState) {
    let captured: Rc<RefCell<Option<WearScalingListState>>> = Rc::default();
    let installed = Rc::clone(&captured);
    let robot = create_headless_robot_test(200, 400, move || {
        let state = rememberWearScalingListState(CentreAnchor::default());
        *installed.borrow_mut() = Some(state);
        WearScalingLazyColumn(
            Modifier::empty().fill_max_size(),
            state,
            WearScalingLazyColumnSpec::default(),
            |scope| {
                scope.items(LazyItems::new(400), |_| {
                    Box(
                        Modifier::empty().fill_max_width().height(40.0),
                        BoxSpec::default(),
                        || {},
                    );
                });
            },
        );
    });
    let state = (*captured.borrow()).expect("the list is composed");
    (robot, state)
}

// A swipe flings the list on every platform: on watchOS, which gives its
// moves no event time, the samples take the time of their handling.
#[test]
fn a_wear_list_swipe_flings_the_list() {
    let (mut robot, state) = wear_list();
    fling_toward_the_end(&mut robot);
    let released = state.anchor().index;
    run_frames(&mut robot, 120);

    let rested = state.anchor().index;
    assert!(
        rested >= released + 20,
        "the fling carries the list 20 rows past row {released}, it rests on row {rested}"
    );
}

#[test]
fn a_wear_list_jump_stops_a_fling() {
    let (mut robot, state) = wear_list();
    fling_toward_the_end(&mut robot);
    let flung = state.anchor();
    run_frames(&mut robot, 1);
    assert_ne!(state.anchor(), flung, "the fling moves the list");

    state.scroll_to_item(0, 0.0);
    run_frames(&mut robot, 120);

    assert_eq!(
        state.anchor(),
        CentreAnchor {
            index: 0,
            offset: 0.0
        },
        "the list stays where the jump put it"
    );
}

#[test]
fn a_wear_list_animation_takes_the_list_from_a_fling() {
    let (mut robot, state) = wear_list();
    fling_toward_the_end(&mut robot);

    state.animate_scroll_to_item(2, 0.0);
    run_frames(&mut robot, 240);

    assert_eq!(
        state.anchor(),
        CentreAnchor {
            index: 2,
            offset: 0.0
        },
        "the animation lands on its item"
    );
}
