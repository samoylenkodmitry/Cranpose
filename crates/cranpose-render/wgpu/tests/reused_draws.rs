use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui::{
    Color, Modifier, Size, TextStyle, composable,
    widgets::{Box, BoxSpec, Column, ColumnSpec, Text},
};

use crate::support;

const WIDTH: u32 = 200;
const HEIGHT: u32 = 240;
const ROWS: [Color; 2] = [Color(0.92, 0.94, 0.98, 1.0), Color(0.86, 0.90, 0.96, 1.0)];
const PANEL: Color = Color(0.20, 0.45, 0.85, 1.0);

/// Six rows that never change over a panel whose label and offset do.
#[composable]
fn Page(label: MutableState<u32>, shift: MutableState<f32>) {
    support::FramePage(WIDTH, HEIGHT, Color::WHITE, move || {
        Column(
            Modifier::empty().fill_max_size().padding(6.0),
            ColumnSpec::default(),
            move || {
                for row in 0..6 {
                    Box(
                        Modifier::empty()
                            .fill_max_width()
                            .height(22.0)
                            .background(ROWS[row % 2]),
                        BoxSpec::default(),
                        move || {
                            Text(
                                format!("row {row}"),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                        },
                    );
                }
                Box(
                    Modifier::empty()
                        .offset(shift.get(), 0.0)
                        .size(Size {
                            width: 120.0,
                            height: 30.0,
                        })
                        .background(PANEL),
                    BoxSpec::default(),
                    move || {
                        Text(
                            format!("count {}", label.get()),
                            Modifier::empty(),
                            TextStyle::default(),
                        );
                    },
                );
            },
        );
    });
}

#[derive(Clone, Copy)]
struct PageStates {
    label: MutableState<u32>,
    shift: MutableState<f32>,
}

struct Harness {
    shell: AppShell<WgpuRenderer>,
    states: Rc<RefCell<Option<PageStates>>>,
}

impl Harness {
    fn new(renderer: WgpuRenderer, label: u32, shift: f32) -> Self {
        let states = Rc::new(RefCell::new(None));
        let states_for_page = Rc::clone(&states);
        let mut shell = AppShell::new(
            renderer,
            location_key(file!(), line!(), column!()),
            move || {
                let label = rememberMutableStateOf(move || label);
                let shift = rememberMutableStateOf(move || shift);
                *states_for_page.borrow_mut() = Some(PageStates { label, shift });
                Page(label, shift);
            },
        );
        shell.set_viewport(WIDTH as f32, HEIGHT as f32);
        shell.set_buffer_size(WIDTH, HEIGHT);
        shell.update();
        Self { shell, states }
    }

    fn set(&mut self, label: u32, shift: f32) {
        let states = self
            .states
            .borrow()
            .as_ref()
            .copied()
            .expect("the page holds its states");
        self.shell.debug_enter_app_context(|| {
            states.label.set(label);
            states.shift.set(shift);
        });
    }

    fn frame(&mut self) -> (RenderStatsSnapshot, CapturedFrame) {
        support::update_and_capture(&mut self.shell, WIDTH, HEIGHT)
    }
}

#[test]
fn reused_draws_follow_a_changed_label_and_a_moved_panel() {
    let (_lock, renderer) = match support::headless_renderer_parts() {
        Ok(parts) => parts,
        Err(error) => {
            eprintln!("skipping reused draw check: {error}");
            return;
        }
    };
    let mut harness = Harness::new(renderer, 0, 0.0);
    let mut reused = 0;
    for _ in 0..6 {
        reused = harness.frame().0.reused_draw_segments;
    }
    assert!(
        reused > 0,
        "rows that stood unchanged for frames must append their earlier draws"
    );

    for (label, shift) in [(7, 0.0), (7, 23.5), (8, 23.5)] {
        harness.set(label, shift);
        let (_, frame) = harness.frame();
        let mut fresh = Harness::new(
            support::create_headless_renderer().expect("a second headless renderer"),
            label,
            shift,
        );
        let (_, expected) = fresh.frame();
        support::assert_same_bytes(
            &format!("label {label} at shift {shift}"),
            WIDTH,
            &frame.pixels,
            &expected.pixels,
        );
    }
}
