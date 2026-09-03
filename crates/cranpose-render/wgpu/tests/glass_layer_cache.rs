mod support;

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key};
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_ui::{
    Color, LinearArrangement, Modifier, RenderEffect, TextStyle, composable,
    widgets::{Box, BoxSpec, Column, ColumnSpec, LazyColumn, LazyColumnSpec, Text},
};

const FRAME_WIDTH: u32 = 640;
const FRAME_HEIGHT: u32 = 640;

#[composable]
#[allow(non_snake_case)]
fn GlassRow(index: usize) {
    let tint = match index % 4 {
        0 => Color(0.20, 0.26, 0.36, 0.55),
        1 => Color(0.28, 0.20, 0.34, 0.55),
        2 => Color(0.18, 0.32, 0.30, 0.55),
        _ => Color(0.32, 0.26, 0.20, 0.55),
    };
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(96.0)
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
fn GlassScene(list_state: LazyListState, pulse: MutableState<f32>, drift: MutableState<f32>) {
    Box(
        Modifier::empty()
            .size_points(FRAME_WIDTH as f32, FRAME_HEIGHT as f32)
            .background(Color(0.03, 0.04, 0.06, 1.0))
            .rounded_corners(18.0),
        BoxSpec::new(),
        move || {
            Box(
                Modifier::empty().fill_max_size().padding(12.0),
                BoxSpec::new(),
                move || {
                    LazyColumn(
                        Modifier::empty().fill_max_size(),
                        list_state,
                        LazyColumnSpec::new()
                            .vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                        move |scope| {
                            scope.items(LazyItems::new(40).key(|i: usize| i as u64), GlassRow);
                        },
                    );
                    GlassOverlay(pulse, drift);
                },
            );
        },
    );
}

struct GlassHarness {
    shell: AppShell<cranpose_render_wgpu::WgpuRenderer>,
    list_state: Rc<RefCell<Option<LazyListState>>>,
    pulse: Rc<RefCell<Option<MutableState<f32>>>>,
    drift: Rc<RefCell<Option<MutableState<f32>>>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct FrameCacheStats {
    hits: u32,
    misses: u32,
    blur_passes: u32,
    isolated_layer_renders: u32,
}

impl GlassHarness {
    fn new(renderer: cranpose_render_wgpu::WgpuRenderer) -> Self {
        let root_key = location_key(file!(), line!(), column!());
        let list_state: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
        let pulse: Rc<RefCell<Option<MutableState<f32>>>> = Rc::new(RefCell::new(None));
        let drift: Rc<RefCell<Option<MutableState<f32>>>> = Rc::new(RefCell::new(None));
        let list_state_for_app = Rc::clone(&list_state);
        let pulse_for_app = Rc::clone(&pulse);
        let drift_for_app = Rc::clone(&drift);
        let mut shell = AppShell::new(renderer, root_key, move || {
            let state = rememberLazyListState();
            let pulse_state = cranpose_core::rememberMutableStateOf(|| 0.0f32);
            let drift_state = cranpose_core::rememberMutableStateOf(|| 0.0f32);
            *list_state_for_app.borrow_mut() = Some(state);
            *pulse_for_app.borrow_mut() = Some(pulse_state);
            *drift_for_app.borrow_mut() = Some(drift_state);
            GlassScene(state, pulse_state, drift_state);
        });
        shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
        shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
        shell.update();
        Self {
            shell,
            list_state,
            pulse,
            drift,
        }
    }

    fn frame(&mut self, scroll_delta: f32, pulse: f32, drift: f32) -> FrameCacheStats {
        if scroll_delta != 0.0 {
            let state = self
                .list_state
                .borrow()
                .as_ref()
                .cloned()
                .expect("list state captured");
            self.shell
                .debug_enter_app_context(|| state.dispatch_scroll_delta(scroll_delta));
        }
        let pulse_state = self
            .pulse
            .borrow()
            .as_ref()
            .cloned()
            .expect("pulse state captured");
        let drift_state = self
            .drift
            .borrow()
            .as_ref()
            .cloned()
            .expect("drift state captured");
        self.shell.debug_enter_app_context(|| {
            pulse_state.set(pulse);
            drift_state.set(drift);
        });
        self.shell.update();
        self.shell
            .renderer()
            .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
            .expect("frame capture should succeed");
        let stats = self
            .shell
            .renderer()
            .last_frame_stats()
            .expect("frame stats");
        FrameCacheStats {
            hits: stats.layer_cache_hits,
            misses: stats.layer_cache_misses,
            blur_passes: stats.blur_passes,
            isolated_layer_renders: stats.isolated_layer_renders,
        }
    }
}

const WARMUP_FRAMES: usize = 4;
const MEASURED_FRAMES: usize = 8;

#[test]
fn a_still_glass_scene_leaves_no_layer_cache_misses() {
    let (_lock, renderer) = match support::headless_renderer_parts() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            return;
        }
    };
    let mut harness = GlassHarness::new(renderer);

    for _ in 0..WARMUP_FRAMES {
        harness.frame(0.0, 0.0, 0.0);
    }
    for frame_index in 0..MEASURED_FRAMES {
        let stats = harness.frame(0.0, 0.0, 0.0);
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
    let (_lock, renderer) = match support::headless_renderer_parts() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            return;
        }
    };
    let mut harness = GlassHarness::new(renderer);

    let animation = |frame_index: usize| {
        let t = (frame_index + 1) as f32 / 16.0;
        (t, t)
    };
    for frame_index in 0..WARMUP_FRAMES {
        let (pulse, drift) = animation(frame_index);
        harness.frame(0.0, pulse, drift);
    }
    for frame_index in 0..MEASURED_FRAMES {
        let (pulse, drift) = animation(WARMUP_FRAMES + frame_index);
        let stats = harness.frame(0.0, pulse, drift);
        assert_eq!(
            stats.misses, 1,
            "an animated overlay re-renders exactly its own backdrop blur, its content \
             draws straight into the page; anything more means a still row lost its \
             cache (frame {frame_index}): {stats:?}"
        );
        assert!(
            stats.hits >= 12,
            "the still rows' surfaces and backdrops must keep hitting while the \
             overlay animates (frame {frame_index}): {stats:?}"
        );
        assert_eq!(
            stats.blur_passes, 1,
            "only the overlay's backdrop re-blurs while the rows hold still \
             (frame {frame_index}): {stats:?}"
        );
    }
}
