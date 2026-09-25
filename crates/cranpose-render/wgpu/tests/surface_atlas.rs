use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key};
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui::{
    Alignment, Box, BoxSpec, Color, CompositingStrategy, GraphicsLayer, Modifier, Text, TextStyle,
    composable,
    text::{SpanStyle, TextUnit},
};

use crate::support;

const WIDTH: u32 = 240;
const HEIGHT: u32 = 120;
const HALF: usize = (WIDTH / 2) as usize;
const PAGE: Color = Color(1.0, 1.0, 1.0, 1.0);

/// One of the two cards: where it sits, how it is scaled and turned, and its
/// colour. The cards' scales differ, so their surfaces rasterize at scales of
/// their own.
struct Card {
    x: f32,
    scale: f32,
    degrees: f32,
    color: Color,
}

const CARDS: [Card; 2] = [
    Card {
        x: 20.0,
        scale: 0.9,
        degrees: 20.0,
        color: Color(0.85, 0.25, 0.30, 1.0),
    },
    Card {
        x: 140.0,
        scale: 0.7,
        degrees: -35.0,
        color: Color(0.20, 0.55, 0.85, 1.0),
    },
];

#[composable]
fn Cards(shown: [bool; 2], label: MutableState<u32>) {
    Box(
        Modifier::empty()
            .size_points(WIDTH as f32, HEIGHT as f32)
            .background(PAGE),
        BoxSpec::default(),
        move || {
            for card in CARDS
                .iter()
                .zip(shown)
                .filter_map(|(card, shown)| shown.then_some(card))
            {
                let (scale, degrees, color) = (card.scale, card.degrees, card.color);
                Box(
                    Modifier::empty()
                        .offset(card.x, 20.0)
                        .size_points(80.0, 80.0)
                        .graphics_layer_value(GraphicsLayer {
                            scale_x: scale,
                            scale_y: scale,
                            rotation_z: degrees,
                            compositing_strategy: CompositingStrategy::Offscreen,
                            ..Default::default()
                        })
                        .background(color)
                        .rounded_corners(10.0),
                    BoxSpec::new().content_alignment(Alignment::CENTER),
                    move || {
                        Text(
                            format!("card {}", label.get()),
                            Modifier::empty(),
                            TextStyle::from_span_style(SpanStyle {
                                color: Some(Color::WHITE),
                                font_size: TextUnit::Sp(16.0),
                                ..Default::default()
                            }),
                        );
                    },
                );
            }
        },
    );
}

struct CardsHarness {
    shell: AppShell<WgpuRenderer>,
    label: Rc<RefCell<Option<MutableState<u32>>>>,
}

impl CardsHarness {
    fn new(renderer: WgpuRenderer, shown: [bool; 2]) -> Self {
        let root_key = location_key(file!(), line!(), column!());
        let label: Rc<RefCell<Option<MutableState<u32>>>> = Rc::new(RefCell::new(None));
        let label_for_app = Rc::clone(&label);
        let mut shell = AppShell::new(renderer, root_key, move || {
            let state = cranpose_core::rememberMutableStateOf(|| 0u32);
            *label_for_app.borrow_mut() = Some(state);
            Cards(shown, state);
        });
        shell.set_viewport(WIDTH as f32, HEIGHT as f32);
        shell.set_buffer_size(WIDTH, HEIGHT);
        shell.update();
        Self { shell, label }
    }

    /// Settles the cards' pipelines, then relabels them so the measured
    /// frame renders their surfaces afresh.
    fn relabeled_frame(&mut self) -> (RenderStatsSnapshot, CapturedFrame) {
        support::settle(|| support::update_and_capture(&mut self.shell, WIDTH, HEIGHT));
        support::wait_for_background_compiler_idle();
        let label = self
            .label
            .borrow()
            .as_ref()
            .copied()
            .expect("label captured");
        self.shell.debug_enter_app_context(|| label.set(1));
        support::update_and_capture(&mut self.shell, WIDTH, HEIGHT)
    }
}

fn half(frame: &CapturedFrame, right: bool) -> Vec<u8> {
    let row_bytes = WIDTH as usize * 4;
    let start = if right { HALF * 4 } else { 0 };
    frame
        .pixels
        .chunks_exact(row_bytes)
        .flat_map(|row| row[start..start + HALF * 4].iter().copied())
        .collect()
}

#[test]
fn surfaces_at_different_scales_share_one_atlas_pass_and_draw_as_they_do_alone() {
    let Ok((_lock, renderer)) = support::headless_renderer_parts() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let (both_stats, both) = CardsHarness::new(renderer, [true, true]).relabeled_frame();
    let alone = |shown| {
        CardsHarness::new(
            support::headless_renderer_beside_locked().expect("reference renderer"),
            shown,
        )
        .relabeled_frame()
    };
    let (left_stats, left) = alone([true, false]);
    let (_, right) = alone([false, true]);
    assert!(
        support::pipelines_settled(&both_stats),
        "the measured frame drew with stand-in pipelines: {both_stats:?}"
    );
    assert_eq!(both_stats.isolated_layer_renders, 2);
    assert_eq!(left_stats.isolated_layer_renders, 1);
    assert_eq!(
        both_stats.pass_count, left_stats.pass_count,
        "two surfaces at scales of their own render in one atlas pass, no more passes \
         than one surface alone"
    );
    support::assert_same_bytes(
        "left card",
        WIDTH / 2,
        &half(&left, false),
        &half(&both, false),
    );
    support::assert_same_bytes(
        "right card",
        WIDTH / 2,
        &half(&right, true),
        &half(&both, true),
    );
}
