//! Cards composited through surfaces of their own render side by side in
//! one atlas pass under one binding: their shapes, glyphs and images batch
//! across the cards, and each card draws what it draws alone.

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key};
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui::{
    Box, BoxSpec, Color, CompositingStrategy, GraphicsLayer, Modifier, Text, TextStyle, composable,
    text::{SpanStyle, TextUnit},
};
use cranpose_ui_graphics::{Brush, CornerRadii, ImageBitmap, Rect, Stroke};

use crate::support;

/// The Mate 20 X's device pixels per point: a 0.75 point stroke puts edges
/// at odd eighths of a point, which land halfway between sixteenths of a
/// device pixel, where the snapped placements of a card round.
const DENSITY: f32 = 2.75;
const WIDTH_POINTS: f32 = 192.0;
const HEIGHT_POINTS: f32 = 80.0;
const WIDTH: u32 = 528;
const HEIGHT: u32 = 220;
const CARDS: usize = 4;
/// The points each card owns across the page: it and all it draws past its
/// own bounds stay inside.
const COLUMN_POINTS: f32 = WIDTH_POINTS / CARDS as f32;
const COLUMN: usize = WIDTH as usize / CARDS;
const PAGE: Color = Color(1.0, 1.0, 1.0, 1.0);

fn checker() -> ImageBitmap {
    let side = 8;
    let pixels = (0..side * side)
        .flat_map(|index| {
            if (index % side + index / side) % 2 == 0 {
                [240, 200, 60, 255]
            } else {
                [30, 80, 170, 255]
            }
        })
        .collect();
    ImageBitmap::from_rgba8(side, side, pixels).expect("checker image")
}

/// The cards in `shown`, each in its column at a fraction of a point from
/// the column's edge: a background to its edges, a 0.75 point border on
/// odd eighths, an image, a clipped box whose content reaches past its
/// clip, a label, and a tab hanging past its left edge. The image spans 32
/// device pixels for its 8 texels, so no texel edge lands on a pixel
/// centre, where a nearest sample drawn at another place in a target
/// rounds either way.
#[composable]
fn Cards(shown: [bool; CARDS], label: MutableState<u32>) {
    Box(
        Modifier::empty()
            .size_points(WIDTH_POINTS, HEIGHT_POINTS)
            .background(PAGE),
        BoxSpec::default(),
        move || {
            for (index, _) in shown.iter().enumerate().filter(|(_, shown)| **shown) {
                let shade = 0.2 + 0.15 * index as f32;
                Box(
                    Modifier::empty()
                        .offset(COLUMN_POINTS * index as f32 + 6.3, 10.125)
                        .size_points(34.0, 56.0)
                        .graphics_layer_value(GraphicsLayer {
                            compositing_strategy: CompositingStrategy::Offscreen,
                            ..Default::default()
                        })
                        .draw_behind(move |scope| {
                            let size = scope.size();
                            scope.draw_rect(Brush::solid(Color(shade, 0.5, 0.7, 1.0)));
                            scope.draw_round_rect_at_stroked(
                                Rect {
                                    x: 2.125,
                                    y: 3.375,
                                    width: size.width - 4.5,
                                    height: 20.625,
                                },
                                Brush::solid(Color(0.95, 0.95, 0.9, 1.0)),
                                CornerRadii::uniform(3.0),
                                Stroke::new(0.75),
                            );
                            scope.draw_image_at(
                                Rect {
                                    x: 4.25,
                                    y: 28.0,
                                    width: 32.0 / DENSITY,
                                    height: 32.0 / DENSITY,
                                },
                                checker(),
                                1.0,
                                None,
                            );
                        }),
                    BoxSpec::default(),
                    move || {
                        Box(
                            Modifier::empty()
                                .offset(18.375, 26.625)
                                .size_points(12.0, 14.0)
                                .clip_to_bounds()
                                .draw_behind(|scope| {
                                    scope.draw_rect_at(
                                        Rect {
                                            x: -3.0,
                                            y: 4.0,
                                            width: 20.0,
                                            height: 5.0,
                                        },
                                        Brush::solid(Color(0.9, 0.3, 0.2, 1.0)),
                                    );
                                }),
                            BoxSpec::default(),
                            || {},
                        );
                        Box(
                            Modifier::empty()
                                .offset(-4.25, 44.0)
                                .size_points(9.0, 6.0)
                                .background(Color(0.1, 0.1, 0.1, 1.0)),
                            BoxSpec::default(),
                            || {},
                        );
                        Text(
                            format!("{}{}", index, label.get()),
                            Modifier::empty().offset(4.0, 5.0),
                            TextStyle::from_span_style(SpanStyle {
                                color: Some(Color(0.05, 0.05, 0.1, 1.0)),
                                font_size: TextUnit::Sp(11.0),
                                ..Default::default()
                            }),
                        );
                    },
                );
            }
        },
    );
}

/// Renders the cards in `shown`, settles their pipelines, then relabels
/// them so the measured frame renders their surfaces afresh.
fn relabeled_frame(
    renderer: WgpuRenderer,
    shown: [bool; CARDS],
) -> (RenderStatsSnapshot, CapturedFrame) {
    let label: Rc<RefCell<Option<MutableState<u32>>>> = Rc::new(RefCell::new(None));
    let label_for_app = Rc::clone(&label);
    let mut shell = AppShell::new_with_size_and_density(
        renderer,
        location_key(file!(), line!(), column!()),
        move || {
            let state = cranpose_core::rememberMutableStateOf(|| 0u32);
            *label_for_app.borrow_mut() = Some(state);
            Cards(shown, state);
        },
        (WIDTH, HEIGHT),
        (WIDTH_POINTS, HEIGHT_POINTS),
        DENSITY,
    );
    support::settle(|| support::update_and_capture(&mut shell, WIDTH, HEIGHT));
    support::wait_for_background_compiler_idle();
    let label = label.borrow().as_ref().copied().expect("label captured");
    shell.debug_enter_app_context(|| label.set(1));
    support::update_and_capture(&mut shell, WIDTH, HEIGHT)
}

fn column(frame: &CapturedFrame, index: usize) -> Vec<u8> {
    let row_bytes = WIDTH as usize * 4;
    frame
        .pixels
        .chunks_exact(row_bytes)
        .flat_map(|row| {
            row[index * COLUMN * 4..(index + 1) * COLUMN * 4]
                .iter()
                .copied()
        })
        .collect()
}

#[test]
fn cards_side_by_side_in_an_atlas_batch_together_and_draw_as_they_do_alone() {
    let Ok((_lock, renderer)) = support::headless_renderer_parts() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let (all_stats, all) = relabeled_frame(renderer, [true; CARDS]);
    assert!(
        support::pipelines_settled(&all_stats),
        "the measured frame drew with stand-in pipelines: {all_stats:?}"
    );
    assert_eq!(all_stats.isolated_layer_renders, CARDS as u32);
    for index in 0..CARDS {
        let mut shown = [false; CARDS];
        shown[index] = true;
        let (alone_stats, alone) = relabeled_frame(
            support::headless_renderer_beside_locked().expect("reference renderer"),
            shown,
        );
        assert_eq!(alone_stats.isolated_layer_renders, 1);
        assert!(
            all_stats.shape_passes <= alone_stats.shape_passes
                && all_stats.text_passes <= alone_stats.text_passes,
            "the cards' shapes and labels draw in no more batches than one card's: \
             {all_stats:?} against {alone_stats:?}"
        );
        support::assert_same_bytes(
            &format!("card {index}"),
            COLUMN as u32,
            &column(&alone, index),
            &column(&all, index),
        );
    }
}
