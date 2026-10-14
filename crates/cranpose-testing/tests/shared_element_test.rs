//! `SharedElement` in a `SharedTransitionLayout` through the headless robot:
//! while `AnimatedContent` changes between two pages that share an element,
//! a copy of it moves between the two boxes the pages give it and the
//! copies in place hide; after the change the element is in place alone; a
//! seek holds the copy where the gesture is. `SharedBounds` flies both
//! pages' contents in that box.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_testing::{RobotTestRule, TestRenderer, create_headless_robot_test};
use cranpose_ui::{
    Box, BoxSpec, ContentSeek, Modifier, Rect, SeekableAnimatedContent, SharedBounds,
    SharedElement, SharedTransitionLayout, Size, Text, TextStyle, fade_in, fade_out,
};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Page {
    List,
    Detail,
}

/// Where each page puts the shared photo.
const SMALL: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 40.0,
    height: 40.0,
};
const LARGE: Rect = Rect {
    x: 100.0,
    y: 200.0,
    width: 120.0,
    height: 80.0,
};

type Controls = (MutableState<Page>, MutableState<Option<f32>>);

fn screen() -> (RobotTestRule<TestRenderer>, Rc<Cell<Option<Controls>>>) {
    screen_sharing(false)
}

/// The two pages, sharing the photo as an element, or as bounds whose
/// content each page words its own way.
fn screen_sharing(bounds: bool) -> (RobotTestRule<TestRenderer>, Rc<Cell<Option<Controls>>>) {
    let controls: Rc<Cell<Option<Controls>>> = Rc::default();
    let installed = Rc::clone(&controls);
    let mut robot = create_headless_robot_test(400, 400, move || {
        let page = rememberMutableStateOf(|| Page::List);
        let seek = rememberMutableStateOf(|| None::<f32>);
        installed.set(Some((page, seek)));
        SharedTransitionLayout(Modifier::empty().size(Size::new(400.0, 400.0)), move || {
            let toward = if page.get() == Page::List {
                Page::Detail
            } else {
                Page::List
            };
            SeekableAnimatedContent(
                page.get(),
                seek.get().map(|fraction| ContentSeek { toward, fraction }),
                |_, _| fade_in().together_with(fade_out()),
                move |page| {
                    let place = if page == Page::List { SMALL } else { LARGE };
                    Box(
                        Modifier::empty()
                            .size(Size::new(400.0, 400.0))
                            .padding_each(place.x, place.y, 0.0, 0.0),
                        BoxSpec::default(),
                        move || {
                            let size = Modifier::empty().size(Size::new(place.width, place.height));
                            if !bounds {
                                SharedElement("photo", size, || {
                                    Text("Photo", Modifier::empty(), TextStyle::default());
                                });
                                return;
                            }
                            let label = if page == Page::List {
                                "Card"
                            } else {
                                "Details"
                            };
                            SharedBounds("photo", size, move || {
                                Text(label, Modifier::empty(), TextStyle::default());
                            });
                        },
                    );
                },
            );
        });
    });
    robot.wait_for_motion();
    (robot, controls)
}

/// The boxes of the photo's text: the copies in place and any in flight.
fn photos(robot: &mut RobotTestRule<TestRenderer>) -> Vec<Rect> {
    texts(robot, "Photo")
}

fn texts(robot: &mut RobotTestRule<TestRenderer>, label: &str) -> Vec<Rect> {
    robot
        .get_all_rects()
        .into_iter()
        .filter(|(_, text)| text.as_deref() == Some(label))
        .map(|(rect, _)| rect)
        .collect()
}

fn frames(robot: &mut RobotTestRule<TestRenderer>, count: usize) {
    for _ in 0..count {
        robot.advance_time(16_666_667);
    }
}

fn between(rect: &Rect) -> bool {
    rect.x > SMALL.x && rect.x < LARGE.x && rect.y > SMALL.y && rect.y < LARGE.y
}

#[test]
fn the_shared_element_flies_between_the_pages() {
    let (mut robot, controls) = screen();
    assert_eq!(photos(&mut robot).len(), 1, "one page shows the photo");
    let (page, _) = controls.get().expect("composed");
    page.set(Page::Detail);
    frames(&mut robot, 8);
    let midway = photos(&mut robot);
    assert!(
        midway.iter().any(between),
        "a copy flies between the pages: {midway:?}"
    );
    robot.wait_for_motion();
    let landed = photos(&mut robot);
    assert_eq!(landed.len(), 1, "the photo is in place alone: {landed:?}");
    assert!((landed[0].x - LARGE.x).abs() < 1.0 && (landed[0].y - LARGE.y).abs() < 1.0);
}

#[test]
fn a_seek_holds_the_flying_copy_where_the_gesture_is() {
    let (mut robot, controls) = screen();
    let (_, seek) = controls.get().expect("composed");
    seek.set(Some(0.5));
    frames(&mut robot, 3);
    let held = photos(&mut robot);
    let flying = held
        .iter()
        .find(|rect| between(rect))
        .unwrap_or_else(|| panic!("a copy in flight: {held:?}"));
    let midway = |a: f32, b: f32| (a + b) / 2.0;
    assert!(
        (flying.x - midway(SMALL.x, LARGE.x)).abs() < 2.0
            && (flying.y - midway(SMALL.y, LARGE.y)).abs() < 2.0,
        "half way: {flying:?}"
    );
}

#[test]
fn shared_bounds_fly_both_contents_in_one_box() {
    let (mut robot, controls) = screen_sharing(true);
    let (page, _) = controls.get().expect("composed");
    page.set(Page::Detail);
    frames(&mut robot, 8);
    // One snapshot: each read waits for the robot to be idle.
    let rects = robot.get_all_rects();
    let flying = |label: &str| {
        rects
            .iter()
            .find(|(rect, text)| text.as_deref() == Some(label) && between(rect))
            .map(|(rect, _)| (rect.x, rect.y))
    };
    let (card, details) = (flying("Card"), flying("Details"));
    assert!(
        card.is_some() && card == details,
        "both contents fly between the pages in one box: {rects:?}"
    );
    robot.wait_for_motion();
    assert!(texts(&mut robot, "Card").is_empty(), "the card went");
    let landed = texts(&mut robot, "Details");
    assert_eq!(
        landed.len(),
        1,
        "the details are in place alone: {landed:?}"
    );
    assert!((landed[0].x - LARGE.x).abs() < 1.0 && (landed[0].y - LARGE.y).abs() < 1.0);
}
