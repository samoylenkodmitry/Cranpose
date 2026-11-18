// `LiquidContextMenu` through the headless robot: a long press lifts the
// content at 1.15 times its size and opens the menu 16 points below it, or
// above it near the bottom edge; a menu that cannot centre on the lifted
// content lines up with its edge; both keep inside the safe area; the
// release that ends the long press keeps the menu open; a tap on an action
// runs it and closes the menu; a tap elsewhere closes it and runs nothing;
// the content shows exactly once in every frame of the lift and the return,
// and comes back with its state; a custom preview lifts at its own size;
// readers open the menu through the long press action.

use std::{
    rc::Rc,
    time::{Duration, Instant},
};

use cranpose_core::{CompositionLocalProvider, mutableStateOf, remember};
use cranpose_liquid::prelude::*;
use cranpose_testing::robot_assertions::assert_approx_eq;
use cranpose_ui::{
    Alignment, EdgeInsets, LayoutBox, Modifier, Point, Rect, Size, local_safe_area_insets,
    widgets::{Box, BoxSpec, Text},
};

use crate::reader::{
    Log, Robot, hold, liquid_screen, long_press_action, node, nodes, tap, tap_node,
};

const TILE: Size = Size {
    width: 160.0,
    height: 100.0,
};
const PREVIEW: Size = Size {
    width: 240.0,
    height: 160.0,
};
const FRAME_NANOS: u64 = 16_666_667;

// Where the tile stands and what it lifts.
#[derive(Clone, Copy)]
struct Layout {
    alignment: Alignment,
    offset: Point,
    insets: EdgeInsets,
    // Whether the long press lifts a 240 by 160 preview in place of the tile.
    preview: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            alignment: Alignment::CENTER,
            offset: Point::ZERO,
            insets: EdgeInsets::default(),
            preview: false,
        }
    }
}

// A 160 by 100 tile whose context menu holds Copy, Share and Delete, in a
// 400 by 800 window. The tile counts its taps.
fn tile(layout: Layout) -> (Robot, Log) {
    liquid_screen(move |log| {
        let record = Rc::clone(log);
        CompositionLocalProvider([local_safe_area_insets().provides(layout.insets)], || {
            Box(
                Modifier::empty().fill_max_size(),
                BoxSpec::default().content_alignment(layout.alignment),
                move || {
                    let record = Rc::clone(&record);
                    let actions = move |scope: &LiquidMenuScope| {
                        for (label, entry) in [("Copy", "copy"), ("Share", "share")] {
                            let record = Rc::clone(&record);
                            scope.item(LiquidMenuItem::new(label), move || {
                                record.borrow_mut().push(entry);
                            });
                        }
                        let record = Rc::clone(&record);
                        scope.item(LiquidMenuItem::new("Delete").destructive(), move || {
                            record.borrow_mut().push("delete");
                        });
                    };
                    let modifier = Modifier::empty()
                        .offset(layout.offset.x, layout.offset.y)
                        .size(TILE);
                    if layout.preview {
                        LiquidContextMenuWithPreview(
                            modifier,
                            "Photo options",
                            actions,
                            || {
                                Box(Modifier::empty().size(PREVIEW), BoxSpec::default(), || {});
                            },
                            counter,
                        );
                    } else {
                        LiquidContextMenu(
                            modifier,
                            LiquidShape::RoundedRect(16.0),
                            "Photo options",
                            actions,
                            counter,
                        );
                    }
                },
            );
        });
    })
}

// The tile's content: a count of its taps, kept in a remembered state.
fn counter() {
    let taps = remember(|| mutableStateOf(0_u32)).with(|taps| *taps);
    Box(
        Modifier::empty()
            .fill_max_size()
            .clickable(move |_| taps.set(taps.get() + 1)),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            Text(
                format!("Taps {}", taps.get()),
                Modifier::empty(),
                liquid_typography().body,
            );
        },
    );
}

// Holds the pointer on (`x`, `y`) past the long press time, then lifts it.
fn long_press(robot: &mut Robot, x: f32, y: f32) {
    hold(robot, x, y, 600);
    robot.mouse_up();
    robot.wait_for_motion();
}

