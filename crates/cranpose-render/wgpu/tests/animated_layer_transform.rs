use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key};
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui::{
    Alignment, Color, LinearArrangement, Modifier, TextStyle, composable,
    widgets::{Box, BoxSpec, Column, ColumnSpec, Row, RowSpec, Text},
};

use crate::support;

const FRAME_WIDTH: u32 = 360;
const FRAME_HEIGHT: u32 = 480;
const COLUMNS: usize = 3;
const ROWS: usize = 4;
const TILES: u32 = (COLUMNS * ROWS) as u32;
const WARMUP_FRAMES: usize = 4;
const MEASURED_FRAMES: usize = 12;
const SCALE_PERIOD_FRAMES: usize = 126;
const MAX_STEPS_PER_TILE: u32 = 6;

const PALETTE: [Color; 4] = [
    Color(0.85, 0.25, 0.30, 1.0),
    Color(0.20, 0.55, 0.85, 1.0),
    Color(0.25, 0.70, 0.40, 1.0),
    Color(0.80, 0.60, 0.15, 1.0),
];

#[composable]
fn Tile(index: usize, seconds: MutableState<f32>, opaque: bool) {
    let phase = index as f32;
    Box(
        Modifier::empty()
            .weight(1.0)
            .fill_max_height()
            .graphics_layer_block(move |layer| {
                let t = seconds.get();
                layer.rotation_z = (t * 90.0 + phase * 13.0) % 360.0;
                let scale = 0.85 + 0.15 * (t * 3.0 + phase * 0.4).sin();
                layer.scale_x = scale;
                layer.scale_y = scale;
                if !opaque {
                    layer.alpha = 0.65 + 0.35 * (0.5 + 0.5 * (t * 2.0 + phase * 0.7).sin());
                }
            })
            .background(PALETTE[index % PALETTE.len()])
            .rounded_corners(12.0),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        move || {
            Text(index.to_string(), Modifier::empty(), TextStyle::default());
        },
    );
}

#[composable]
fn Tiles(seconds: MutableState<f32>, opaque: bool) {
    Column(
        Modifier::empty().fill_max_size().padding(4.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(8.0)),
        move || {
            for row in 0..ROWS {
                Row(
                    Modifier::empty().fill_max_width().weight(1.0),
                    RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(8.0)),
                    move || {
                        for column in 0..COLUMNS {
                            Tile(row * COLUMNS + column, seconds, opaque);
                        }
                    },
                );
            }
        },
    );
}

struct TileHarness {
    shell: AppShell<WgpuRenderer>,
    seconds: Rc<RefCell<Option<MutableState<f32>>>>,
}

impl TileHarness {
    fn new(renderer: WgpuRenderer, opaque: bool) -> Self {
        let root_key = location_key(file!(), line!(), column!());
        let seconds: Rc<RefCell<Option<MutableState<f32>>>> = Rc::new(RefCell::new(None));
        let seconds_for_app = Rc::clone(&seconds);
        let mut shell = AppShell::new(renderer, root_key, move || {
            let state = cranpose_core::rememberMutableStateOf(|| 0.0f32);
            *seconds_for_app.borrow_mut() = Some(state);
            Tiles(state, opaque);
        });
        shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
        shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
        shell.update();
        Self { shell, seconds }
    }

    fn frame(&mut self, seconds: f32) -> (RenderStatsSnapshot, CapturedFrame) {
        let state = self
            .seconds
            .borrow()
            .as_ref()
            .copied()
            .expect("state captured");
        self.shell.debug_enter_app_context(|| state.set(seconds));
        support::update_and_capture(&mut self.shell, FRAME_WIDTH, FRAME_HEIGHT)
    }

    fn settled_frame(&mut self, seconds: f32) -> CapturedFrame {
        support::settle(|| self.frame(seconds))
    }
}

fn frame_seconds(frame: usize) -> f32 {
    frame as f32 / 60.0
}

fn harness(opaque: bool) -> Option<(std::sync::MutexGuard<'static, ()>, TileHarness)> {
    match support::headless_renderer_parts() {
        Ok((lock, renderer)) => Some((lock, TileHarness::new(renderer, opaque))),
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            None
        }
    }
}

fn fresh_harness() -> TileHarness {
    TileHarness::new(
        support::headless_renderer_beside_locked().expect("reference renderer"),
        false,
    )
}

