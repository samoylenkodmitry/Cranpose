mod support;

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key};
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui::{
    Color, LinearArrangement, Modifier, RenderEffect, TextStyle, composable,
    widgets::{Box, BoxSpec, Column, ColumnSpec, LazyColumn, LazyColumnSpec, Text},
};

const FRAME_WIDTH: u32 = 640;
const FRAME_HEIGHT: u32 = 640;
const PAGE_PADDING: f32 = 12.0;
const ROW_HEIGHT: f32 = 96.0;
const ROW_SPACING: f32 = 8.0;

fn visible_rows() -> u32 {
    ((FRAME_HEIGHT as f32 - 2.0 * PAGE_PADDING + ROW_SPACING) / (ROW_HEIGHT + ROW_SPACING)).ceil()
        as u32
}

#[composable]
#[allow(non_snake_case)]
fn GlassRow(index: usize, first_row_warm: MutableState<bool>) {
    let tint = match index % 4 {
        0 if first_row_warm.get() && index == 0 => Color(0.92, 0.36, 0.12, 0.55),
        0 => Color(0.20, 0.26, 0.36, 0.55),
        1 => Color(0.28, 0.20, 0.34, 0.55),
        2 => Color(0.18, 0.32, 0.30, 0.55),
        _ => Color(0.32, 0.26, 0.20, 0.55),
    };
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(ROW_HEIGHT)
            .backdrop_effect(RenderEffect::blur(12.0))
            .background(tint)
            .rounded_corners(18.0)
            .padding(14.0),
        BoxSpec::new(),
        move || {
            Text(
                format!("Glass row {index}"),
                Modifier::empty(),
                TextStyle::default(),
            );
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn GlassOverlay(pulse: MutableState<f32>, drift: MutableState<f32>) {
    let pulse_value = pulse.get();
    let drift_value = drift.get();
    Box(
        Modifier::empty()
            .offset(48.0 + drift_value * 180.0, 72.0 + drift_value * 96.0)
            .width(300.0)
            .height(164.0)
            .backdrop_effect(RenderEffect::blur(10.0 + pulse_value * 14.0))
            .background(Color(0.80, 0.88, 0.98, 0.20 + pulse_value * 0.12))
            .rounded_corners(22.0)
            .padding(18.0),
        BoxSpec::new(),
        move || {
            Column(
                Modifier::empty(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                move || {
                    Text(
                        "Animated glass".to_string(),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                },
            );
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn GlassScene(
    list_state: LazyListState,
    pulse: MutableState<f32>,
    drift: MutableState<f32>,
    first_row_warm: MutableState<bool>,
) {
    Box(
        Modifier::empty()
            .size_points(FRAME_WIDTH as f32, FRAME_HEIGHT as f32)
            .background(Color(0.03, 0.04, 0.06, 1.0))
            .rounded_corners(18.0),
        BoxSpec::new(),
        move || {
            Box(
                Modifier::empty().fill_max_size().padding(PAGE_PADDING),
                BoxSpec::new(),
                move || {
                    LazyColumn(
                        Modifier::empty().fill_max_size(),
                        list_state,
                        LazyColumnSpec::new()
                            .vertical_arrangement(LinearArrangement::SpacedBy(ROW_SPACING)),
                        move |scope| {
                            scope
                                .items(LazyItems::new(40).key(|i: usize| i as u64), move |index| {
                                    GlassRow(index, first_row_warm)
                                });
                        },
                    );
                    GlassOverlay(pulse, drift);
                },
            );
        },
    );
}

#[derive(Clone, Copy, Default)]
struct SceneInput {
    scroll_delta: f32,
    pulse: f32,
    drift: f32,
    first_row_warm: bool,
}

struct GlassHarness {
    shell: AppShell<cranpose_render_wgpu::WgpuRenderer>,
    list_state: Rc<RefCell<Option<LazyListState>>>,
    pulse: Rc<RefCell<Option<MutableState<f32>>>>,
    drift: Rc<RefCell<Option<MutableState<f32>>>>,
    first_row_warm: Rc<RefCell<Option<MutableState<bool>>>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct FrameCacheStats {
    hits: u32,
    misses: u32,
    blur_passes: u32,
    isolated_layer_renders: u32,
}

fn captured<T: Clone>(slot: &Rc<RefCell<Option<T>>>) -> T {
    slot.borrow().as_ref().cloned().expect("state captured")
}

impl GlassHarness {
    fn new(renderer: cranpose_render_wgpu::WgpuRenderer) -> Self {
        let root_key = location_key(file!(), line!(), column!());
        let list_state: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
        let pulse: Rc<RefCell<Option<MutableState<f32>>>> = Rc::new(RefCell::new(None));
        let drift: Rc<RefCell<Option<MutableState<f32>>>> = Rc::new(RefCell::new(None));
        let first_row_warm: Rc<RefCell<Option<MutableState<bool>>>> = Rc::new(RefCell::new(None));
        let list_state_for_app = Rc::clone(&list_state);
        let pulse_for_app = Rc::clone(&pulse);
        let drift_for_app = Rc::clone(&drift);
        let warm_for_app = Rc::clone(&first_row_warm);
        let mut shell = AppShell::new(renderer, root_key, move || {
            let state = rememberLazyListState();
            let pulse_state = cranpose_core::rememberMutableStateOf(|| 0.0f32);
            let drift_state = cranpose_core::rememberMutableStateOf(|| 0.0f32);
            let warm_state = cranpose_core::rememberMutableStateOf(|| false);
            *list_state_for_app.borrow_mut() = Some(state);
            *pulse_for_app.borrow_mut() = Some(pulse_state);
            *drift_for_app.borrow_mut() = Some(drift_state);
            *warm_for_app.borrow_mut() = Some(warm_state);
            GlassScene(state, pulse_state, drift_state, warm_state);
        });
        shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
        shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
        shell.update();
        Self {
            shell,
            list_state,
            pulse,
            drift,
            first_row_warm,
        }
    }

    fn frame(&mut self, input: SceneInput) -> (FrameCacheStats, CapturedFrame) {
        if input.scroll_delta != 0.0 {
            let state = captured(&self.list_state);
            self.shell
                .debug_enter_app_context(|| state.dispatch_scroll_delta(input.scroll_delta));
        }
        let pulse_state = captured(&self.pulse);
        let drift_state = captured(&self.drift);
        let warm_state = captured(&self.first_row_warm);
        self.shell.debug_enter_app_context(|| {
            pulse_state.set(input.pulse);
            drift_state.set(input.drift);
            warm_state.set(input.first_row_warm);
        });
        self.shell.update();
        let frame = self
            .shell
            .renderer()
            .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
            .expect("frame capture should succeed");
        let stats = self
            .shell
            .renderer()
            .last_frame_stats()
            .expect("frame stats");
        (
            FrameCacheStats {
                hits: stats.layer_cache_hits,
                misses: stats.layer_cache_misses,
                blur_passes: stats.blur_passes,
                isolated_layer_renders: stats.isolated_layer_renders,
            },
            frame,
        )
    }

    fn stats(&mut self, input: SceneInput) -> FrameCacheStats {
        self.frame(input).0
    }
}

const WARMUP_FRAMES: usize = 4;
const MEASURED_FRAMES: usize = 8;

fn harness() -> Option<(std::sync::MutexGuard<'static, ()>, GlassHarness)> {
    match support::headless_renderer_parts() {
        Ok((lock, renderer)) => Some((lock, GlassHarness::new(renderer))),
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            None
        }
    }
}

#[test]
fn a_still_glass_scene_leaves_no_layer_cache_misses() {
    let Some((_lock, mut harness)) = harness() else {
        return;
    };
    let still = SceneInput::default();
    for _ in 0..WARMUP_FRAMES {
        harness.stats(still);
    }
    for frame_index in 0..MEASURED_FRAMES {
        let stats = harness.stats(still);
        assert_eq!(
            stats.misses, 0,
            "a still glass frame must not miss the layer cache (frame {frame_index}): {stats:?}"
        );
        assert!(
            stats.hits > 0,
            "a still glass frame must serve its layers from the cache (frame {frame_index}): {stats:?}"
        );
        assert_eq!(
            stats.blur_passes, 0,
            "a still glass frame must not re-run any blur (frame {frame_index}): {stats:?}"
        );
        assert_eq!(
            stats.isolated_layer_renders, 0,
            "a still glass frame must not re-render any isolated layer (frame {frame_index}): {stats:?}"
        );
    }
}

#[test]
fn an_animated_overlay_leaves_still_glass_rows_fully_cached() {
    let Some((_lock, mut harness)) = harness() else {
        return;
    };
    let animation = |frame_index: usize| {
        let t = (frame_index + 1) as f32 / 16.0;
        SceneInput {
            pulse: t,
            drift: t,
            ..SceneInput::default()
        }
    };
    for frame_index in 0..WARMUP_FRAMES {
        harness.stats(animation(frame_index));
    }
    for frame_index in 0..MEASURED_FRAMES {
        let stats = harness.stats(animation(WARMUP_FRAMES + frame_index));
        assert_eq!(
            stats.misses, 1,
            "an animated overlay re-renders exactly its own backdrop blur, its content \
             draws straight into the page; anything more means a still row lost its \
             cache (frame {frame_index}): {stats:?}"
        );
        assert_eq!(
            stats.hits,
            visible_rows(),
            "every still row's glass result must keep hitting while the overlay \
             animates (frame {frame_index}): {stats:?}"
        );
        assert_eq!(
            stats.blur_passes, 1,
            "only the overlay's backdrop re-blurs while the rows hold still \
             (frame {frame_index}): {stats:?}"
        );
    }
}

const RIGID_SCROLL_STEP: f32 = 8.0;

#[test]
fn a_rigid_scroll_reuses_every_glass_result_whose_input_moved_with_it() {
    let Some((_lock, mut harness)) = harness() else {
        return;
    };
    let scrolling = SceneInput {
        scroll_delta: RIGID_SCROLL_STEP,
        ..SceneInput::default()
    };
    for _ in 0..WARMUP_FRAMES {
        harness.stats(scrolling);
    }
    for frame_index in 0..MEASURED_FRAMES {
        let stats = harness.stats(scrolling);
        assert!(
            stats.hits + 1 >= visible_rows(),
            "rows scrolling rigidly over a flat background read the same pixels every \
             frame and must reuse their glass results (frame {frame_index}): {stats:?}"
        );
        assert!(
            stats.blur_passes <= 2,
            "only the overlay, whose backdrop scrolls under it, and at most one row \
             entering the viewport may blur per scrolled frame (frame {frame_index}): {stats:?}"
        );
    }
}

fn overlay_interior_pixels(frame: &CapturedFrame) -> Vec<u8> {
    let (left, top, right, bottom) = (80usize, 100usize, 320usize, 220usize);
    let mut pixels = Vec::with_capacity((right - left) * (bottom - top) * 4);
    for y in top..bottom {
        let row = y * frame.width as usize * 4;
        pixels.extend_from_slice(&frame.pixels[row + left * 4..row + right * 4]);
    }
    pixels
}

fn max_channel_delta(a: &[u8], b: &[u8]) -> u8 {
    a.iter()
        .zip(b)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0)
}

#[test]
fn a_cached_glass_result_follows_a_change_beneath_it() {
    let Some((_lock, mut harness)) = harness() else {
        return;
    };
    let cool = SceneInput::default();
    let warm = SceneInput {
        first_row_warm: true,
        ..SceneInput::default()
    };
    for _ in 0..WARMUP_FRAMES {
        harness.stats(cool);
    }
    let (cached, before) = harness.frame(cool);
    assert_eq!(
        cached.misses, 0,
        "the overlay must be served from the cache: {cached:?}"
    );
    let (_, after) = harness.frame(warm);
    let (settled, settled_frame) = harness.frame(warm);
    let before = overlay_interior_pixels(&before);
    let after = overlay_interior_pixels(&after);
    assert!(
        max_channel_delta(&before, &after) > 24,
        "the overlay reads the first row; a warmer row must show through the glass"
    );
    assert_eq!(
        max_channel_delta(&after, &overlay_interior_pixels(&settled_frame)),
        0,
        "the frame after the change and the settled frame must agree: {settled:?}"
    );

    let mut fresh = GlassHarness::new(
        support::headless_renderer_beside_locked().expect("second headless renderer"),
    );
    for _ in 0..WARMUP_FRAMES {
        fresh.stats(warm);
    }
    let (_, reference) = fresh.frame(warm);
    assert!(
        max_channel_delta(&after, &overlay_interior_pixels(&reference)) <= 1,
        "a glass result reused from the cache must match a renderer that never cached"
    );
}