fn shown(robot: &mut Robot, label: &str) -> usize {
    nodes(robot)
        .iter()
        .filter(|node| node.label.as_deref() == Some(label))
        .count()
}

// Where each laid-out node that shows `text` stands, the overlay's
// included, before offsets and graphics-layer transforms. Readers see only
// the open menu, so this reads the layout tree.
fn placed_text(robot: &mut Robot, text: &str) -> Vec<Rect> {
    fn collect(node: &LayoutBox, text: &str, found: &mut Vec<Rect>) {
        if node
            .node_data
            .modifier_slices()
            .text_content()
            .is_some_and(|shown| shown == text)
        {
            found.push(node.rect);
        }
        for child in &node.children {
            collect(child, text, found);
        }
    }
    robot.shell_mut().with_layout_tree(|tree| {
        let mut found = Vec::new();
        if let Some(tree) = tree {
            collect(tree.root(), text, &mut found);
        }
        found
    })
}

// Frames on until the menu has closed and the content stands at `place`
// again; the closing motion waits on the clock, so the frames take real time.
// Every frame shows the content once.
fn settle_back(robot: &mut Robot, text: &str, place: Rect) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        robot.advance_time(FRAME_NANOS);
        let placed = placed_text(robot, text);
        assert_eq!(placed.len(), 1, "a frame of the return shows {text} once");
        if placed[0] == place && shown(robot, "Copy") == 0 {
            return;
        }
        assert!(Instant::now() < deadline, "the content came back");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_long_press_opens_the_menu_16_points_below_the_lifted_content() {
    let (mut robot, log) = tile(Layout::default());
    assert_eq!(shown(&mut robot, "Copy"), 0, "the menu starts closed");
    long_press(&mut robot, 200.0, 400.0);
    // The lifted tile spans 400 ± 57.5; the menu's rows start 10 points
    // below its top edge.
    let copy = node(&mut robot, "Copy").target_bounds();
    assert_approx_eq(
        copy.y,
        400.0 + TILE.height * 1.15 * 0.5 + 16.0 + 10.0,
        1.0,
        "Copy's row starts 26 points below the lifted tile",
    );
    assert_approx_eq(copy.x, 75.0, 0.5, "the menu centres under the tile");
    let delete = node(&mut robot, "Delete").target_bounds();
    assert!(
        delete.y > copy.y,
        "the rows keep their order below the tile"
    );
    assert!(log.borrow().is_empty(), "the release ran no action");
}

#[test]
fn near_the_bottom_edge_the_menu_opens_above_the_lifted_content() {
    let (mut robot, _) = tile(Layout {
        alignment: Alignment::BOTTOM_CENTER,
        ..Layout::default()
    });
    long_press(&mut robot, 200.0, 750.0);
    let delete = node(&mut robot, "Delete").target_bounds();
    let copy = node(&mut robot, "Copy").target_bounds();
    // The lifted tile keeps 16 points above the window's bottom edge.
    let lifted_top = 800.0 - 16.0 - TILE.height * 1.15;
    assert_approx_eq(
        delete.y + delete.height,
        lifted_top - 16.0 - 10.0,
        1.0,
        "Delete's row ends 26 points above the lifted tile",
    );
    assert!(
        copy.y < delete.y,
        "the rows keep their order above the tile"
    );
}

#[test]
fn the_lifted_content_and_the_menu_keep_inside_the_safe_area() {
    let (mut robot, _) = tile(Layout {
        alignment: Alignment::BOTTOM_CENTER,
        insets: EdgeInsets {
            bottom: 34.0,
            ..EdgeInsets::default()
        },
        ..Layout::default()
    });
    long_press(&mut robot, 200.0, 750.0);
    let delete = node(&mut robot, "Delete").target_bounds();
    let lifted_top = 800.0 - 34.0 - 16.0 - TILE.height * 1.15;
    assert_approx_eq(
        delete.y + delete.height,
        lifted_top - 16.0 - 10.0,
        1.0,
        "the lifted tile keeps 16 points above the home indicator's inset",
    );
}

