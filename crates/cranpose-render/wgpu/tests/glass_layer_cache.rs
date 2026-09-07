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
use support::max_channel_delta;

const FRAME_WIDTH: u32 = 640;
const FRAME_HEIGHT: u32 = 640;
const PAGE_PADDING: f32 = 12.0;
const ROW_HEIGHT: f32 = 96.0;
const ROW_SPACING: f32 = 8.0;
const NO_BACKDROP_CACHE: &str = "CRANPOSE_NO_BACKDROP_CACHE";

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
    admissions: u32,
    entries: u32,
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
                admissions: stats.backdrop_admissions,
                entries: stats.layer_cache_size,
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

fn warmup_frames() -> usize {
    2 * (visible_rows() as usize + 1)
}
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
    for _ in 0..warmup_frames() {
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
    for frame_index in 0..warmup_frames() {
        harness.stats(animation(frame_index));
    }
    for frame_index in 0..MEASURED_FRAMES {
        let stats = harness.stats(animation(warmup_frames() + frame_index));
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
    for _ in 0..warmup_frames() {
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
    for _ in 0..warmup_frames() {
        harness.stats(cool);
    }
    let (cached, before) = harness.frame(cool);
    assert_eq!(
        cached.misses, 0,
        "the overlay must be served from the cache: {cached:?}"
    );
    let (_, after) = harness.frame(warm);
    let (settled, settled_frame) = harness.frame(warm);
    assert!(
        max_channel_delta(
            &overlay_interior_pixels(&before),
            &overlay_interior_pixels(&after)
        ) > 24,
        "the overlay reads the first row; a warmer row must show through the glass"
    );
    assert_eq!(
        max_channel_delta(&after.pixels, &settled_frame.pixels),
        0,
        "the frame after the change and the settled frame must agree: {settled:?}"
    );
    let mut follow_ups = Vec::new();
    let mut warm_frames = 0;
    while warm_frames < 2 {
        assert!(
            follow_ups.len() < 6,
            "the changed backdrops must settle into the cache within a few frames"
        );
        let (stats, frame) = harness.frame(warm);
        if stats.misses == 0 {
            assert!(stats.hits > 0, "a warm frame must hit the cache: {stats:?}");
            warm_frames += 1;
        } else {
            warm_frames = 0;
        }
        follow_ups.push(frame);
    }

    let mut fresh = GlassHarness::new(
        support::headless_renderer_beside_locked().expect("second headless renderer"),
    );
    let cool_reference = never_cached_frame(&mut fresh, cool);
    assert_eq!(
        max_channel_delta(&before.pixels, &cool_reference.pixels),
        0,
        "a still frame served from the cache must be the bytes of a renderer that never cached"
    );
    let warm_reference = never_cached_frame(&mut fresh, warm);
    let frames = [
        ("the frame after the change", &after),
        ("the settled frame", &settled_frame),
    ]
    .into_iter()
    .chain(follow_ups.iter().map(|frame| ("a follow-up frame", frame)));
    for (label, frame) in frames {
        assert_eq!(
            max_channel_delta(&frame.pixels, &warm_reference.pixels),
            0,
            "{label} must be the bytes of a renderer that never cached"
        );
    }
}

#[test]
fn a_backdrop_changing_every_frame_leaves_no_unread_pin_behind() {
    let Some((_lock, mut harness)) = harness() else {
        return;
    };
    let still = SceneInput::default();
    for _ in 0..warmup_frames() {
        harness.stats(still);
    }
    let settled = harness.stats(still);
    assert_eq!(
        settled.misses, 0,
        "the warm-up must leave the scene cached: {settled:?}"
    );
    for step in 1..=40 {
        let stats = harness.stats(SceneInput {
            drift: step as f32 * 0.01,
            ..still
        });
        assert!(
            stats.admissions > 0,
            "step {step}: a new key is pinned the frame it appears: {stats:?}"
        );
        assert!(
            stats.entries <= settled.entries + stats.admissions,
            "step {step}: a pin nothing read back is released when its key changes, so the \
             cache holds the settled scene plus this frame's pins, not {} entries against {} \
             settled",
            stats.entries,
            settled.entries
        );
    }
}

#[test]
fn a_backdrop_that_moves_on_after_a_replay_takes_its_pin_with_it() {
    let Some((_lock, mut harness)) = harness() else {
        return;
    };
    let still = SceneInput::default();
    for _ in 0..warmup_frames() {
        harness.stats(still);
    }
    let settled = harness.stats(still);
    assert_eq!(
        settled.misses, 0,
        "the warm-up must leave the scene cached: {settled:?}"
    );
    for step in 1..=20 {
        let input = SceneInput {
            drift: step as f32 * 0.01,
            ..still
        };
        let pinned = harness.stats(input);
        let replayed = harness.stats(input);
        assert!(
            pinned.admissions > 0 && replayed.misses == 0,
            "step {step}: pinned on the first frame, replayed on the second: {pinned:?} {replayed:?}"
        );
        for (label, stats) in [("pinned", pinned), ("replayed", replayed)] {
            assert!(
                stats.entries <= settled.entries + 1,
                "step {step} {label}: a pin lives exactly as long as its key is current, so the \
                 cache holds the settled scene plus one pin, not {} entries against {} settled",
                stats.entries,
                settled.entries
            );
        }
    }
}

fn never_cached_frame(harness: &mut GlassHarness, input: SceneInput) -> CapturedFrame {
    cranpose_render_wgpu::set_debug_toggle(NO_BACKDROP_CACHE, Some("1"));
    for _ in 0..warmup_frames() {
        harness.stats(input);
    }
    let (stats, frame) = harness.frame(input);
    cranpose_render_wgpu::set_debug_toggle(NO_BACKDROP_CACHE, None);
    assert_eq!(
        stats.admissions, 0,
        "the reference must not cache backdrops: {stats:?}"
    );
    frame
}

fn bright_text_pixels(frame: &CapturedFrame, row: usize) -> usize {
    let top = (PAGE_PADDING + row as f32 * (ROW_HEIGHT + ROW_SPACING)) as usize + 14;
    let mut bright = 0;
    for y in top..top + 20 {
        for x in 26..58 {
            let index = (y * frame.width as usize + x) * 4;
            if frame.pixels[index..index + 3]
                .iter()
                .all(|channel| *channel >= 200)
            {
                bright += 1;
            }
        }
    }
    bright
}

#[test]
fn a_cold_frame_draws_every_row_text_over_its_glass() {
    let Some((_lock, mut harness)) = harness() else {
        return;
    };
    let (_, cold) = harness.frame(SceneInput::default());
    let first = bright_text_pixels(&cold, 0);
    assert!(
        first > 50,
        "row 0 must show its text: {first} bright pixels"
    );
    for row in 1..visible_rows() as usize {
        let bright = bright_text_pixels(&cold, row);
        assert!(
            bright * 10 >= first * 9 && bright * 9 <= first * 10,
            "row {row} shows {bright} bright text pixels against {first} in row 0: its glass must \
             read the page below it, not its own text"
        );
    }
}

#[test]
fn a_glass_whose_backdrop_holds_two_frames_is_replayed_on_the_second() {
    let Some((_lock, mut harness)) = harness() else {
        return;
    };
    let still = SceneInput::default();
    for _ in 0..warmup_frames() {
        harness.stats(still);
    }
    let settled = harness.stats(still);
    assert_eq!(
        settled.misses, 0,
        "the warm-up must leave the scene cached: {settled:?}"
    );
    let mut fresh = GlassHarness::new(
        support::headless_renderer_beside_locked().expect("second headless renderer"),
    );
    for step in 1..=4 {
        let input = SceneInput {
            drift: step as f32 * 0.01,
            ..still
        };
        let first = harness.stats(input);
        let (second, frame) = harness.frame(input);
        assert!(
            second.misses == 0 && second.hits > 0,
            "step {step}: the second frame of a hold must replay the backdrop pinned on its \
             first: first {first:?} second {second:?}"
        );
        let reference = never_cached_frame(&mut fresh, input);
        assert_eq!(
            max_channel_delta(&frame.pixels, &reference.pixels),
            0,
            "step {step}: the replayed frame must be the bytes of a renderer that never cached"
        );
    }
}
