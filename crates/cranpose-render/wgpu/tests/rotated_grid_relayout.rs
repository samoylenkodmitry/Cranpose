use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key};
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui::{
    Alignment, Color, CompositingStrategy, GraphicsLayer, Modifier, TextStyle, composable,
    widgets::{Box, BoxSpec, Column, ColumnSpec, Row, RowSpec, Text},
};

use crate::support;

const FRAME_WIDTH: u32 = 360;
const FRAME_HEIGHT: u32 = 480;
const COLUMNS: usize = 6;
const ROWS: usize = 10;
const CELLS: u32 = (COLUMNS * ROWS) as u32;

/// The screen a grid fills and how many cells it holds.
#[derive(Clone, Copy, PartialEq)]
struct Screen {
    size: (u32, u32),
    density: f32,
    columns: usize,
    rows: usize,
}

const SMALL: Screen = Screen {
    size: (FRAME_WIDTH, FRAME_HEIGHT),
    density: 1.0,
    columns: COLUMNS,
    rows: ROWS,
};

/// The benchmark's grid_layer on a phone: its last rows of cells sit two
/// thousand pixels down.
const PHONE: Screen = Screen {
    size: (1080, 2200),
    density: 2.625,
    columns: 12,
    rows: 30,
};

const PALETTE: [Color; 3] = [
    Color(0.85, 0.25, 0.30, 1.0),
    Color(0.20, 0.55, 0.85, 1.0),
    Color(0.25, 0.70, 0.40, 1.0),
];

/// One cell of the grid, turned by a few degrees. An offscreen cell asks for a
/// surface of its own; any other draws in place, straight into the page.
#[composable]
fn Cell(index: usize, offscreen: bool) {
    Box(
        Modifier::empty()
            .weight(1.0)
            .fill_max_height()
            .padding(1.0)
            .graphics_layer_value(GraphicsLayer {
                rotation_z: ((index % 7) as f32 - 3.0) * 2.0,
                compositing_strategy: if offscreen {
                    CompositingStrategy::Offscreen
                } else {
                    CompositingStrategy::Auto
                },
                ..Default::default()
            })
            .background(PALETTE[index % PALETTE.len()])
            .rounded_corners(3.0),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        move || {
            Text(
                format!("cell {index}"),
                Modifier::empty(),
                TextStyle::default(),
            );
        },
    );
}

#[composable]
fn Grid(width: MutableState<f32>, offscreen: bool, screen: Screen) {
    Column(
        Modifier::empty()
            .fill_max_width_fraction(width.get())
            .fill_max_height(),
        ColumnSpec::default(),
        move || {
            for row in 0..screen.rows {
                Row(
                    Modifier::empty().fill_max_width().weight(1.0),
                    RowSpec::default(),
                    move || {
                        for column in 0..screen.columns {
                            Cell(row * screen.columns + column, offscreen);
                        }
                    },
                );
            }
        },
    );
}

struct GridHarness {
    shell: AppShell<WgpuRenderer>,
    width: Rc<RefCell<Option<MutableState<f32>>>>,
    size: (u32, u32),
}

impl GridHarness {
    fn new(renderer: WgpuRenderer, offscreen: bool) -> Self {
        Self::on(renderer, offscreen, SMALL)
    }

    fn on(renderer: WgpuRenderer, offscreen: bool, screen: Screen) -> Self {
        let root_key = location_key(file!(), line!(), column!());
        let width: Rc<RefCell<Option<MutableState<f32>>>> = Rc::new(RefCell::new(None));
        let width_for_app = Rc::clone(&width);
        let mut shell = AppShell::new(renderer, root_key, move || {
            let state = cranpose_core::rememberMutableStateOf(|| 1.0f32);
            *width_for_app.borrow_mut() = Some(state);
            Grid(state, offscreen, screen);
        });
        let (width_px, height_px) = screen.size;
        shell.set_density(screen.density);
        shell.set_viewport(
            width_px as f32 / screen.density,
            height_px as f32 / screen.density,
        );
        shell.set_buffer_size(width_px, height_px);
        shell.update();
        Self {
            shell,
            width,
            size: screen.size,
        }
    }

    fn frame(&mut self, fraction: f32) -> (RenderStatsSnapshot, CapturedFrame) {
        let state = self
            .width
            .borrow()
            .as_ref()
            .copied()
            .expect("state captured");
        self.shell.debug_enter_app_context(|| state.set(fraction));
        support::update_and_capture(&mut self.shell, self.size.0, self.size.1)
    }
}

fn width_fraction(frame: usize) -> f32 {
    0.7 + 0.3 * (0.5 + 0.5 * (frame as f32 / 60.0 * 2.0).sin())
}

const WARMUP_FRAMES: usize = 3;

/// A grid width that grows by more than a pixel per column every frame, so
/// every cell's whole-pixel width changes: the animated width moves under
/// two pixels a frame, which resizes only the columns those pixels land in.
fn growing_width_fraction(frame: usize) -> f32 {
    0.7 + 0.03 * frame as f32
}
const MEASURED_FRAMES: usize = 6;
const MAX_PASSES: u32 = 6;
const IN_PLACE_MAX_PASSES: u32 = 3;
/// The frames content that can draw in place holds still before its surface
/// is kept.
const IN_PLACE_PATIENCE: usize = 16;
const EXTREMUM_SPAN: usize = 100;

