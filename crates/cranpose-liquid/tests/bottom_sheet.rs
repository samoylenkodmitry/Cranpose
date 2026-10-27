//! `LiquidModalBottomSheet` through the headless robot: it opens from below,
//! a tap on it keeps it open, and a scrim tap, a drag down or `hide()` slide
//! it out before `on_dismiss`, as do Escape, the back gesture and a screen
//! reader's dismiss; a list in it scrolls first and hands the sheet the drag
//! it leaves at its top.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::rememberMutableStateOf;
use cranpose_liquid::prelude::*;
use cranpose_testing::{
    RobotTestRule, TestRenderer, create_headless_robot_test, robot_assertions::find_semantics_node,
};
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, Key, KeyEvent, Modifier, Point, ScrollState, Size, Text,
    TextStyle,
};

const WIDTH: f32 = 300.0;
const HEIGHT: f32 = 600.0;

struct Screen {
    robot: RobotTestRule<TestRenderer>,
    dismissed: Rc<Cell<u32>>,
    scroll: Rc<RefCell<Option<ScrollState>>>,
}

/// A sheet with a line of text and a Done button, or with a scrolling list.
fn screen(with_list: bool) -> Screen {
    let dismissed = Rc::new(Cell::new(0));
    let counted = Rc::clone(&dismissed);
    let scroll: Rc<RefCell<Option<ScrollState>>> = Rc::default();
    let installed = Rc::clone(&scroll);
    let robot = create_headless_robot_test(WIDTH as u32, HEIGHT as u32, move || {
        let counted = Rc::clone(&counted);
        let installed = Rc::clone(&installed);
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let open = rememberMutableStateOf(|| true);
            let sheet = rememberLiquidSheetState(false);
            let list_scroll = cranpose_ui::rememberScrollState!(0.0);
            *installed.borrow_mut() = Some(list_scroll);
            if !open.get() {
                return;
            }
            let counted = Rc::clone(&counted);
            let done = sheet.clone();
            LiquidModalBottomSheet(
                sheet,
                move || {
                    counted.set(counted.get() + 1);
                    open.set(false);
                },
                move || {
                    if with_list {
                        SheetList(list_scroll);
                    } else {
                        Text("Sheet content", Modifier::empty(), TextStyle::default());
                        let done = done.clone();
                        Box(
                            Modifier::empty()
                                .size(Size::new(120.0, 48.0))
                                .clickable(move |_| done.hide()),
                            BoxSpec::default(),
                            || {
                                Text("Done", Modifier::empty(), TextStyle::default());
                            },
                        );
                    }
                },
            );
        });
    });
    let mut screen = Screen {
        robot,
        dismissed,
        scroll,
    };
    screen.robot.wait_for_motion();
    screen
}

#[cranpose_macros::composable]
fn SheetList(scroll: ScrollState) {
    Column(
        Modifier::empty()
            .fill_max_width()
            .height(240.0)
            .vertical_scroll(scroll, false),
        ColumnSpec::default(),
        || {
            for row in 0..20 {
                Text(
                    format!("Row {row}"),
                    Modifier::empty().height(40.0),
                    TextStyle::default(),
                );
            }
        },
    );
}

impl Screen {
    fn center_of(&mut self, text: &str) -> Option<Point> {
        self.robot.find_by_text(text).center()
    }

    fn is_open(&mut self) -> bool {
        self.robot.find_by_text("Sheet content").exists()
            || self.robot.find_by_text("Row 0").exists()
    }

    fn scroll(&self) -> f32 {
        (*self.scroll.borrow()).map_or(0.0, |scroll| scroll.value())
    }
}

#[test]
fn the_sheet_opens_at_the_bottom_and_a_tap_on_it_keeps_it_open() {
    let mut screen = screen(false);
    let text = screen
        .center_of("Sheet content")
        .expect("the sheet is open");
    assert!(
        text.y > HEIGHT / 2.0,
        "the sheet sits at the bottom: {text:?}"
    );
    screen.robot.click_at(text.x, text.y);
    screen.robot.wait_for_motion();
    assert!(screen.is_open());
    assert_eq!(screen.dismissed.get(), 0);
}