#[test]
fn a_menu_that_cannot_centre_lines_up_with_the_lifted_edge() {
    // The tile's centre sits at x 129: the lifted tile spans 37 to 221, and a
    // menu centred under it would start at 4, inside the 16 point margin.
    let (mut robot, _) = tile(Layout {
        offset: Point::new(-71.0, 0.0),
        ..Layout::default()
    });
    long_press(&mut robot, 129.0, 400.0);
    let copy = node(&mut robot, "Copy").target_bounds();
    assert_approx_eq(
        copy.x,
        129.0 - TILE.width * 1.15 * 0.5,
        0.5,
        "the menu lines up with the lifted tile's left edge",
    );
}

#[test]
fn a_custom_preview_lifts_at_its_own_size_with_the_menu_below_it() {
    let (mut robot, _) = tile(Layout {
        preview: true,
        ..Layout::default()
    });
    long_press(&mut robot, 200.0, 400.0);
    let copy = node(&mut robot, "Copy").target_bounds();
    assert_approx_eq(
        copy.y,
        400.0 + PREVIEW.height * 0.5 + 16.0 + 10.0,
        1.0,
        "Copy's row starts 26 points below the 160 point preview",
    );
}

#[test]
fn a_tap_on_an_action_runs_it_and_closes_the_menu() {
    let (mut robot, log) = tile(Layout::default());
    long_press(&mut robot, 200.0, 400.0);
    tap_node(&mut robot, "Share");
    robot.wait_for_motion();
    assert_eq!(*log.borrow(), ["share"]);
    assert_eq!(shown(&mut robot, "Copy"), 0, "the menu closed");
}

#[test]
fn a_tap_outside_closes_the_menu_and_runs_nothing() {
    let (mut robot, log) = tile(Layout::default());
    long_press(&mut robot, 200.0, 400.0);
    tap(&mut robot, 20.0, 80.0);
    robot.wait_for_motion();
    assert_eq!(shown(&mut robot, "Copy"), 0, "the menu closed");
    assert!(log.borrow().is_empty(), "no action ran");
}

#[test]
fn the_content_shows_once_in_every_frame_of_the_lift_and_the_return() {
    let (mut robot, _) = tile(Layout::default());
    let place = placed_text(&mut robot, "Taps 0")[0];
    robot.mouse_move(200.0, 400.0);
    robot.mouse_down();
    for frame in 0..40 {
        robot.advance_time(FRAME_NANOS);
        assert_eq!(
            placed_text(&mut robot, "Taps 0").len(),
            1,
            "frame {frame} of the lift shows the content once"
        );
    }
    robot.mouse_up();
    assert!(shown(&mut robot, "Copy") > 0, "the menu opened");
    tap(&mut robot, 20.0, 80.0);
    settle_back(&mut robot, "Taps 0", place);
}

#[test]
fn the_content_comes_back_with_its_state_and_opens_again() {
    let (mut robot, _) = tile(Layout::default());
    tap(&mut robot, 200.0, 400.0);
    let place = placed_text(&mut robot, "Taps 1")[0];
    long_press(&mut robot, 200.0, 400.0);
    tap(&mut robot, 20.0, 80.0);
    settle_back(&mut robot, "Taps 1", place);
    tap(&mut robot, 200.0, 400.0);
    assert_eq!(
        placed_text(&mut robot, "Taps 2").len(),
        1,
        "the tile takes taps again"
    );
    long_press(&mut robot, 200.0, 400.0);
    assert!(shown(&mut robot, "Copy") > 0, "the menu opens again");
}

#[test]
fn readers_open_the_menu_through_the_long_press_action() {
    let (mut robot, _) = tile(Layout::default());
    let open = long_press_action(&mut robot, "Photo options");
    assert!(open.invoke(), "the tile took the long press");
    robot.wait_for_motion();
    assert!(shown(&mut robot, "Copy") > 0, "the menu opened");
}