fn second_period_stats(harness: &mut TileHarness) -> Vec<RenderStatsSnapshot> {
    for frame in 0..SCALE_PERIOD_FRAMES {
        harness.frame(frame_seconds(frame));
    }
    (SCALE_PERIOD_FRAMES..SCALE_PERIOD_FRAMES + MEASURED_FRAMES)
        .map(|frame| harness.frame(frame_seconds(frame)).0)
        .collect()
}

fn assert_no_texture_once_scales_were_seen(opaque: bool) {
    let Some((_lock, mut harness)) = harness(opaque) else {
        return;
    };
    let stats = second_period_stats(&mut harness);
    let news: Vec<u32> = stats.iter().map(|stats| stats.offscreen_news).collect();
    assert!(
        news.iter().all(|news| *news == 0),
        "tiles (opaque: {opaque}) whose transform keeps changing over unchanged content must \
         reuse what an earlier period drew, not create textures every frame: {news:?}"
    );
}

fn assert_almost_no_surface_redrawn(opaque: bool) {
    let Some((_lock, mut harness)) = harness(opaque) else {
        return;
    };
    let stats = second_period_stats(&mut harness);
    let renders: u32 = stats.iter().map(|stats| stats.isolated_layer_renders).sum();
    assert!(
        renders <= TILES,
        "a period after every scale step was drawn, {MEASURED_FRAMES} frames of {TILES} \
         animated tiles (opaque: {opaque}) redrew {renders} surfaces; before the fix they \
         redrew every tile every frame"
    );
    let sizes: Vec<u32> = stats.iter().map(|stats| stats.layer_cache_size).collect();
    assert!(
        sizes.iter().all(|size| *size <= TILES * MAX_STEPS_PER_TILE),
        "each tile keeps at most one raster per scale step it passes through: {sizes:?}"
    );
}

#[test]
fn an_animated_layer_transform_allocates_no_texture_once_its_scales_were_seen() {
    assert_no_texture_once_scales_were_seen(false);
}

#[test]
fn an_animated_opaque_layer_transform_allocates_no_texture_once_its_scales_were_seen() {
    assert_no_texture_once_scales_were_seen(true);
}

#[test]
fn an_animated_layer_transform_redraws_almost_no_surface_and_bounds_the_cache() {
    assert_almost_no_surface_redrawn(false);
}

#[test]
fn an_animated_opaque_layer_transform_redraws_almost_no_surface_and_bounds_the_cache() {
    assert_almost_no_surface_redrawn(true);
}

#[test]
fn a_layer_that_stops_scaling_draws_what_a_fresh_renderer_draws() {
    let Some((_lock, mut animated)) = harness(false) else {
        return;
    };
    for frame in 0..WARMUP_FRAMES {
        animated.frame(frame_seconds(frame));
    }
    support::wait_for_background_compiler_idle();
    let held = frame_seconds(WARMUP_FRAMES);
    let (_, moving) = animated.frame(held);
    let (stats, still) = animated.frame(held);
    assert!(
        support::pipelines_settled(&stats),
        "the still frame drew with stand-in pipelines: {stats:?}"
    );
    assert!(
        stats.isolated_layer_renders >= TILES,
        "the first frame a scale holds redraws every tile at its exact scale: {stats:?}"
    );
    let expected = fresh_harness().settled_frame(held);
    support::assert_same_bytes("still frame", FRAME_WIDTH, &expected.pixels, &still.pixels);
    assert_ne!(
        moving.pixels, still.pixels,
        "the moving frame must differ from the still one, or the still frame proves nothing"
    );
}

#[test]
fn an_animated_frame_draws_what_a_fresh_renderer_given_the_same_motion_draws() {
    let Some((_lock, mut animated)) = harness(false) else {
        return;
    };
    for frame in 0..WARMUP_FRAMES {
        animated.frame(frame_seconds(frame));
    }
    support::wait_for_background_compiler_idle();
    for frame in WARMUP_FRAMES..WARMUP_FRAMES + MEASURED_FRAMES {
        let (stats, actual) = animated.frame(frame_seconds(frame));
        if frame % 4 != 3 {
            continue;
        }
        assert!(
            support::pipelines_settled(&stats),
            "frame {frame} drew with stand-in pipelines: {stats:?}"
        );
        let mut fresh = fresh_harness();
        fresh.settled_frame(frame_seconds(frame - 1));
        let (fresh_stats, expected) = fresh.frame(frame_seconds(frame));
        assert!(
            support::pipelines_settled(&fresh_stats),
            "the reference frame {frame} drew with stand-in pipelines: {fresh_stats:?}"
        );
        support::assert_same_bytes(
            &format!("frame {frame}"),
            FRAME_WIDTH,
            &expected.pixels,
            &actual.pixels,
        );
    }
}