#[test]
fn a_tap_on_the_scrim_slides_the_sheet_out_then_dismisses_it() {
    let mut screen = screen(false);
    screen.robot.click_at(WIDTH / 2.0, 40.0);
    screen.robot.wait_for_motion();
    assert_eq!(screen.dismissed.get(), 1);
    assert!(!screen.is_open());
}

#[test]
fn a_drag_down_on_the_handle_dismisses_the_sheet() {
    let mut screen = screen(false);
    let text = screen
        .center_of("Sheet content")
        .expect("the sheet is open");
    // The handle is above the content.
    let handle = text.y - 20.0;
    screen.robot.drag(text.x, handle, text.x, HEIGHT - 1.0);
    screen.robot.wait_for_motion();
    assert_eq!(screen.dismissed.get(), 1);
}

#[test]
fn a_short_slow_drag_leaves_the_sheet_open() {
    let mut screen = screen(false);
    let text = screen
        .center_of("Sheet content")
        .expect("the sheet is open");
    let shell = screen.robot.shell_mut();
    shell.set_cursor_at_time(text.x, text.y, Some(0));
    shell.pointer_pressed_at_time(Some(0));
    for step in 1..=10 {
        shell.set_cursor_at_time(text.x, text.y + step as f32 * 3.0, Some(step * 16));
        shell.update();
    }
    // Held still before the release, so the release has no velocity.
    shell.pointer_released_at_time(Some(400));
    screen.robot.wait_for_motion();
    assert_eq!(screen.dismissed.get(), 0);
    let settled = screen
        .center_of("Sheet content")
        .expect("the sheet is open");
    assert!(
        (settled.y - text.y).abs() < 1.0,
        "the sheet went back to rest: {settled:?} from {text:?}"
    );
}

#[test]
fn hide_slides_the_sheet_out_before_it_dismisses() {
    let mut screen = screen(false);
    let done = screen.center_of("Done").expect("the sheet shows Done");
    screen.robot.mouse_move(done.x, done.y);
    screen.robot.mouse_down();
    screen.robot.mouse_up();
    assert_eq!(screen.dismissed.get(), 0, "the sheet is still sliding out");
    screen.robot.wait_for_motion();
    assert_eq!(screen.dismissed.get(), 1);
}

#[test]
fn a_list_scrolls_before_the_sheet_moves() {
    let mut screen = screen(true);
    let row = screen.center_of("Row 2").expect("the list is open");
    screen.robot.drag(row.x, row.y, row.x, row.y - 100.0);
    screen.robot.wait_for_motion();
    assert!(
        screen.scroll() > 50.0,
        "the list scrolled: {}",
        screen.scroll()
    );
    assert_eq!(screen.dismissed.get(), 0);
}

#[test]
fn a_list_at_its_top_hands_the_drag_down_to_the_sheet() {
    let mut screen = screen(true);
    let row = screen.center_of("Row 0").expect("the list is open");
    screen.robot.drag(row.x, row.y, row.x, HEIGHT - 4.0);
    screen.robot.wait_for_motion();
    assert_eq!(screen.scroll(), 0.0);
    assert_eq!(screen.dismissed.get(), 1);
}

#[test]
fn escape_and_the_back_gesture_slide_the_sheet_out() {
    for escape in [true, false] {
        let mut screen = screen(false);
        let shell = screen.robot.shell_mut();
        let handled = if escape {
            shell.on_key_event(&KeyEvent::key_down(Key::Escape, ""))
        } else {
            shell.dismiss_top_modal()
        };
        assert!(handled, "the open sheet takes the request");
        screen.robot.wait_for_motion();
        assert_eq!(screen.dismissed.get(), 1);
    }
}

#[test]
fn a_screen_reader_dismisses_the_sheet() {
    let mut screen = screen(false);
    screen.robot.shell_mut().set_semantics_enabled(true);
    screen.robot.wait_for_idle();
    let tree = screen
        .robot
        .shell_mut()
        .semantics_tree()
        .expect("semantics");
    let dismiss = find_semantics_node(tree.root(), &|node| node.details().dismiss.is_some())
        .and_then(|node| node.details().dismiss.clone())
        .expect("the sheet offers a dismiss action");
    assert!(dismiss.invoke());
    screen.robot.wait_for_motion();
    assert_eq!(screen.dismissed.get(), 1);
}

