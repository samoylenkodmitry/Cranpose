use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use cranpose_foundation::PointerEventKind;
use cranpose_testing::create_headless_robot_test;
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, Modifier, PointerInputScope, ScrollState, Size,
};

#[test]
fn headless_robot_dispatches_pointer_and_accessibility_activation() {
    let count = Rc::new(Cell::new(0));
    let recorded = Rc::clone(&count);
    let mut robot = create_headless_robot_test(200, 100, move || {
        let recorded = Rc::clone(&recorded);
        Box(
            Modifier::empty()
                .size(Size::new(100.0, 48.0))
                .content_description("Increment")
                .clickable(move |_| recorded.set(recorded.get() + 1)),
            BoxSpec::default(),
            || {},
        );
    });
    assert!(robot.click_at(50.0, 24.0));
    assert_eq!(count.get(), 1);
    robot.shell_mut().set_semantics_enabled(true);
    let tree = cranpose_testing::placed_semantics_from_shell(robot.shell_mut()).expect("control");
    let control = tree
        .flatten()
        .into_iter()
        .find(|node| node.clickable)
        .expect("button");
    assert!(
        robot
            .shell_mut()
            .accessibility_activate(control.node_id, None)
    );
    assert_eq!(count.get(), 2);
    assert!(!robot.click_at(150.0, 75.0));
    assert_eq!(count.get(), 2);
}

// A column of 1,000 rows of 40 points that scrolls in a 200 by 400 window.
// With `busy`, each pointer move takes that much of the machine's time before
// the column sees it, as a loaded machine stretches each frame.
fn scrolling_column(
    busy: Option<Duration>,
) -> (
    cranpose_testing::RobotTestRule<cranpose_testing::TestRenderer>,
    ScrollState,
) {
    let captured: Rc<RefCell<Option<ScrollState>>> = Rc::default();
    let installed = Rc::clone(&captured);
    let robot = create_headless_robot_test(200, 400, move || {
        let scroll = cranpose_ui::rememberScrollState!(0.0);
        *installed.borrow_mut() = Some(scroll);
        let slow_moves =
            Modifier::empty().pointer_input(busy, move |scope: PointerInputScope| async move {
                scope
                    .await_pointer_event_scope(|events| async move {
                        loop {
                            let event = events.await_pointer_event().await;
                            if let (PointerEventKind::Move, Some(busy)) = (event.kind, busy) {
                                std::thread::sleep(busy);
                            }
                        }
                    })
                    .await;
            });
        Column(
            slow_moves.fill_max_size().vertical_scroll(scroll, false),
            ColumnSpec::default(),
            || {
                for _ in 0..1_000 {
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

// The robot times a drag's moves by its own frame clock. A machine that takes
// 10 ms for each move, more than the 8 ms between the moves, still ends the
// drag with the same velocity, so the fling after it stands at the same place
// half a second later.
#[test]
fn a_drag_flings_as_far_on_a_busy_machine_as_on_an_idle_one() {
    let flung = |busy| {
        let (mut robot, scroll) = scrolling_column(busy);
        robot.drag(100.0, 300.0, 100.0, 100.0);
        for _ in 0..30 {
            robot.advance_time(16_666_667);
        }
        scroll.value()
    };
    let idle = flung(None);
    assert!(
        idle > 400.0,
        "the drag and its fling scrolled {idle} points"
    );
    assert_eq!(flung(Some(Duration::from_millis(10))), idle);
}
