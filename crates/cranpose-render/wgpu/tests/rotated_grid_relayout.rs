use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key};
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui::{
    Alignment, Color, GraphicsLayer, Modifier, TextStyle, composable,
    widgets::{Box, BoxSpec, Column, ColumnSpec, Row, RowSpec, Text},
};

use crate::support;

const FRAME_WIDTH: u32 = 360;
const FRAME_HEIGHT: u32 = 480;
const COLUMNS: usize = 6;
const ROWS: usize = 10;
const CELLS: u32 = (COLUMNS * ROWS) as u32;

const PALETTE: [Color; 3] = [
    Color(0.85, 0.25, 0.30, 1.0),
    Color(0.20, 0.55, 0.85, 1.0),
    Color(0.25, 0.70, 0.40, 1.0),
];

#[composable]
fn Cell(index: usize) {
    Box(
        Modifier::empty()
            .weight(1.0)
            .fill_max_height()
            .padding(1.0)
            .graphics_layer_value(GraphicsLayer {
                rotation_z: ((index % 7) as f32 - 3.0) * 2.0,
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
fn Grid(width: MutableState<f32>) {
    Column(
        Modifier::empty()
            .fill_max_width_fraction(width.get())
            .fill_max_height(),
        ColumnSpec::default(),
        move || {
            for row in 0..ROWS {
                Row(
                    Modifier::empty().fill_max_width().weight(1.0),
                    RowSpec::default(),
                    move || {
                        for column in 0..COLUMNS {
                            Cell(row * COLUMNS + column);
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
}

impl GridHarness {
    fn new(renderer: WgpuRenderer) -> Self {
        let root_key = location_key(file!(), line!(), column!());
        let width: Rc<RefCell<Option<MutableState<f32>>>> = Rc::new(RefCell::new(None));
        let width_for_app = Rc::clone(&width);
        let mut shell = AppShell::new(renderer, root_key, move || {
            let state = cranpose_core::rememberMutableStateOf(|| 1.0f32);
            *width_for_app.borrow_mut() = Some(state);
            Grid(state);
        });
        shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
        shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
        shell.update();
        Self { shell, width }
    }

    fn frame(&mut self, fraction: f32) -> (RenderStatsSnapshot, CapturedFrame) {
        let state = self
            .width
            .borrow()
            .as_ref()
            .copied()
            .expect("state captured");
        self.shell.debug_enter_app_context(|| state.set(fraction));
        support::update_and_capture(&mut self.shell, FRAME_WIDTH, FRAME_HEIGHT)
    }
}

fn width_fraction(frame: usize) -> f32 {
    0.7 + 0.3 * (0.5 + 0.5 * (frame as f32 / 60.0 * 2.0).sin())
}

const WARMUP_FRAMES: usize = 3;
const MEASURED_FRAMES: usize = 6;
const MAX_PASSES: u32 = 6;

fn harness() -> Option<(std::sync::MutexGuard<'static, ()>, GridHarness)> {
    match support::headless_renderer_parts() {
        Ok((lock, renderer)) => Some((lock, GridHarness::new(renderer))),
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            None
        }
    }
}

fn fresh_harness() -> GridHarness {
    GridHarness::new(support::headless_renderer_beside_locked().expect("reference renderer"))
}

fn settled(harness: &mut GridHarness, frame: usize) -> CapturedFrame {
    support::settle(|| harness.frame(width_fraction(frame)))
}

#[test]
fn a_relayout_under_rotated_cells_draws_them_in_a_few_passes() {
    let Some((_lock, mut harness)) = harness() else {
        return;
    };
    for frame in 0..WARMUP_FRAMES {
        harness.frame(width_fraction(frame));
    }
    for frame in WARMUP_FRAMES..WARMUP_FRAMES + MEASURED_FRAMES {
        let (stats, _) = harness.frame(width_fraction(frame));
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
fn rotated_cells_drawn_together_draw_what_each_drawn_alone_draws() {
    let Some((_lock, mut moving)) = harness() else {
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
        let expected = settled(&mut fresh_harness(), frame);
        support::assert_same_bytes(
            &format!("frame {frame}"),
            FRAME_WIDTH,
            &expected.pixels,
            &actual.pixels,
        );
    }
}

#[test]
fn a_rotated_grid_that_stops_moving_draws_what_a_fresh_renderer_draws() {
    let Some((_lock, mut moving)) = harness() else {
        return;
    };
    for frame in 0..WARMUP_FRAMES + MEASURED_FRAMES {
        moving.frame(width_fraction(frame));
    }
    support::wait_for_background_compiler_idle();
    let held = WARMUP_FRAMES + MEASURED_FRAMES;
    moving.frame(width_fraction(held));
    let still = settled(&mut moving, held);
    let expected = settled(&mut fresh_harness(), held);
    support::assert_same_bytes("still frame", FRAME_WIDTH, &expected.pixels, &still.pixels);
}
