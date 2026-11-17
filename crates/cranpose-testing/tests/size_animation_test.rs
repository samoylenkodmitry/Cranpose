// Size animations through the shell: `AnimatedVisibility` with expand and
// shrink transitions, and `Modifier::animate_content_size`. The measured
// size of the animating box moves across frames, its neighbor in the parent
// moves with it, and a change while it moves turns it around from the size
// that shows.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_testing::{
    PlacedSemanticsNode, create_headless_robot_test, placed_semantics_from_shell,
    robot::{RobotTestRule, TestRenderer},
};
use cranpose_ui::{
    AnimatedVisibility, Box, BoxSpec, Column, ColumnSpec, EnterTransition, ExitTransition,
    GraphicsLayer, LayoutBox, LayoutDirection, Modifier, ProvideLayoutDirection, Rect, Row,
    RowSpec, Size, VerticalAlignment, expand_horizontally, expand_vertically,
    expand_vertically_with, shrink_horizontally, shrink_vertically, shrink_vertically_with,
    slide_in_vertically,
};

const CONTENT: Size = Size::new(40.0, 60.0);
const NEIGHBOUR: Size = Size::new(10.0, 10.0);

#[derive(Clone, Copy)]
enum Axis {
    Vertical,
    Horizontal,
}

struct Screen {
    robot: RobotTestRule<TestRenderer>,
    state: MutableState<bool>,
}

// What a frame shows: the animating panel, its content when it is composed,
// and the neighbor after it.
struct Frame {
    panel: Rect,
    content: Option<Rect>,
    next: Rect,
}

fn tagged(tag: &'static str, size: Size) {
    Box(
        Modifier::empty().size(size).test_tag(tag),
        BoxSpec::default(),
        || {},
    );
}

