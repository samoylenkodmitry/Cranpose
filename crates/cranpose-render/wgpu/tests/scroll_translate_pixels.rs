//! A scrolled frame's pixels must move by the consumed delta. The scroll
//! translate fast path patches layer transforms without re-lowering; every
//! consumer of the retained scene — dynamic conversion, retained span
//! replay, cached-layer composites — must honor the moved transforms, or
//! the presented frame silently freezes while layout (and semantics) walk
//! on without it. robot_scroll_decoration_invariance caught exactly that:
//! an underline row anti-correlated with its text's semantic position by
//! precisely the per-step scroll delta, meaning nothing on screen moved.
//!
//! This list of three consumers is NOT known to be exhaustive — read the
//! rest of this note before trusting it the way PR #542's investigation
//! initially did. That investigation measured a real, reproduced,
//! frozen/overshoot/converge scroll-tracking defect (a glass backdrop
//! failing to track scrolled content beneath it) and instrumented all
//! three consumers named above, one at a time, on the exact run that
//! reproduced the bug: cached-layer composites tracked the confirmed
//! scroll position exactly on every step (206 samples, zero deviation);
//! retained span replay never engaged for the failing content at all
//! (structurally absent, not stale); and dynamic conversion also tracked
//! exactly on every step, including the frozen and overshooting ones (see
//! PR #542's comments for the per-step numbers and the code paths
//! instrumented for each). All three came back clean on a run that still
//! failed. Whatever was placing that content, it was not named here. If
//! you are chasing a similar symptom and have ruled out all three
//! consumers above on your own failing run, the bug is not in this list —
//! look at capture/paint timing (does the consumer reading the retained
//! scene's transform run before or after the frame's own content actually
//! lands in the shared render target) before assuming a fourth transform
//! consumer exists to find.

mod support;

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_ui::{
    Color, LinearArrangement, Modifier, TextStyle, composable,
    widgets::{Box, BoxSpec, LazyColumn, LazyColumnSpec, Text},
};

const FRAME_WIDTH: u32 = 320;
const FRAME_HEIGHT: u32 = 480;
const ROW_HEIGHT: f32 = 72.0;
const ROW_GAP: f32 = 10.0;

#[composable]
#[allow(non_snake_case)]
fn ColoredRow(index: usize) {
    let fill = match index % 4 {
        0 => Color(0.85, 0.25, 0.25, 1.0),
        1 => Color(0.20, 0.55, 0.90, 1.0),
        2 => Color(0.25, 0.75, 0.40, 1.0),
        _ => Color(0.90, 0.75, 0.20, 1.0),
    };
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(ROW_HEIGHT)
            .background(fill),
        BoxSpec::new(),
        move || {
            Text(
                format!("row {index}"),
                Modifier::empty(),
                TextStyle::default(),
            );
        },
    );
}

struct Harness {
    shell: AppShell<cranpose_render_wgpu::WgpuRenderer>,
    list_state: Rc<RefCell<Option<LazyListState>>>,
}

impl Harness {
    fn new(renderer: cranpose_render_wgpu::WgpuRenderer) -> Self {
        let root_key = location_key(file!(), line!(), column!());
        let list_state: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
        let list_state_for_app = Rc::clone(&list_state);
        let mut shell = AppShell::new(renderer, root_key, move || {
            let state = rememberLazyListState();
            *list_state_for_app.borrow_mut() = Some(state);
            Box(
                Modifier::empty()
                    .size_points(FRAME_WIDTH as f32, FRAME_HEIGHT as f32)
                    .background(Color(0.05, 0.06, 0.08, 1.0)),
                BoxSpec::new(),
                move || {
                    LazyColumn(
                        Modifier::empty().fill_max_size(),
                        state,
                        LazyColumnSpec::new()
                            .vertical_arrangement(LinearArrangement::SpacedBy(ROW_GAP)),
                        |scope| {
                            scope.items(LazyItems::new(40).key(|i: usize| i as u64), ColoredRow);
                        },
                    );
                },
            );
        });
        shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
        shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
        shell.update();
        Self { shell, list_state }
    }

    fn capture(&mut self) -> cranpose_render_wgpu::CapturedFrame {
        self.shell
            .renderer()
            .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
            .expect("frame capture should succeed")
    }

    fn scroll(&mut self, delta: f32) -> f32 {
        let state = self
            .list_state
            .borrow()
            .as_ref()
            .cloned()
            .expect("list state captured");
        let consumed = self
            .shell
            .debug_enter_app_context(|| state.dispatch_scroll_delta(delta));
        self.shell.update();
        consumed
    }
}

/// First y at which the second row's blue fill appears in the given column.
fn blue_row_top(frame: &cranpose_render_wgpu::CapturedFrame, x: u32) -> Option<u32> {
    (0..frame.height).find(|&y| {
        let offset = ((y * frame.width + x) * 4) as usize;
        let (r, g, b) = (
            frame.pixels[offset],
            frame.pixels[offset + 1],
            frame.pixels[offset + 2],
        );
        // Color(0.20, 0.55, 0.90) in sRGB-ish bytes: strongly blue.
        b > 180 && g > 100 && g < 180 && r < 100
    })
}

#[test]
fn a_scrolled_frame_moves_its_pixels_by_the_consumed_delta() {
    let (_lock, renderer) = support::headless_renderer_parts().expect("headless renderer");
    let mut harness = Harness::new(renderer);

    // Settle: a warm frame so every cache and retained recording exists.
    harness.scroll(-1.0);
    let before = harness.capture();
    let before_top = blue_row_top(&before, FRAME_WIDTH / 2)
        .expect("the second (blue) row must be visible before the scroll");

    let consumed = harness.scroll(-30.0);
    assert!(
        consumed.abs() > 20.0,
        "the scroll must consume most of the delta, consumed={consumed}"
    );
    let after = harness.capture();
    let after_top = blue_row_top(&after, FRAME_WIDTH / 2)
        .expect("the blue row must still be visible after the scroll");

    let moved = before_top as i64 - after_top as i64;
    let expected = consumed.abs().round() as i64;
    assert!(
        (moved - expected).abs() <= 1,
        "the presented pixels must move with the scroll: blue row top \
         {before_top} -> {after_top} (moved {moved}px, consumed {expected}px)"
    );
}
