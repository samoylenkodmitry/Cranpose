//! The loupe a dragged text handle raises is never drawn outside its window:
//! it slides back from a side, drops below the line when the window has no
//! room above it, and shrinks when the window is smaller than it. In every
//! case its lens keeps magnifying the grabbed line at the finger. A window
//! with room shows it where it always was.

use cranpose_core::remember;
use cranpose_testing::{RobotTestRule, TestRenderer, create_headless_robot_test};
use cranpose_ui::{
    BasicTextFieldOptions, BasicTextFieldWithOptions, LayoutBox, Modifier, RenderEffect,
    TextFieldState, TextStyle,
    text::{AnnotatedString, measure_text},
    widgets::loupe::{LOUPE_HEIGHT, LOUPE_RISE, LOUPE_WIDTH},
};
use cranpose_ui_graphics::{Point, Rect};

/// Shader slots of the loupe material: the mode flag and the magnified point,
/// in dp from the bubble's center.
const LOUPE_MODE_UNIFORM: usize = 80;
const LOUPE_FOCUS_X_UNIFORM: usize = 81;
const LOUPE_FOCUS_Y_UNIFORM: usize = 82;

/// Layout is placed on whole pixels, so a drawn edge can differ by one pixel.
const PIXEL: f32 = 1.0;

const FIELD_X: f32 = 10.0;
const FIELD_HEIGHT: f32 = 32.0;
/// How far below the line's bottom the finger grabs the cursor handle: on its
/// dot.
const GRAB_BELOW_LINE: f32 = 10.0;

/// The loupe as drawn: its bounds in the window and the point its lens
/// magnifies.
struct Loupe {
    bounds: Rect,
    focus: Point,
}

/// One text field in a window, the cursor handle shown, the finger down on it.
struct Drag {
    robot: RobotTestRule<TestRenderer>,
    window: (f32, f32),
    line_bottom: f32,
    line_height: f32,
}

impl Drag {
    fn begin(window: (f32, f32), field_width: f32, field_y: f32) -> Self {
        let mut robot = create_headless_robot_test(window.0 as u32, window.1 as u32, move || {
            let state = remember(|| TextFieldState::new("hello world")).with(|state| *state);
            BasicTextFieldWithOptions(
                state,
                Modifier::empty()
                    .offset(FIELD_X, field_y)
                    .width(field_width)
                    .height(FIELD_HEIGHT),
                BasicTextFieldOptions::default(),
            );
        });
        robot.wait_for_idle();
        let line_height = robot.shell_mut().app_context().enter(|| {
            measure_text(&AnnotatedString::from("hello world"), &TextStyle::default()).height
        });
        // A tap focuses the field and puts the caret, with its handle, at the
        // start of the text.
        robot.mouse_move(FIELD_X + 1.0, field_y + 0.5 * line_height);
        robot.mouse_down();
        robot.mouse_up();
        robot.wait_for_idle();
        let line_bottom = field_y + line_height;
        robot.mouse_move(FIELD_X, line_bottom + GRAB_BELOW_LINE);
        robot.mouse_down();
        Self {
            robot,
            window,
            line_bottom,
            line_height,
        }
    }

    fn line_mid(&self) -> f32 {
        self.line_bottom - 0.5 * self.line_height
    }

    /// Drags the finger to `x` along the line below the text, lets the loupe
    /// finish growing and returns it.
    fn to(&mut self, x: f32) -> Loupe {
        let y = self.line_bottom + GRAB_BELOW_LINE;
        for step in 1..=6 {
            let t = step as f32 / 6.0;
            self.robot.mouse_move(FIELD_X + (x - FIELD_X) * t, y);
            self.robot.advance_time(16_666_667);
        }
        for _ in 0..120 {
            self.robot.advance_time(16_666_667);
        }
        self.loupe()
    }

    fn loupe(&mut self) -> Loupe {
        let found = self.robot.shell_mut().with_layout_tree(|tree| {
            fn find(node: &LayoutBox) -> Option<Loupe> {
                let layer = node.node_data.modifier_slices().graphics_layer();
                if let Some(RenderEffect::Shader { shader }) = layer
                    .as_ref()
                    .and_then(|layer| layer.backdrop_effect.as_ref())
                {
                    let uniforms = shader.uniforms();
                    if uniforms[LOUPE_MODE_UNIFORM] > 0.5 {
                        let bounds = node.rect;
                        return Some(Loupe {
                            bounds,
                            focus: Point::new(
                                bounds.x + 0.5 * bounds.width + uniforms[LOUPE_FOCUS_X_UNIFORM],
                                bounds.y + 0.5 * bounds.height + uniforms[LOUPE_FOCUS_Y_UNIFORM],
                            ),
                        });
                    }
                }
                node.children.iter().find_map(find)
            }
            tree.and_then(|tree| find(tree.root()))
        });
        found.expect("a dragged handle raises the loupe")
    }