// A neighbor, the panel that `panel(state)` composes with the tag "panel",
// and a neighbor, along `axis`, in `direction`.
fn screen(
    axis: Axis,
    direction: LayoutDirection,
    initial: bool,
    panel: impl Fn(bool) + 'static,
) -> Screen {
    let slot: Rc<Cell<Option<MutableState<bool>>>> = Rc::default();
    let installed = Rc::clone(&slot);
    let panel = Rc::new(panel);
    let mut robot = create_headless_robot_test(300, 300, move || {
        let state = rememberMutableStateOf(|| initial);
        installed.set(Some(state));
        let panel = Rc::clone(&panel);
        ProvideLayoutDirection(direction, move || {
            let stacked = move || {
                tagged("previous", NEIGHBOUR);
                panel(state.get());
                tagged("next", NEIGHBOUR);
            };
            match axis {
                Axis::Vertical => Column(Modifier::empty(), ColumnSpec::default(), stacked),
                Axis::Horizontal => Row(Modifier::empty(), RowSpec::default(), stacked),
            };
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    Screen {
        robot,
        state: slot.get().expect("composed"),
    }
}

// The panel of an `AnimatedVisibility` of `CONTENT` with these transitions.
fn visibility(axis: Axis, enter: EnterTransition, exit: ExitTransition) -> impl Fn(bool) + 'static {
    move |visible| {
        let (enter, exit) = (enter.clone(), exit.clone());
        let content = move || {
            AnimatedVisibility(visible, enter.clone(), exit.clone(), || {
                tagged("content", CONTENT);
            });
        };
        let panel = Modifier::empty().test_tag("panel");
        match axis {
            Axis::Vertical => Column(panel, ColumnSpec::default(), content),
            Axis::Horizontal => Row(panel, RowSpec::default(), content),
        };
    }
}

fn layers(node: &LayoutBox, found: &mut Vec<GraphicsLayer>) {
    if let Some(layer) = node.node_data.modifier_slices().graphics_layer() {
        found.push(layer.into_owned());
    }
    for child in &node.children {
        layers(child, found);
    }
}

impl Screen {
    // What the shell lays out now: reading it runs no frame, as a finder does.
    fn frame(&mut self) -> Frame {
        let placed = placed_semantics_from_shell(self.robot.shell_mut()).expect("placed");
        let find = |tag: &str| {
            placed
                .find_by_tag(tag)
                .map(PlacedSemanticsNode::target_bounds)
        };
        Frame {
            panel: find("panel").unwrap_or_default(),
            content: find("content"),
            next: find("next").expect("the neighbor after the panel"),
        }
    }

    // Sets the state, and runs `count` frames after that.
    fn run(&mut self, visible: bool, count: usize) -> Vec<Frame> {
        self.state.set(visible);
        self.more(count)
    }

    fn more(&mut self, count: usize) -> Vec<Frame> {
        (0..count)
            .map(|_| {
                self.robot.advance_time(16_666_667);
                self.frame()
            })
            .collect()
    }

    fn rest(&mut self) -> Frame {
        self.robot.wait_for_motion();
        self.frame()
    }

    // The layers that graphics-layer modifiers give the layout now.
    fn layers(&mut self) -> Vec<GraphicsLayer> {
        let mut found = Vec::new();
        self.robot.shell_mut().with_layout_tree(|tree| {
            if let Some(tree) = tree {
                layers(tree.root(), &mut found);
            }
        });
        found
    }
}

fn panel_heights(frames: &[Frame]) -> Vec<f32> {
    frames.iter().map(|frame| frame.panel.height).collect()
}

fn panel_widths(frames: &[Frame]) -> Vec<f32> {
    frames.iter().map(|frame| frame.panel.width).collect()
}

fn rises(values: &[f32]) -> bool {
    values.windows(2).all(|pair| pair[0] <= pair[1])
}

fn falls(values: &[f32]) -> bool {
    values.windows(2).all(|pair| pair[0] >= pair[1])
}

// Whether some frame has a value strictly between `low` and `high`.
fn passes(values: &[f32], low: f32, high: f32) -> bool {
    values.iter().any(|&value| value > low && value < high)
}

#[test]
fn an_expanding_container_grows_from_nothing_and_the_sibling_below_moves_with_it() {
    let mut screen = screen(
        Axis::Vertical,
        LayoutDirection::Ltr,
        false,
        visibility(Axis::Vertical, expand_vertically(), shrink_vertically()),
    );
    let first = screen.frame();
    assert_eq!(first.panel.height, 0.0, "nothing shows at first");
    assert_eq!(first.next.y, NEIGHBOUR.height);

    let frames = screen.run(true, 40);
    let heights = panel_heights(&frames);
    assert!(rises(&heights), "the height never falls: {heights:?}");
    assert!(
        passes(&heights, 0.0, CONTENT.height),
        "the height passes between nothing and the content's: {heights:?}"
    );
    for frame in &frames {
        assert_eq!(
            frame.next.y,
            NEIGHBOUR.height + frame.panel.height,
            "the sibling below sits under the panel"
        );
        let content = frame.content.expect("the content is composed");
        assert_eq!(
            (content.width, content.height),
            (CONTENT.width, CONTENT.height),
            "the content keeps its full size"
        );
        assert_eq!(
            content.y + content.height,
            frame.panel.y + frame.panel.height,
            "the bottom of the content stays on the bottom of the panel"
        );
    }
    assert_eq!(screen.frame().panel.height, CONTENT.height);
}

#[test]
fn a_shrinking_container_falls_to_nothing_and_the_sibling_moves_back_as_the_content_leaves() {
    let mut screen = screen(
        Axis::Vertical,
        LayoutDirection::Ltr,
        true,
        visibility(Axis::Vertical, expand_vertically(), shrink_vertically()),
    );
    let first = screen.frame();
    assert_eq!(
        first.panel.height, CONTENT.height,
        "content that is visible at first shows at once"
    );

    let frames = screen.run(false, 40);
    let heights = panel_heights(&frames);
    assert!(falls(&heights), "the height never rises: {heights:?}");
    assert!(
        passes(&heights, 0.0, CONTENT.height),
        "the height passes between the content's and nothing: {heights:?}"
    );
    for frame in &frames {
        assert_eq!(frame.next.y, NEIGHBOUR.height + frame.panel.height);
    }
    let last = frames.last().expect("frames");
    assert_eq!(last.panel.height, 0.0);
    assert!(last.content.is_none(), "the content left the composition");
}

#[test]
fn the_alignment_sets_which_part_of_the_content_shows_first() {
    let mut top = screen(
        Axis::Vertical,
        LayoutDirection::Ltr,
        false,
        visibility(
            Axis::Vertical,
            expand_vertically_with(VerticalAlignment::Top, |_| 0.0),
            shrink_vertically(),
        ),
    );
    let frames = top.run(true, 8);
    let frame = frames.last().expect("frames");
    let content = frame.content.expect("content");
    assert!(frame.panel.height > 0.0 && frame.panel.height < CONTENT.height);
    assert_eq!(
        content.y, frame.panel.y,
        "the top of the content shows first"
    );
}

#[test]
fn the_size_functions_set_where_an_expansion_starts_and_where_a_shrink_ends() {
    let half = |full: f32| full / 2.0;
    let mut screen = screen(
        Axis::Vertical,
        LayoutDirection::Ltr,
        false,
        visibility(
            Axis::Vertical,
            expand_vertically_with(VerticalAlignment::Bottom, half),
            shrink_vertically_with(VerticalAlignment::Bottom, half),
        ),
    );
    let entering = screen.run(true, 30);
    let started: Vec<f32> = panel_heights(&entering)
        .into_iter()
        .filter(|&height| height > 0.0)
        .collect();
    assert_eq!(
        started.first(),
        Some(&(CONTENT.height / 2.0)),
        "{started:?}"
    );
    assert!(rises(&started), "{started:?}");

    let leaving = screen.run(false, 40);
    let shown: Vec<f32> = leaving
        .iter()
        .filter(|frame| frame.content.is_some())
        .map(|frame| frame.panel.height)
        .collect();
    assert!(falls(&shown), "{shown:?}");
    assert!(
        shown.iter().all(|&height| height >= CONTENT.height / 2.0),
        "the content ends at half its height: {shown:?}"
    );
    assert_eq!(screen.rest().panel.height, 0.0, "then it leaves");
}

#[test]
fn a_horizontal_expansion_moves_the_sibling_beside_it_and_aligns_to_the_end() {
    let horizontal = || {
        visibility(
            Axis::Horizontal,
            expand_horizontally(),
            shrink_horizontally(),
        )
    };
    let mut screen = screen(Axis::Horizontal, LayoutDirection::Ltr, false, horizontal());
    assert_eq!(screen.frame().panel.width, 0.0);

    let frames = screen.run(true, 40);
    let widths = panel_widths(&frames);
    assert!(rises(&widths), "{widths:?}");
    assert!(passes(&widths, 0.0, CONTENT.width), "{widths:?}");
    for frame in &frames {
        assert_eq!(frame.next.x, NEIGHBOUR.width + frame.panel.width);
        assert_eq!(
            frame.panel.height, CONTENT.height,
            "the height does not change"
        );
        let content = frame.content.expect("content");
        assert_eq!(
            content.x + content.width,
            frame.panel.x + frame.panel.width,
            "the end of the content stays on the end of the panel"
        );
    }
    assert_eq!(screen.frame().panel.width, CONTENT.width);
}

#[test]
fn the_end_is_the_left_in_a_right_to_left_layout() {
    let mut screen = screen(
        Axis::Horizontal,
        LayoutDirection::Rtl,
        false,
        visibility(
            Axis::Horizontal,
            expand_horizontally(),
            shrink_horizontally(),
        ),
    );
    let frames = screen.run(true, 8);
    let frame = frames.last().expect("frames");
    let content = frame.content.expect("content");
    assert!(frame.panel.width > 0.0 && frame.panel.width < CONTENT.width);
    assert_eq!(
        content.x, frame.panel.x,
        "the left of the content stays on the left"
    );
}

#[test]
fn a_change_while_the_content_expands_turns_it_around_from_the_height_it_reached() {
    let mut screen = screen(
        Axis::Vertical,
        LayoutDirection::Ltr,
        false,
        visibility(Axis::Vertical, expand_vertically(), shrink_vertically()),
    );
    let reached = screen.run(true, 8).last().expect("frames").panel.height;
    assert!(reached > 0.0 && reached < CONTENT.height, "{reached}");

    // The change waits for the content it brings to compose, so the
    // expansion runs on for a frame or two before it turns.
    let heights = panel_heights(&screen.run(false, 40));
    assert!(
        heights
            .windows(2)
            .all(|pair| (pair[1] - pair[0]).abs() <= 12.0),
        "the height moves on from {reached} without a jump: {heights:?}"
    );
    assert!(
        heights.iter().all(|&height| height < CONTENT.height),
        "it never reaches the full height again: {heights:?}"
    );
    assert_eq!(heights.last(), Some(&0.0));
}

#[test]
fn a_size_transition_clips_only_while_it_runs_and_only_when_asked() {
    let clipped = |layers: &[GraphicsLayer]| layers.iter().any(|layer| layer.clip);
    let mut screen = screen(
        Axis::Vertical,
        LayoutDirection::Ltr,
        false,
        visibility(Axis::Vertical, expand_vertically(), shrink_vertically()),
    );
    assert!(!clipped(&screen.layers()), "nothing runs");
    screen.run(true, 6);
    assert!(clipped(&screen.layers()), "the expansion clips");
    screen.rest();
    assert!(
        !clipped(&screen.layers()),
        "the content at rest does not clip"
    );

    let mut open = self::screen(
        Axis::Vertical,
        LayoutDirection::Ltr,
        false,
        visibility(
            Axis::Vertical,
            expand_vertically().with_clip(false),
            shrink_vertically(),
        ),
    );
    open.run(true, 6);
    assert!(!clipped(&open.layers()), "the clip is off");
}

#[test]
fn a_slide_moves_the_content_inside_the_size_that_clips_it() {
    let mut screen = screen(
        Axis::Vertical,
        LayoutDirection::Ltr,
        false,
        visibility(
            Axis::Vertical,
            expand_vertically_with(VerticalAlignment::Top, |_| 0.0) + slide_in_vertically(-1.0),
            shrink_vertically(),
        ),
    );
    let frames = screen.run(true, 6);
    let frame = frames.last().expect("frames");
    let content = frame.content.expect("content");
    assert!(
        content.y < frame.panel.y,
        "the content starts above the panel's top: {content:?} in {:?}",
        frame.panel
    );
    assert!(
        screen
            .layers()
            .iter()
            .all(|layer| layer.translation_y == 0.0),
        "no layer carries the slide, the clip would move with it"
    );
    let rest = screen.rest();
    assert_eq!(rest.content.expect("content").y, rest.panel.y);
}

// The content of the animating box: `CONTENT` when `large`, else half of it.
fn sized(large: bool) -> Size {
    if large {
        CONTENT
    } else {
        Size::new(CONTENT.width / 2.0, CONTENT.height / 2.0)
    }
}

fn animated(large: bool) {
    Box(
        Modifier::empty().animate_content_size().test_tag("panel"),
        BoxSpec::default(),
        move || tagged("content", sized(large)),
    );
}

#[test]
fn a_content_change_animates_the_size_and_the_sibling_below_follows() {
    let mut screen = screen(Axis::Vertical, LayoutDirection::Ltr, false, animated);
    let first = screen.frame();
    assert_eq!(
        (first.panel.width, first.panel.height),
        (sized(false).width, sized(false).height),
        "the first size shows without an animation"
    );

    let frames = screen.run(true, 80);
    let heights = panel_heights(&frames);
    let widths = panel_widths(&frames);
    assert!(rises(&heights) && rises(&widths), "{heights:?} {widths:?}");
    assert!(
        passes(&heights, sized(false).height, CONTENT.height),
        "{heights:?}"
    );
    assert!(
        passes(&widths, sized(false).width, CONTENT.width),
        "{widths:?}"
    );
    for frame in &frames {
        assert_eq!(frame.next.y, NEIGHBOUR.height + frame.panel.height);
    }
    let rest = screen.rest();
    assert_eq!(
        (rest.panel.width, rest.panel.height),
        (CONTENT.width, CONTENT.height)
    );

    let back = panel_heights(&screen.run(false, 80));
    assert!(
        falls(&back) && passes(&back, sized(false).height, CONTENT.height),
        "{back:?}"
    );
    assert_eq!(screen.rest().panel.height, sized(false).height);
}

#[test]
fn a_content_change_while_the_size_moves_retargets_from_the_size_that_shows() {
    let mut screen = screen(Axis::Vertical, LayoutDirection::Ltr, false, animated);
    let reached = screen.run(true, 6).last().expect("frames").panel.height;
    assert!(
        reached > sized(false).height && reached < CONTENT.height,
        "{reached}"
    );

    let heights = panel_heights(&screen.run(false, 80));
    assert!(
        heights[0] > sized(false).height && heights[0] <= reached + 4.0,
        "it turns from {reached} without a jump: {heights:?}"
    );
    assert_eq!(screen.rest().panel.height, sized(false).height);
}
