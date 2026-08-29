//! A fixed glass chrome bar over a scrolling list — the topology of the
//! desktop demo's receipts tab (`apps/desktop-demo/src/app/glass_feed.rs`).
//! The chrome's backdrop samples the moving list, so every pixel the list
//! scrolls must reach the glass: a scroll that holds the same blurred
//! capture across a span of pixels before snapping forward would be a
//! continuity bug, not a caching optimization.
//!
//! Renders through the production `GlassSurface`/`Glass::regular()` material
//! (a blur-then-shader effect chain), not a bare blur, so this pins the
//! compositor's own key/hash math (`backdrop_effect_cache_key`,
//! `backdrop_scene_prefix_hash`) as innocent for the real widget. This test
//! passes, but that is not evidence the compositor is innocent overall: it
//! drives `WgpuRenderer::init_gpu`'s offscreen synchronous `capture_frame`
//! path, which renders into a brand-new offscreen texture every call and so
//! never exercises a target reused across frames — the one thing the real
//! windowed present path (`redraw_native_window`, a rotating swapchain pool)
//! does on every frame. The reported continuity bug is reproduced instead by
//! `apps/desktop-demo/robot-runners/robot_glass_backdrop_scroll_stability.rs`,
//! a non-headless robot test asserting each scroll step's backdrop shift
//! individually — see `TIME_WASTERS.md` for why a headless/offscreen render
//! path structurally cannot catch this class of bug.

mod support;

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_liquid::{Glass, GlassSurface, LiquidTheme, LiquidThemeSpec};
use cranpose_ui::{
    Brush, Color, CornerRadii, LinearArrangement, Modifier, Point, composable,
    widgets::{Box, BoxSpec, LazyColumn, LazyColumnSpec},
};

const FRAME_WIDTH: u32 = 320;
const FRAME_HEIGHT: u32 = 480;
const ROW_HEIGHT: f32 = 76.0;
const ROW_GAP: f32 = 12.0;
const CHROME_HEIGHT: f32 = 64.0;
const ROW_COUNT: usize = 30;

/// Diagonal two-color gradients, one per row — vivid and continuously
/// varying along both axes, the kind of backdrop a fixed glass bar has to
/// re-blur at every scrolled pixel rather than a flat fill a shift within
/// could leave byte-identical.
const ROW_GRADIENTS: [[Color; 2]; 3] = [
    [Color(1.0, 0.37, 0.38, 1.0), Color(1.0, 0.69, 0.48, 1.0)],
    [Color(0.23, 0.48, 0.84, 1.0), Color(0.0, 0.82, 1.0, 1.0)],
    [Color(0.56, 0.18, 0.89, 1.0), Color(0.29, 0.0, 0.88, 1.0)],
];

#[composable]
#[allow(non_snake_case)]
fn GradientRow(index: usize) {
    let gradient = ROW_GRADIENTS[index % ROW_GRADIENTS.len()];
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(ROW_HEIGHT)
            .draw_behind(move |scope| {
                let size = scope.size();
                scope.draw_round_rect(
                    Brush::linear_gradient_range(
                        vec![gradient[0], gradient[1]],
                        Point::new(0.0, 0.0),
                        Point::new(size.width, size.height),
                    ),
                    CornerRadii::uniform(14.0),
                );
            }),
        BoxSpec::new(),
        || {},
    );
}

#[composable]
#[allow(non_snake_case)]
fn ReceiptsLikeScene(list_state: LazyListState) {
    LiquidTheme(LiquidThemeSpec::default(), || {
        Box(
            Modifier::empty()
                .size_points(FRAME_WIDTH as f32, FRAME_HEIGHT as f32)
                .background(Color(0.05, 0.05, 0.07, 1.0)),
            BoxSpec::new(),
            move || {
                LazyColumn(
                    Modifier::empty().fill_max_size(),
                    list_state,
                    LazyColumnSpec::new()
                        .vertical_arrangement(LinearArrangement::SpacedBy(ROW_GAP))
                        .content_padding(CHROME_HEIGHT, 24.0),
                    move |scope| {
                        scope.items(
                            LazyItems::new(ROW_COUNT).key(|i: usize| i as u64),
                            GradientRow,
                        );
                    },
                );
                GlassSurface(
                    Modifier::empty().fill_max_width().height(CHROME_HEIGHT),
                    Glass::regular(),
                    || {},
                );
            },
        );
    });
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
            ReceiptsLikeScene(state);
        });
        shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
        shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
        shell.update();
        Self { shell, list_state }
    }

    /// Scrolls by `delta`, recomposes and renders, and returns the captured
    /// frame.
    fn scroll_and_capture(&mut self, delta: f32) -> cranpose_render_wgpu::CapturedFrame {
        let state = self
            .list_state
            .borrow()
            .as_ref()
            .cloned()
            .expect("list state captured");
        self.shell
            .debug_enter_app_context(|| state.dispatch_scroll_delta(delta));
        self.shell.update();
        self.shell
            .renderer()
            .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
            .expect("frame capture should succeed")
    }
}

/// A rectangular patch of RGBA bytes cut out of a captured frame, so two
/// frames can be compared over just the glass chrome's footprint instead of
/// the whole buffer.
fn extract_patch(
    frame: &cranpose_render_wgpu::CapturedFrame,
    (x, y, width, height): (u32, u32, u32, u32),
) -> Vec<u8> {
    let mut patch = Vec::with_capacity((width * height * 4) as usize);
    for row in y..y + height {
        let row_start = ((row * frame.width + x) * 4) as usize;
        let row_end = row_start + (width * 4) as usize;
        patch.extend_from_slice(&frame.pixels[row_start..row_end]);
    }
    patch
}

/// The pin: once the chrome bar's sampled patch already shows real,
/// continuously-varying row content (reached by scrolling well past the top
/// padding first — sampling a still-empty patch would trivially "hold"
/// because there is nothing yet to change, which is not the invariant under
/// test), every further single-pixel scroll step must present freshly
/// blurred content. Not "it changes eventually" — a cache that misses only
/// every third pixel would still pass a coarser "did it ever move" check
/// while presenting two stale frames out of three, which is the reported
/// bug (glass content holds still for a run of scrolled pixels, then
/// snaps).
#[test]
fn a_fixed_glass_chrome_repaints_every_single_pixel_the_list_scrolls() {
    let (_lock, renderer) = match support::headless_renderer_parts() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            return;
        }
    };
    let mut harness = Harness::new(renderer);

    // Settle deep into the list first: the sampled patch must already show
    // real, continuously-varying row content before the measured sweep
    // starts.
    harness.scroll_and_capture(-300.0);

    // Well inside the chrome bar's (0, 0)..(320, 64) footprint, clear of its
    // rounded corners, sampling only glass over the moving list.
    let sample_rect = (40u32, 15u32, 240u32, 30u32);

    let first = harness.scroll_and_capture(-1.0);
    let mut previous = extract_patch(&first, sample_rect);
    for step in 1..40 {
        let frame = harness.scroll_and_capture(-1.0);
        let patch = extract_patch(&frame, sample_rect);
        assert_ne!(
            previous, patch,
            "scroll step {step}: the glass chrome's pixels held over from the \
             previous one-pixel scroll step instead of re-blurring the list \
             that moved underneath it"
        );
        previous = patch;
    }
}