    fn assert_inside_window(&self, loupe: &Loupe) {
        let (width, height) = self.window;
        let bounds = loupe.bounds;
        assert!(
            bounds.x >= 0.0
                && bounds.y >= 0.0
                && bounds.x + bounds.width <= width
                && bounds.y + bounds.height <= height,
            "the loupe at {bounds:?} leaves the {width} x {height} window"
        );
    }

    /// The lens still magnifies the grabbed line at the finger, wherever the
    /// bubble was put.
    fn assert_magnifies_line_at(&self, loupe: &Loupe, finger_x: f32) {
        let line_mid = self.line_mid();
        assert!(
            (loupe.focus.x - finger_x).abs() <= PIXEL && (loupe.focus.y - line_mid).abs() <= PIXEL,
            "the lens looks at {:?}, not at the line at ({finger_x}, {line_mid})",
            loupe.focus
        );
    }
}

#[test]
fn a_field_near_the_top_of_a_short_window_keeps_the_loupe_inside_it() {
    let mut drag = Drag::begin((320.0, 100.0), 300.0, 8.0);
    let loupe = drag.to(100.0);
    drag.assert_inside_window(&loupe);
    drag.assert_magnifies_line_at(&loupe, 100.0);
}

#[test]
fn without_room_above_the_line_the_loupe_sits_below_it() {
    let mut drag = Drag::begin((320.0, 240.0), 300.0, 8.0);
    let loupe = drag.to(100.0);
    drag.assert_inside_window(&loupe);
    assert!(
        loupe.bounds.y >= drag.line_bottom,
        "the loupe at {:?} still covers the line that ends at {}",
        loupe.bounds,
        drag.line_bottom
    );
    drag.assert_magnifies_line_at(&loupe, 100.0);
}

#[test]
fn near_a_side_of_the_window_the_loupe_slides_along_the_edge() {
    let mut right = Drag::begin((320.0, 400.0), 300.0, 200.0);
    let loupe = right.to(312.0);
    right.assert_inside_window(&loupe);
    assert!(
        (loupe.bounds.x + loupe.bounds.width - 320.0).abs() <= PIXEL,
        "the loupe at {:?} lets go of the right edge",
        loupe.bounds
    );
    right.assert_magnifies_line_at(&loupe, 312.0);

    let mut left = Drag::begin((320.0, 400.0), 300.0, 200.0);
    let loupe = left.to(2.0);
    left.assert_inside_window(&loupe);
    assert!(
        loupe.bounds.x.abs() <= PIXEL,
        "the loupe at {:?} lets go of the left edge",
        loupe.bounds
    );
    left.assert_magnifies_line_at(&loupe, 2.0);
}

#[test]
fn a_window_smaller_than_the_loupe_shrinks_it() {
    let mut drag = Drag::begin((100.0, 60.0), 80.0, 8.0);
    let loupe = drag.to(60.0);
    drag.assert_inside_window(&loupe);
    assert!(
        loupe.bounds.width < LOUPE_WIDTH - PIXEL && loupe.bounds.height < LOUPE_HEIGHT - PIXEL,
        "the loupe at {:?} kept its full size in a window of 100 x 60",
        loupe.bounds
    );
    drag.assert_magnifies_line_at(&loupe, 60.0);
}

#[test]
fn a_window_with_room_shows_the_loupe_where_it_always_was() {
    let mut drag = Drag::begin((320.0, 400.0), 300.0, 200.0);
    let loupe = drag.to(100.0);
    let bounds = loupe.bounds;
    let expected = Rect {
        x: 100.0 - 0.5 * LOUPE_WIDTH,
        y: drag.line_mid() - LOUPE_RISE - 0.5 * LOUPE_HEIGHT,
        width: LOUPE_WIDTH,
        height: LOUPE_HEIGHT,
    };
    assert!(
        (bounds.x - expected.x).abs() <= PIXEL
            && (bounds.y - expected.y).abs() <= PIXEL
            && (bounds.width - expected.width).abs() <= PIXEL
            && (bounds.height - expected.height).abs() <= PIXEL,
        "the loupe moved: {bounds:?} instead of {expected:?}"
    );
    // The lens rises straight over the line, as it did: nothing was
    // clamped, so no offset toward the finger was added.
    assert!(
        (loupe.focus.x - (bounds.x + 0.5 * bounds.width)).abs() < 1.0e-3
            && (loupe.focus.y - (bounds.y + 0.5 * bounds.height) - LOUPE_RISE).abs() < 1.0e-3,
        "the lens focus {:?} is off its place under the bubble {bounds:?}",
        loupe.focus
    );
}
