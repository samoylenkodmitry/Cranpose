use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key};
use cranpose_render_wgpu::{CapturedFrame, WgpuRenderer};
use cranpose_ui::{
    Color, GraphicsLayer, LinearArrangement, Modifier, TextStyle, composable,
    widgets::{Box, BoxSpec, Column, ColumnSpec, Row, RowSpec, Text},
};

use crate::support;

const FRAME_WIDTH: u32 = 360;
const FRAME_HEIGHT: u32 = 1200;
const CHIPS: usize = 6;
const DEPTH_PAST_LIMIT: usize = 132;
const IN_PLACE_MAX_PASSES: u32 = 3;
const INNERMOST: Color = Color(0.95, 0.10, 0.45, 1.0);

const LEVEL_BACKGROUND: [Color; 2] = [
    Color(0.886, 0.910, 0.941, 1.0),
    Color(0.796, 0.835, 0.882, 1.0),
];

#[composable]
fn Level(remaining: usize) {
    let background = if remaining == 0 {
        INNERMOST
    } else {
        LEVEL_BACKGROUND[remaining % 2]
    };
    Column(
        Modifier::empty()
            .fill_max_width()
            .graphics_layer_value(GraphicsLayer {
                rotation_z: if remaining.is_multiple_of(2) {
                    0.6
                } else {
                    -0.6
                },
                ..Default::default()
            })
            .background(background)
            .rounded_corners(4.0)
            .padding_each(3.0, 1.0, 1.0, 1.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(1.0)),
        move || {
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(2.0)),
                move || {
                    for chip in 0..CHIPS {
                        Text(
                            format!("L{remaining}.{chip}"),
                            Modifier::empty().weight(1.0),
                            TextStyle::default(),
                        );
                    }
                },
            );
            if remaining > 0 {
                Level(remaining - 1);
            }
        },
    );
}

#[composable]
fn Deep(width: MutableState<f32>, depth: usize) {
    Box(
        Modifier::empty().fill_max_width_fraction(width.get()),
        BoxSpec::default(),
        move || Level(depth),
    );
}

struct DeepHarness {
    shell: AppShell<WgpuRenderer>,
    width: Rc<RefCell<Option<MutableState<f32>>>>,
}

impl DeepHarness {
    fn new(renderer: WgpuRenderer, depth: usize) -> Self {
        let root_key = location_key(file!(), line!(), column!());
        let width: Rc<RefCell<Option<MutableState<f32>>>> = Rc::new(RefCell::new(None));
        let width_for_app = Rc::clone(&width);
        let mut shell = AppShell::new(renderer, root_key, move || {
            let state = cranpose_core::rememberMutableStateOf(|| 1.0f32);
            *width_for_app.borrow_mut() = Some(state);
            Deep(state, depth);
        });
        shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
        shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
        shell.update();
        Self { shell, width }
    }

    fn frame(&mut self, frame: usize) -> Result<CapturedFrame, String> {
        let state = self
            .width
            .borrow()
            .as_ref()
            .copied()
            .expect("state captured");
        let fraction = 0.7 + 0.3 * (0.5 + 0.5 * (frame as f32 / 60.0 * 2.0).sin());
        self.shell.debug_enter_app_context(|| state.set(fraction));
        self.shell.update();
        self.shell
            .renderer()
            .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
            .map_err(|error| format!("{error:?}"))
    }
}

fn is_innermost(pixel: &[u8; 4]) -> bool {
    pixel[0] > 200 && pixel[1] < 80 && pixel[2] > 70 && pixel[2] < 160
}

fn is_level_background(pixel: &[u8; 4]) -> bool {
    LEVEL_BACKGROUND.iter().any(|color| {
        [color.0, color.1, color.2]
            .iter()
            .zip(pixel)
            .all(|(channel, byte)| (channel * 255.0 - f32::from(*byte)).abs() < 6.0)
    })
}

fn innermost_pixels(frame: &CapturedFrame) -> usize {
    frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| is_innermost(pixel))
        .count()
}

#[test]
fn forty_nested_rotated_levels_draw_down_to_the_innermost() {
    let Ok((_lock, renderer)) = support::headless_renderer_parts() else {
        return;
    };
    let mut harness = DeepHarness::new(renderer, 40);
    for frame in 0..3 {
        let captured = harness
            .frame(frame)
            .unwrap_or_else(|error| panic!("frame {frame} of forty nested layers failed: {error}"));
        assert!(
            innermost_pixels(&captured) > 200,
            "frame {frame}: the innermost of forty nested rotated levels must reach the frame"
        );
    }
}

#[test]
fn forty_nested_rotated_levels_that_relayout_draw_in_place() {
    let Ok((_lock, renderer)) = support::headless_renderer_parts() else {
        return;
    };
    let mut harness = DeepHarness::new(renderer, 40);
    for frame in 0..5 {
        harness
            .frame(frame)
            .unwrap_or_else(|error| panic!("frame {frame} of forty nested layers failed: {error}"));
        let stats = harness
            .shell
            .renderer()
            .last_frame_stats()
            .expect("frame stats");
        assert_eq!(
            stats.isolated_layer_renders, 0,
            "frame {frame}: levels that only turn draw straight into the page, not each into a \
             surface of its own that the next frame's relayout throws away: {stats:?}"
        );
        assert!(
            stats.pass_count <= IN_PLACE_MAX_PASSES,
            "frame {frame} drew forty levels in {} passes: {stats:?}",
            stats.pass_count
        );
    }
}

#[test]
fn layers_nested_past_the_resolve_depth_still_draw_the_frame() {
    let spawned = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let Ok((_lock, renderer)) = support::headless_renderer_parts() else {
                return None;
            };
            let mut harness = DeepHarness::new(renderer, DEPTH_PAST_LIMIT);
            Some(harness.frame(0))
        })
        .expect("spawn a thread with room for deep recursion");
    let Some(captured) = spawned.join().expect("the deep frame must not crash") else {
        return;
    };
    let captured = captured
        .unwrap_or_else(|error| panic!("a layer nested too deep failed the frame: {error}"));
    let drawn = captured
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| is_level_background(pixel))
        .count();
    assert!(
        drawn > 1000,
        "the levels above the resolve depth must still draw"
    );
}