/// A sheet taller than half the window, and its state.
fn tall_screen(skip_partially_expanded: bool) -> (Screen, Rc<RefCell<Option<LiquidSheetState>>>) {
    let dismissed = Rc::new(Cell::new(0));
    let counted = Rc::clone(&dismissed);
    let captured: Rc<RefCell<Option<LiquidSheetState>>> = Rc::default();
    let installed = Rc::clone(&captured);
    let robot = create_headless_robot_test(WIDTH as u32, HEIGHT as u32, move || {
        let counted = Rc::clone(&counted);
        let installed = Rc::clone(&installed);
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let open = rememberMutableStateOf(|| true);
            let sheet = rememberLiquidSheetState(skip_partially_expanded);
            *installed.borrow_mut() = Some(sheet.clone());
            if !open.get() {
                return;
            }
            let counted = Rc::clone(&counted);
            LiquidModalBottomSheet(
                sheet,
                move || {
                    counted.set(counted.get() + 1);
                    open.set(false);
                },
                || {
                    Text("Tall content", Modifier::empty(), TextStyle::default());
                    Box(
                        Modifier::empty().size(Size::new(WIDTH, 1000.0)),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        });
    });
    let mut screen = Screen {
        robot,
        dismissed,
        scroll: Rc::default(),
    };
    screen.robot.wait_for_motion();
    (screen, captured)
}

fn state(captured: &Rc<RefCell<Option<LiquidSheetState>>>) -> LiquidSheetState {
    captured.borrow().clone().expect("composed")
}

#[test]
fn a_tall_sheet_opens_to_half_the_window_and_a_drag_up_opens_it_fully() {
    let (mut screen, captured) = tall_screen(false);
    assert!(!state(&captured).is_expanded());
    let below_half = HEIGHT / 2.0 + 20.0;
    screen
        .robot
        .drag(WIDTH / 2.0, below_half, WIDTH / 2.0, 40.0);
    screen.robot.wait_for_motion();
    assert!(state(&captured).is_expanded());
    // The sheet now reaches above the half of the window.
    screen.robot.click_at(WIDTH / 2.0, HEIGHT / 4.0);
    screen.robot.wait_for_motion();
    assert_eq!(screen.dismissed.get(), 0);
}

#[test]
fn a_drag_down_from_half_the_window_closes_the_sheet() {
    let (mut screen, _) = tall_screen(false);
    screen
        .robot
        .drag(WIDTH / 2.0, HEIGHT / 2.0 + 20.0, WIDTH / 2.0, HEIGHT - 1.0);
    screen.robot.wait_for_motion();
    assert_eq!(screen.dismissed.get(), 1);
}

#[test]
fn a_sheet_that_skips_half_opens_fully() {
    let (_screen, captured) = tall_screen(true);
    assert!(state(&captured).is_expanded());
}

#[test]
fn a_reader_opens_and_lowers_a_tall_sheet() {
    let (mut screen, captured) = tall_screen(false);
    screen.robot.shell_mut().set_semantics_enabled(true);
    screen.robot.wait_for_idle();
    let action = |screen: &mut Screen, expand: bool| {
        let tree = screen
            .robot
            .shell_mut()
            .semantics_tree()
            .expect("semantics");
        let found = find_semantics_node(tree.root(), &|node| {
            if expand {
                node.details().expand.is_some()
            } else {
                node.details().collapse.is_some()
            }
        })
        .expect("the sheet offers the action");
        let details = found.details();
        if expand {
            details.expand.clone()
        } else {
            details.collapse.clone()
        }
        .expect("action")
    };
    assert!(action(&mut screen, true).invoke());
    screen.robot.wait_for_motion();
    assert!(state(&captured).is_expanded());
    assert!(action(&mut screen, false).invoke());
    screen.robot.wait_for_motion();
    assert!(!state(&captured).is_expanded());
}
