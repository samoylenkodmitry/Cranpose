use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key};
use cranpose_render_wgpu::{CapturedFrame, WgpuRenderer};
use cranpose_ui::{
    Color, LinearArrangement, Modifier, TextStyle, composable,
    widgets::{Box, BoxSpec, Column, ColumnSpec, Text},
};

use crate::support;

const FRAME_WIDTH: u32 = 240;
const FRAME_HEIGHT: u32 = 320;
const FRAMES: usize = 14;
// The frames at which the card's own content changes, while the strip below
// it moves on every frame.
const CARD_CHANGES: [usize; 2] = [6, 10];

const SHADES: [Color; 3] = [
    Color(0.85, 0.25, 0.30, 1.0),
    Color(0.20, 0.55, 0.85, 1.0),
    Color(0.25, 0.70, 0.40, 1.0),
];

#[derive(Clone, Copy)]
struct Inputs {
    seconds: MutableState<f32>,
    version: MutableState<usize>,
}

#[composable]
fn Card(version: MutableState<usize>) {
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(140.0)
            .padding(16.0)
            .graphics_layer_block(|layer| {
                layer.rotation_z = 4.0;
                layer.alpha = 0.7;
            })
            .background(Color(0.95, 0.95, 0.90, 1.0))
            .rounded_corners(10.0),
        BoxSpec::new(),
        move || {
            let shown = version.get();
            Column(
                Modifier::empty().padding(8.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
                move || {
                    Text(
                        format!("card {shown}"),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                    Box(
                        Modifier::empty()
                            .size_points(120.0, 30.0)
                            .background(SHADES[shown % SHADES.len()]),
                        BoxSpec::new(),
                        || {},
                    );
                },
            );
        },
    );
}

#[composable]
fn Strip(seconds: MutableState<f32>) {
    Box(
        Modifier::empty()
            .size_points(60.0, 60.0)
            .graphics_layer_block(move |layer| {
                layer.translation_x = (seconds.get() * 300.0) % 160.0;
            })
            .background(Color(0.30, 0.30, 0.75, 1.0)),
        BoxSpec::new(),
        || {},
    );
}

#[composable]
fn Screen(seconds: MutableState<f32>, version: MutableState<usize>) {
    Column(
        Modifier::empty().fill_max_size().padding(8.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(12.0)),
        move || {
            Card(version);
            Strip(seconds);
        },
    );
}

struct Harness {
    shell: AppShell<WgpuRenderer>,
    inputs: Rc<RefCell<Option<Inputs>>>,
}

// The card's content version at frame `frame`.
fn version_at(frame: usize) -> usize {
    CARD_CHANGES.iter().filter(|&&at| at <= frame).count()
}

fn seconds_at(frame: usize) -> f32 {
    frame as f32 / 60.0
}

impl Harness {
    // A harness whose first frame shows frame `first` of the run.
    fn new(renderer: WgpuRenderer, first: usize) -> Self {
        let root_key = location_key(file!(), line!(), column!());
        let inputs: Rc<RefCell<Option<Inputs>>> = Rc::new(RefCell::new(None));
        let inputs_for_app = Rc::clone(&inputs);
        let mut shell = AppShell::new(renderer, root_key, move || {
            let seconds = cranpose_core::rememberMutableStateOf(|| seconds_at(first));
            let version = cranpose_core::rememberMutableStateOf(|| version_at(first));
            *inputs_for_app.borrow_mut() = Some(Inputs { seconds, version });
            Screen(seconds, version);
        });
        shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
        shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
        Self { shell, inputs }
    }

    fn frame(&mut self, frame: usize) -> CapturedFrame {
        let inputs = self.inputs.borrow().expect("inputs captured");
        support::settle(|| {
            self.shell.debug_enter_app_context(|| {
                inputs.seconds.set(seconds_at(frame));
                inputs.version.set(version_at(frame));
            });
            support::update_and_capture(&mut self.shell, FRAME_WIDTH, FRAME_HEIGHT)
        })
    }
}

fn renderer() -> Option<(std::sync::MutexGuard<'static, ()>, WgpuRenderer)> {
    support::headless_renderer_parts()
        .inspect_err(|err| eprintln!("skipping (headless WGPU init failed): {err}"))
        .ok()
}

// Frame `frame` drawn by a renderer that drew nothing before it.
fn first_frame(frame: usize) -> CapturedFrame {
    let (_lock, renderer) = renderer().expect("a renderer was made before");
    let mut harness = Harness::new(renderer, frame);
    support::settle(|| support::update_and_capture(&mut harness.shell, FRAME_WIDTH, FRAME_HEIGHT))
}

#[test]
fn a_still_isolated_layer_draws_what_a_renderer_with_no_history_draws() {
    let Some((lock, renderer)) = renderer() else {
        return;
    };
    let mut harness = Harness::new(renderer, 0);
    let run: Vec<CapturedFrame> = (0..FRAMES).map(|frame| harness.frame(frame)).collect();
    drop(harness);
    drop(lock);
    for (frame, drawn) in run.iter().enumerate() {
        assert!(
            drawn.pixels == first_frame(frame).pixels,
            "frame {frame}: a turned card that holds still while the strip moves, and that \
             changes at frames {CARD_CHANGES:?}, must draw what a new renderer draws"
        );
    }
}
