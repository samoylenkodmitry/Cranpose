// Where a scroll container puts the target that takes focus: with as little
// scroll as shows it by default, and 30% into the container on a TV, as
// Compose's LocalBringIntoViewSpec gives.

use cranpose_core::{CompositionLocalProvider, remember};
use cranpose_testing::{RobotTestRule, TestRenderer, create_headless_robot_test};
use cranpose_ui::{
    Box, BoxSpec, BringIntoViewSpec, Column, ColumnSpec, Configuration, HorizontalPager, Key,
    KeyEvent, LazyColumn, LazyColumnSpec, LazyItems, LazyListScope, Modifier, PagerSpec, Row,
    RowSpec, ScrollState, Size, UiModeType, local_bring_into_view_spec, local_configuration,
    rememberLazyListState, rememberPagerState,
};

const ITEMS: usize = 20;

#[derive(Clone, Copy)]
enum Container {
    LazyColumn,
    ScrollingColumn,
}

fn item(index: usize) {
    Box(
        Modifier::empty()
            .size(Size::new(400.0, 100.0))
            .focusable()
            .test_tag(format!("item {index}")),
        BoxSpec::default(),
        || {},
    );
}

fn column(container: Container) {
    let size = Modifier::empty().size(Size::new(400.0, 400.0));
    match container {
        Container::LazyColumn => {
            LazyColumn(
                size,
                rememberLazyListState(),
                LazyColumnSpec::default(),
                |scope| {
                    scope.items(LazyItems::new(ITEMS), item);
                },
            );
        }
        Container::ScrollingColumn => {
            let scroll = remember(|| ScrollState::new(0.0)).with(ScrollState::clone);
            Column(
                size.vertical_scroll(scroll, false),
                ColumnSpec::default(),
                || (0..ITEMS).for_each(item),
            );
        }
    }
}

// A 400 by 400 screen on a device of `ui_mode_type` that shows `content`
// under `spec`, or under the device's own spec without one.
fn screen(
    ui_mode_type: UiModeType,
    spec: Option<BringIntoViewSpec>,
    content: impl Fn() + Copy + 'static,
) -> RobotTestRule<TestRenderer> {
    let mut robot = create_headless_robot_test(400, 400, move || {
        let configuration = Configuration { ui_mode_type };
        CompositionLocalProvider(
            [local_configuration().provides(configuration)],
            move || match spec {
                Some(spec) => {
                    CompositionLocalProvider(
                        [local_bring_into_view_spec().provides(spec)],
                        content,
                    );
                }
                None => content(),
            },
        );
    });
    robot.wait_for_idle();
    robot
}

// Presses `key` `times` times, with the motion each starts, and answers
// where item `index` then starts on the screen along the key's axis, to the
// pixel.
fn press_then_start_of(
    robot: &mut RobotTestRule<TestRenderer>,
    key: Key,
    times: usize,
    index: usize,
) -> f32 {
    for _ in 0..times {
        assert!(
            robot.shell_mut().on_key_event(&KeyEvent::key_down(key, "")),
            "the shell takes {key:?}"
        );
        robot.wait_for_motion();
    }
    let bounds = robot
        .find_by_tag(&format!("item {index}"))
        .bounds()
        .unwrap_or_else(|| panic!("item {index} is on the screen"));
    let start = if matches!(key, Key::DirectionLeft | Key::DirectionRight) {
        bounds.x
    } else {
        bounds.y
    };
    start.round()
}

#[test]
fn a_tv_keeps_the_focused_item_30_percent_into_the_list() {
    for container in [Container::LazyColumn, Container::ScrollingColumn] {
        let mut robot = screen(UiModeType::Television, None, move || column(container));
        assert_eq!(
            press_then_start_of(&mut robot, Key::DirectionDown, 2, 1),
            100.0,
            "the list cannot scroll back past its start to put item 1 at 120"
        );
        assert_eq!(
            press_then_start_of(&mut robot, Key::DirectionDown, 1, 2),
            120.0
        );
        assert_eq!(
            press_then_start_of(&mut robot, Key::DirectionDown, 6, 8),
            120.0
        );
        assert_eq!(
            press_then_start_of(&mut robot, Key::DirectionUp, 3, 5),
            120.0
        );
        assert_eq!(
            press_then_start_of(&mut robot, Key::DirectionDown, 14, 19),
            300.0,
            "the last item ends at the list's end"
        );
    }
}

#[test]
fn elsewhere_a_list_scrolls_as_little_as_shows_the_focused_item() {
    for container in [Container::LazyColumn, Container::ScrollingColumn] {
        let mut robot = screen(UiModeType::Normal, None, move || column(container));
        assert_eq!(
            press_then_start_of(&mut robot, Key::DirectionDown, 4, 3),
            300.0,
            "an item in view does not scroll the list"
        );
        assert_eq!(
            press_then_start_of(&mut robot, Key::DirectionDown, 3, 6),
            300.0
        );
        assert_eq!(press_then_start_of(&mut robot, Key::DirectionUp, 4, 2), 0.0);
    }
}

#[test]
fn a_provided_spec_puts_the_focused_item_where_it_says() {
    let centered = BringIntoViewSpec::Pivot {
        parent_fraction: 0.5,
        child_fraction: 0.5,
    };
    let mut robot = screen(UiModeType::Normal, Some(centered), || {
        column(Container::LazyColumn);
    });
    assert_eq!(
        press_then_start_of(&mut robot, Key::DirectionDown, 6, 5),
        150.0
    );
    let mut robot = screen(
        UiModeType::Television,
        Some(BringIntoViewSpec::Default),
        || column(Container::LazyColumn),
    );
    assert_eq!(
        press_then_start_of(&mut robot, Key::DirectionDown, 6, 5),
        300.0
    );
}

#[test]
fn a_pager_on_a_tv_turns_to_the_page_focus_moves_to() {
    let mut robot = screen(UiModeType::Television, None, || {
        HorizontalPager(
            Modifier::empty().size(Size::new(400.0, 400.0)),
            rememberPagerState(0),
            PagerSpec::default(),
            4,
            |page| {
                Row(Modifier::empty(), RowSpec::default(), move || {
                    for index in [page * 2, page * 2 + 1] {
                        Box(
                            Modifier::empty()
                                .size(Size::new(200.0, 100.0))
                                .focusable()
                                .test_tag(format!("item {index}")),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                });
            },
        );
    });
    assert_eq!(
        press_then_start_of(&mut robot, Key::DirectionRight, 3, 2),
        0.0,
        "the pager rests its page at the start, not at the pivot"
    );
}