fn harness(offscreen: bool) -> Option<(std::sync::MutexGuard<'static, ()>, GridHarness)> {
    harness_on(offscreen, SMALL)
}

fn harness_on(
    offscreen: bool,
    screen: Screen,
) -> Option<(std::sync::MutexGuard<'static, ()>, GridHarness)> {
    match support::headless_renderer_parts() {
        Ok((lock, renderer)) => Some((lock, GridHarness::on(renderer, offscreen, screen))),
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            None
        }
    }
}

fn fresh_harness(offscreen: bool) -> GridHarness {
    GridHarness::new(
        support::headless_renderer_beside_locked().expect("reference renderer"),
        offscreen,
    )
}

fn settled(harness: &mut GridHarness, frame: usize) -> CapturedFrame {
    support::settle(|| harness.frame(width_fraction(frame)))
}

#[test]
fn a_relayout_under_offscreen_rotated_cells_draws_them_in_a_few_passes() {
    let Some((_lock, mut harness)) = harness(true) else {
        return;
    };
    for frame in 0..WARMUP_FRAMES {
        harness.frame(growing_width_fraction(frame));
    }
    for frame in WARMUP_FRAMES..WARMUP_FRAMES + MEASURED_FRAMES {
        let (stats, _) = harness.frame(growing_width_fraction(frame));
        assert!(
            stats.isolated_layer_renders > CELLS / 2,
            "every frame resizes the cells, so they must be drawn again: {stats:?}"
        );
        assert!(
            stats.pass_count <= MAX_PASSES,
            "frame {frame} redrew {} rotated cells in {} passes; one pass per cell costs \
             a tiling GPU a pass setup and a store per cell: {stats:?}",
            stats.isolated_layer_renders,
            stats.pass_count
        );
    }
}

#[test]
fn offscreen_rotated_cells_drawn_together_draw_what_each_drawn_alone_draws() {
    let Some((_lock, mut moving)) = harness(true) else {
        return;
    };
    for frame in 0..WARMUP_FRAMES {
        moving.frame(width_fraction(frame));
    }
    support::wait_for_background_compiler_idle();
    for frame in WARMUP_FRAMES..WARMUP_FRAMES + MEASURED_FRAMES {
        let (stats, actual) = moving.frame(width_fraction(frame));
        if frame % 3 != 2 {
            continue;
        }
        assert!(
            support::pipelines_settled(&stats),
            "frame {frame} drew with stand-in pipelines: {stats:?}"
        );
        assert!(
            stats.pass_count <= MAX_PASSES,
            "frame {frame} must draw its cells together for this to compare anything: {stats:?}"
        );
        let expected = settled(&mut fresh_harness(true), frame);
        support::assert_same_bytes(
            &format!("frame {frame}"),
            FRAME_WIDTH,
            &expected.pixels,
            &actual.pixels,
        );
    }
}

fn assert_still_frame_matches_a_fresh_renderer(offscreen: bool) {
    let Some((_lock, mut moving)) = harness(offscreen) else {
        return;
    };
    for frame in 0..WARMUP_FRAMES + MEASURED_FRAMES {
        moving.frame(width_fraction(frame));
    }
    support::wait_for_background_compiler_idle();
    let held = WARMUP_FRAMES + MEASURED_FRAMES;
    moving.frame(width_fraction(held));
    let still = settled(&mut moving, held);
    let mut fresh = fresh_harness(offscreen);
    fresh.frame(width_fraction(held));
    let expected = settled(&mut fresh, held);
    support::assert_same_bytes("still frame", FRAME_WIDTH, &expected.pixels, &still.pixels);
}

#[test]
fn an_offscreen_rotated_grid_that_stops_moving_draws_what_a_fresh_renderer_draws() {
    assert_still_frame_matches_a_fresh_renderer(true);
}

#[test]
fn a_rotated_grid_drawn_in_place_that_stops_moving_draws_what_a_fresh_renderer_draws() {
    assert_still_frame_matches_a_fresh_renderer(false);
}

#[test]
fn a_relayout_under_rotated_cells_draws_them_in_place_without_surfaces() {
    let Some((_lock, mut harness)) = harness(false) else {
        return;
    };
    for frame in 0..WARMUP_FRAMES + MEASURED_FRAMES {
        let (stats, _) = harness.frame(width_fraction(frame));
        assert_eq!(
            stats.isolated_layer_renders, 0,
            "frame {frame}: a cell that only turns draws straight into the page: {stats:?}"
        );
        assert_eq!(
            stats.layer_cache_size, 0,
            "frame {frame}: cells drawn in place keep no surfaces: {stats:?}"
        );
        assert!(
            stats.pass_count <= IN_PLACE_MAX_PASSES,
            "frame {frame} drew the grid in {} passes: {stats:?}",
            stats.pass_count
        );
    }
}

