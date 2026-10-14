//! `NestedScroll` through the headless robot: the container around a
//! scrolling column hears every move of a drag, takes what the column leaves
//! at its top, takes its part before the column moves, and takes the release
//! velocity before the column flings.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_testing::create_headless_robot_test;
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, Modifier, NestedScroll, NestedScrollConnection,
    NestedScrollSource, ScrollAxis, ScrollState, Size,
};

/// Takes `pre` of every move before the column, everything the column
/// leaves when `post` is set, and the whole release velocity when `fling` is
/// set; and adds up what it took.
#[derive(Default)]
struct Container {
    pre: f32,
    post: bool,
    fling: bool,
    took_before: Cell<f32>,
    took_after: Cell<f32>,
    velocity: Cell<f32>,
}

impl NestedScrollConnection for Container {
    fn on_pre_scroll(&self, axis: ScrollAxis, available: f32, _: NestedScrollSource) -> f32 {
        assert_eq!(axis, ScrollAxis::Vertical);
        let took = available * self.pre;
        self.took_before.set(self.took_before.get() + took);
        took
    }

    fn on_post_scroll(&self, _: ScrollAxis, _: f32, available: f32, _: NestedScrollSource) -> f32 {
        if !self.post {
            return 0.0;
        }
        self.took_after.set(self.took_after.get() + available);
        available
    }

    fn on_pre_fling(&self, _: ScrollAxis, velocity: f32) -> f32 {
        self.velocity.set(velocity);
        if self.fling { velocity } else { 0.0 }
    }
}

struct Screen {
    robot: cranpose_testing::RobotTestRule<cranpose_testing::TestRenderer>,
    scroll: ScrollState,
    container: Rc<Container>,
}

fn screen(container: Container) -> Screen {
    let container = Rc::new(container);
    let connection: Rc<dyn NestedScrollConnection> = container.clone();
    let captured: Rc<RefCell<Option<ScrollState>>> = Rc::default();
    let installed = Rc::clone(&captured);
    let robot = create_headless_robot_test(200, 400, move || {
        let scroll = cranpose_ui::rememberScrollState!(0.0);
        *installed.borrow_mut() = Some(scroll);
        NestedScroll(connection.clone(), move || {
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
    });
    let scroll = (*captured.borrow()).expect("the column is composed");
    Screen {
        robot,
        scroll,
        container,
    }
}

#[test]
fn the_container_takes_what_the_column_leaves_at_its_top() {
    let mut screen = screen(Container {
        post: true,
        ..Container::default()
    });
    screen.robot.drag(100.0, 100.0, 100.0, 220.0);
    assert_eq!(screen.scroll.value(), 0.0, "the column stays at its top");
    let took = screen.container.took_after.get();
    assert!(
        took > 60.0,
        "the container took the downward pull the column could not use: {took}"
    );
}

#[test]
fn the_container_takes_its_part_before_the_column_moves() {
    let mut whole = screen(Container::default());
    whole.robot.drag(100.0, 300.0, 100.0, 100.0);
    let alone = whole.scroll.value();

    let mut shared = screen(Container {
        pre: 0.5,
        ..Container::default()
    });
    shared.robot.drag(100.0, 300.0, 100.0, 100.0);
    let with_container = shared.scroll.value();
    assert!(alone > 100.0, "the column scrolled on its own: {alone}");
    assert!(
        (with_container - alone / 2.0).abs() < alone * 0.2,
        "the column moved by what the container left: {with_container} of {alone}"
    );
    assert!(
        shared.container.took_before.get() < 0.0,
        "an upward drag moves the content up: a negative amount"
    );
}

/// How far the column moved after the release.
fn moved_after_release(container: Container) -> (f32, f32) {
    let mut screen = screen(container);
    screen.robot.drag(100.0, 300.0, 100.0, 100.0);
    let at_release = screen.scroll.value();
    for _ in 0..30 {
        screen.robot.advance_time(16_000_000);
        screen.robot.wait_for_idle();
    }
    (
        screen.scroll.value() - at_release,
        screen.container.velocity.get(),
    )
}

#[test]
fn the_container_takes_the_release_velocity_before_the_column_flings() {
    let (flung, velocity) = moved_after_release(Container::default());
    assert!(flung > 0.0, "a fast drag flings the column: {flung}");
    assert!(
        velocity < 0.0,
        "an upward release moves the content up: {velocity}"
    );

    let (flung, _) = moved_after_release(Container {
        fling: true,
        ..Container::default()
    });
    assert_eq!(flung, 0.0, "no fling with the velocity the container took");
}
