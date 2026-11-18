// `Modifier::anchored_draggable` through the public shell: a slow release
// settles at the closest anchor, a fast one goes on to the next anchor in
// its direction, the offset stays within the anchors, a vertical drag leaves
// a horizontal state at rest, a press that stops a settle settles again
// when it lifts, and `animate_to` and `snap_to` land on an anchor.

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{location_key, remember};
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{AnchoredDraggableState, Box, BoxSpec, DraggableAnchors, Modifier, Size};
use cranpose_ui_layout::Axis;

use crate::test_window::first_frame;

const WINDOW: Size = Size {
    width: 400.0,
    height: 200.0,
};
const FRAME: u64 = 16_000_000;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Side {
    Closed,
    TrailingOpen,
    LeadingOpen,
}

// A 300 by 60 row whose state rests closed at 0, opens its trailing side at
// -150 and its leading side at 80.
struct Row {
    shell: AppShell<TestRenderer>,
    state: AnchoredDraggableState<Side>,
    frame: u64,
}

impl Row {
    fn new() -> Self {
        let slot: Rc<RefCell<Option<AnchoredDraggableState<Side>>>> = Rc::default();
        let captured = Rc::clone(&slot);
        let mut shell = AppShell::new(
            TestRenderer::default(),
            location_key(file!(), line!(), column!()),
            move || {
                let state = remember(|| {
                    AnchoredDraggableState::new(
                        Side::Closed,
                        DraggableAnchors::new([
                            (Side::TrailingOpen, -150.0),
                            (Side::Closed, 0.0),
                            (Side::LeadingOpen, 80.0),
                        ]),
                    )
                })
                .with(Clone::clone);
                *captured.borrow_mut() = Some(state.clone());
                Box(
                    Modifier::empty()
                        .size_points(300.0, 60.0)
                        .anchored_draggable(Axis::Horizontal, state),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
        let frame = first_frame(&mut shell, WINDOW);
        let state = slot.borrow().clone().expect("the row composed");
        Self {
            shell,
            state,
            frame,
        }
    }

    fn millis(&self) -> Option<i64> {
        Some(i64::try_from(self.frame / 1_000_000).expect("the test clock fits in i64"))
    }

    fn advance(&mut self, frames: u32) {
        for _ in 0..frames {
            self.frame += FRAME;
            self.shell.update_at_frame_time_nanos(self.frame);
        }
    }

    // A press at (`x0`, `y0`), moves in `steps` equal frames to (`x1`,
    // `y1`), `hold` still frames there and a release.
    fn drag(&mut self, (x0, y0): (f32, f32), (x1, y1): (f32, f32), steps: u32, hold: u32) {
        self.shell.set_cursor_at_time(x0, y0, self.millis());
        self.shell.pointer_pressed_at_time(self.millis());
        for step in 1..=steps {
            self.advance(1);
            let t = step as f32 / steps as f32;
            self.shell
                .set_cursor_at_time(x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, self.millis());
        }
        for _ in 0..hold {
            self.advance(1);
            self.shell.set_cursor_at_time(x1, y1, self.millis());
        }
        self.shell
            .pointer_released_at_position_time(x1, y1, self.millis());
        self.advance(90);
    }
}

#[test]
fn a_slow_release_settles_at_the_closest_anchor() {
    let mut row = Row::new();
    row.drag((280.0, 30.0), (180.0, 30.0), 10, 12);
    assert_eq!(
        row.state.current_value(),
        Side::TrailingOpen,
        "past half way it opens"
    );
    assert_eq!(row.state.offset(), Some(-150.0));

    let mut row = Row::new();
    row.drag((280.0, 30.0), (230.0, 30.0), 10, 12);
    assert_eq!(
        row.state.current_value(),
        Side::Closed,
        "short of half way it closes"
    );
    assert_eq!(row.state.offset(), Some(0.0));
}

#[test]
fn a_fast_release_goes_on_to_the_next_anchor() {
    let mut row = Row::new();
    row.drag((280.0, 30.0), (240.0, 30.0), 3, 0);
    assert!(
        row.state.last_velocity() < -125.0,
        "a flick: {}",
        row.state.last_velocity()
    );
    assert_eq!(row.state.settled_value(), Side::TrailingOpen);

    row.drag((40.0, 30.0), (80.0, 30.0), 3, 0);
    assert_eq!(
        row.state.settled_value(),
        Side::Closed,
        "a flick back closes it"
    );
}

#[test]
fn the_offset_stays_within_the_anchors() {
    let mut row = Row::new();
    row.shell.set_cursor_at_time(290.0, 30.0, row.millis());
    row.shell.pointer_pressed_at_time(row.millis());
    for x in [250.0, 150.0, 50.0, 10.0] {
        row.advance(1);
        row.shell.set_cursor_at_time(x, 30.0, row.millis());
        row.advance(1);
        assert!(
            row.state.require_offset() >= -150.0,
            "{}",
            row.state.require_offset()
        );
    }
    assert_eq!(
        row.state.offset(),
        Some(-150.0),
        "the drag stops at the last anchor"
    );
    assert_eq!(row.state.target_value(), Side::TrailingOpen);
    row.shell
        .pointer_released_at_position_time(10.0, 30.0, row.millis());
    row.advance(60);
    assert_eq!(row.state.settled_value(), Side::TrailingOpen);
}

#[test]
fn a_vertical_drag_leaves_a_horizontal_state_at_rest() {
    let mut row = Row::new();
    row.drag((150.0, 5.0), (140.0, 190.0), 8, 4);
    assert_eq!(row.state.offset(), Some(0.0));
    assert_eq!(row.state.current_value(), Side::Closed);
}

#[test]
fn animate_to_and_snap_to_land_on_an_anchor() {
    let mut row = Row::new();
    row.state.animate_to(Side::LeadingOpen);
    row.advance(1);
    assert!(row.state.is_animation_running());
    assert_eq!(row.state.target_value(), Side::LeadingOpen);
    row.advance(90);
    assert!(!row.state.is_animation_running());
    assert_eq!(row.state.offset(), Some(80.0));
    assert_eq!(row.state.settled_value(), Side::LeadingOpen);
    assert_eq!(row.state.progress(Side::Closed, Side::LeadingOpen), 1.0);

    row.state.snap_to(Side::TrailingOpen);
    assert_eq!(row.state.offset(), Some(-150.0));
    assert_eq!(row.state.current_value(), Side::TrailingOpen);
}

#[test]
fn a_press_that_stops_a_settle_settles_again_when_it_lifts() {
    let mut row = Row::new();
    row.state.animate_to(Side::TrailingOpen);
    row.advance(3);
    let stopped = row.state.require_offset();
    assert!(
        stopped < -1.0 && stopped > -149.0,
        "the spring is under way: {stopped}"
    );
    row.drag((200.0, 30.0), (200.0, 30.0), 1, 0);
    let rest = row.state.require_offset();
    let on_anchor = [0.0_f32, -150.0]
        .iter()
        .any(|anchor| (rest - anchor).abs() < 0.01);
    assert!(on_anchor, "it rests on an anchor: {rest}");
    assert!(!row.state.is_animation_running());
}