#[test]
fn rotated_cells_far_down_a_phone_screen_draw_in_place() {
    let Some((_lock, mut harness)) = harness_on(false, PHONE) else {
        return;
    };
    for frame in 0..WARMUP_FRAMES + MEASURED_FRAMES {
        let (stats, _) = harness.frame(width_fraction(frame));
        assert_eq!(
            stats.isolated_layer_renders, 0,
            "frame {frame}: a turned cell draws straight into the page however far from the \
             origin it sits: {stats:?}"
        );
    }
}

#[test]
fn offscreen_rotated_cells_that_hold_still_for_a_moment_are_kept_by_copy_and_one_each() {
    let Some((_lock, mut harness)) = harness(true) else {
        return;
    };
    harness.frame(width_fraction(0));
    let mut kept = 0;
    for frame in 1..EXTREMUM_SPAN {
        let (stats, _) = harness.frame(width_fraction(frame));
        kept = kept.max(stats.layer_cache_size);
        assert!(
            stats.pass_count <= MAX_PASSES,
            "frame {frame}: cells the cache keeps render together in an atlas, not \
             drawn in passes of their own: {stats:?}"
        );
        assert!(
            stats.layer_cache_size <= CELLS,
            "frame {frame}: a cell's new surface replaces its old one, so the cache holds \
             at most one surface per cell, not {}: {stats:?}",
            stats.layer_cache_size
        );
    }
    assert!(
        kept > CELLS / 2,
        "the width must hold still long enough near its turn for the cache to keep the cells, \
         or this proves nothing: kept at most {kept}"
    );
}

#[test]
fn rotated_cells_kept_once_they_hold_still_render_their_surfaces_together() {
    let Some((_lock, mut harness)) = harness(false) else {
        return;
    };
    let held = width_fraction(0);
    for frame in 0..IN_PLACE_PATIENCE {
        let (stats, _) = harness.frame(held);
        assert_eq!(
            stats.isolated_layer_renders, 0,
            "frame {frame}: cells draw in place until they have held still: {stats:?}"
        );
    }
    let (kept, _) = harness.frame(held);
    assert!(
        kept.isolated_layer_renders > CELLS / 2,
        "cells that held still keep their surfaces: {kept:?}"
    );
    assert!(
        kept.pass_count <= MAX_PASSES,
        "{} cells kept at once render together, not in {} passes: {kept:?}",
        kept.isolated_layer_renders,
        kept.pass_count
    );
    let (still, _) = harness.frame(held);
    assert_eq!(
        still.isolated_layer_renders, 0,
        "the kept surfaces serve the next frame: {still:?}"
    );
}

/// Two turns of the width's motion at 60 Hz.
const FULL_PERIOD: usize = 190;
/// How often a cell may be copied into the cache over that period: once per
/// pause of the motion and a few times while its gate learns the churn. A
/// gate that counts a surface as paid for at its first read copies each
/// cell about 32 times.
const COPIES_PER_CELL: u32 = 7;

/// Offscreen cells render together into atlases, and a kept cell keeps its
/// atlas. On a phone a pixel-at-a-time relayout holds a cell's size
/// two or three frames: a copy read once and then replaced cost a copy and
/// a texture for nothing, and on the device that churn held 30 MB more GPU
/// memory than drawing every cell every frame did (#920).
#[test]
fn offscreen_rotated_cells_resized_a_pixel_at_a_time_are_not_copied_every_hold() {
    let Some((_lock, mut harness)) = harness_on(true, PHONE) else {
        return;
    };
    let cells = (PHONE.columns * PHONE.rows) as u32;
    let (mut copies, mut new_textures) = (0, 0);
    for frame in 0..FULL_PERIOD {
        let (stats, _) = harness.frame(width_fraction(frame));
        if frame >= WARMUP_FRAMES {
            copies += stats.copy_count;
            new_textures += stats.offscreen_news;
        }
    }
    assert!(
        copies <= COPIES_PER_CELL * cells,
        "{copies} copies of {cells} cells over {FULL_PERIOD} frames"
    );
    assert!(
        new_textures <= COPIES_PER_CELL * cells,
        "{new_textures} new textures for {cells} cells over {FULL_PERIOD} frames"
    );
}

/// Draws a settled frame of the grid drawn in place, one cell in seven upright among
/// turned ones, may take: the labels of both kinds of cell share pixels
/// with none of the other kind, so each kind draws its labels together.
const IN_PLACE_MAX_DRAWS: u32 = 6;

#[test]
fn labels_of_upright_cells_among_turned_ones_draw_together() {
    let Some((_lock, mut harness)) = harness(false) else {
        return;
    };
    let mut stats = None;
    for frame in 0..WARMUP_FRAMES + MEASURED_FRAMES {
        stats = Some(harness.frame(width_fraction(frame)).0);
    }
    let Some(stats) = stats else {
        return;
    };
    assert!(
        stats.draw_calls <= IN_PLACE_MAX_DRAWS,
        "the settled grid drew in {} draws: {stats:?}",
        stats.draw_calls
    );
}
