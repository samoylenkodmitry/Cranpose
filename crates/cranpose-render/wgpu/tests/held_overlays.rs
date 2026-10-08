//! A pass holds glyphs and stored runs back while the shapes after them
//! fill one arena chunk: a list of cards draws in a number of calls that
//! grows only by the draws of each card's stored run. Held draws keep their
//! order against every draw they share a pixel with, and in a pass with a
//! depth buffer their place in the pass's order.

use std::sync::atomic::{AtomicUsize, Ordering};

use cranpose_render_common::{
    graph::{DrawCommandId, DrawRunNode, PrimitivePhase, RenderGraph, RenderNode},
    style_shared::DrawPlacement,
};
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot};
use cranpose_ui::{
    Canvas,
    text::{SpanStyle, TextUnit},
};
use cranpose_ui_graphics::{
    BlendMode, Brush, DrawPrimitive, DrawScope, ImageBitmap, ImageSampling, Point, Rect, Stroke,
};
use support::page::*;

use crate::support;

const FRAME_WIDTH: u32 = 480;
const FRAME_HEIGHT: u32 = 240;
const CARD: [f32; 2] = [96.0, 84.0];
const GAP: f32 = 24.0;
const COLUMNS: usize = 4;
const PAGE: Color = Color(0.86, 0.88, 0.9, 1.0);
const INK: Color = Color(0.1, 0.12, 0.2, 1.0);
const LINE: Color = Color(0.0, 1.0, 0.0, 1.0);
const COVER: Color = Color(0.0, 0.0, 1.0, 1.0);
const CHIP: Color = Color(0.85, 0.93, 0.86, 1.0);
const LINE_SEGMENTS: usize = 70;
const NO_INTERIORS_FIRST: &str = "CRANPOSE_NO_INTERIORS_FIRST";
/// The draws a list of cards takes past those of their stored runs: the
/// page and cards' shapes, their opaque interiors and their glyphs.
const LIST_DRAWS: u32 = 4;

/// How many cards [`CardsPage`] composes.
static CARDS: AtomicUsize = AtomicUsize::new(0);

fn card_origin(index: usize) -> [f32; 2] {
    [
        GAP + (index % COLUMNS) as f32 * (CARD[0] + GAP),
        GAP + (index / COLUMNS) as f32 * (CARD[1] + GAP),
    ]
}

fn label(color: Color, size: f32) -> TextStyle {
    TextStyle::from_span_style(SpanStyle {
        color: Some(color),
        font_size: TextUnit::Sp(size),
        ..Default::default()
    })
}

/// A chart of `LINE_SEGMENTS` line records, enough for the run store to keep
/// them.
fn chart(scope: &mut dyn DrawScope, seed: usize) {
    let size = scope.size();
    let step = size.width / LINE_SEGMENTS as f32;
    let point = |at: usize| Point {
        x: at as f32 * step,
        y: size.height * (0.5 + 0.4 * ((at + seed) as f32 * 0.7).sin()),
    };
    for at in 0..LINE_SEGMENTS {
        scope.draw_line(
            Brush::solid(LINE),
            point(at),
            point(at + 1),
            Stroke::new(1.5),
        );
    }
}

/// A card as a list shows it: its background, a title, a chart and a chip.
#[composable]
fn Card(index: usize) {
    let [x, y] = card_origin(index);
    Box(
        rect_modifier([x, y, CARD[0], CARD[1]]).background(Color::WHITE),
        BoxSpec::default(),
        move || {
            Text(
                format!("card {index}"),
                rect_modifier([6.0, 6.0, 84.0, 14.0]),
                label(INK, 10.0),
            );
            Canvas(rect_modifier([6.0, 26.0, 84.0, 30.0]), move |scope| {
                chart(scope, index);
            });
            Box(
                rect_modifier([6.0, 62.0, 36.0, 16.0]).background(CHIP),
                BoxSpec::default(),
                || {
                    Text("tag", Modifier::empty(), label(INK, 10.0));
                },
            );
        },
    );
}

#[composable]
fn CardsPage() {
    FramePage(FRAME_WIDTH, FRAME_HEIGHT, PAGE, || {
        for index in 0..CARDS.load(Ordering::Relaxed) {
            Card(index);
        }
    });
}

/// A box, a chart kept in the run store, a text of more glyphs than a
/// retained glyph run needs beside it, and an opaque cover over the right
/// part of each, the text's first.
#[composable]
fn CoveredPage() {
    FramePage(FRAME_WIDTH, FRAME_HEIGHT, Color::WHITE, || {
        Box(
            rect_modifier([10.0, 10.0, 40.0, 40.0]).background(CHIP),
            BoxSpec::default(),
            || {},
        );
        Canvas(rect_modifier([60.0, 10.0, 80.0, 40.0]), |scope| {
            let step = 80.0 / LINE_SEGMENTS as f32;
            for at in 0..LINE_SEGMENTS {
                scope.draw_line(
                    Brush::solid(LINE),
                    Point::new(at as f32 * step, 20.0),
                    Point::new((at + 1) as f32 * step, 20.0),
                    Stroke::new(10.0),
                );
            }
        });
        Text(
            "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM",
            rect_modifier([150.0, 10.0, 90.0, 120.0]),
            label(Color::BLACK, 14.0),
        );
        Box(
            rect_modifier([180.0, 10.0, 60.0, 120.0]).background(COVER),
            BoxSpec::default(),
            || {},
        );
        Box(
            rect_modifier([110.0, 10.0, 30.0, 40.0]).background(COVER),
            BoxSpec::default(),
            || {},
        );
    });
}

/// A chart kept in the run store whose first record is a large opaque fill,
/// and a text of more glyphs than a retained glyph run needs drawn over it.
#[composable]
fn TextOverChartPage() {
    FramePage(FRAME_WIDTH, FRAME_HEIGHT, Color::WHITE, || {
        Canvas(rect_modifier([10.0, 10.0, 300.0, 120.0]), |scope| {
            let size = scope.size();
            scope.draw_rect_at(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: size.width,
                    height: size.height,
                },
                Brush::solid(COVER),
            );
            chart(scope, 0);
        });
        Text(
            "The quick brown fox jumps over the lazy dog, then over the lazy dog again, \
             and once more over the lazy dog before the fox runs home to its den.",
            rect_modifier([14.0, 14.0, 290.0, 110.0]),
            label(Color::WHITE, 12.0),
        );
    });
}

/// `page` captured warm with `cards` cards, its opaque interiors laid down
/// first or not.
fn capture(
    page: fn(),
    cards: usize,
    interiors_first: bool,
) -> Option<(CapturedFrame, RenderStatsSnapshot)> {
    let (_lock, mut shell) = support::app_shell_for(
        page,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        |_| {
            CARDS.store(cards, Ordering::Relaxed);
            cranpose_render_wgpu::set_debug_toggle(
                NO_INTERIORS_FIRST,
                (!interiors_first).then_some("1"),
            );
        },
    )?;
    let captured = support::warm_shell_frame(&mut shell, FRAME_WIDTH, FRAME_HEIGHT);
    cranpose_render_wgpu::set_debug_toggle(NO_INTERIORS_FIRST, None);
    Some(captured)
}

fn pixel(frame: &CapturedFrame, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * frame.width + x) * 4) as usize;
    [
        frame.pixels[at],
        frame.pixels[at + 1],
        frame.pixels[at + 2],
        frame.pixels[at + 3],
    ]
}

fn luminance(pixel: [u8; 4]) -> u32 {
    u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2])
}

#[test]
fn a_list_of_cards_draws_in_the_calls_its_stored_runs_need() {
    let Some((first, _)) = capture(CardsPage, 8, true) else {
        return;
    };
    for interiors_first in [true, false] {
        for cards in [2, 8] {
            let (_, stats) = capture(CardsPage, cards, interiors_first).expect("headless WGPU");
            assert_eq!(stats.isolated_layer_renders, 0, "the cards draw in place");
            assert!(
                stats.draw_calls <= LIST_DRAWS + cards as u32,
                "{cards} cards, interiors first {interiors_first}: {} draws",
                stats.draw_calls
            );
        }
    }
    let (without, _) = capture(CardsPage, 8, false).expect("headless WGPU");
    let differing = support::differing_pixels(FRAME_WIDTH, &first.pixels, &without.pixels);
    assert!(
        differing.is_empty(),
        "the pre-pass changed pixels: {}",
        support::describe_differing(&differing)
    );
}

#[test]
fn a_shape_drawn_over_a_held_draw_covers_it() {
    for interiors_first in [true, false] {
        let Some((frame, _)) = capture(CoveredPage, 0, interiors_first) else {
            return;
        };
        let blue = [0, 0, 255, 255];
        assert_eq!(
            pixel(&frame, 75, 30),
            [0, 255, 0, 255],
            "the chart shows left of its cover"
        );
        assert_eq!(pixel(&frame, 125, 30), blue, "the cover hides the chart");
        assert_eq!(pixel(&frame, 200, 30), blue, "the cover hides the text");
        assert!(
            (150..180).any(|x| (10..50).any(|y| luminance(pixel(&frame, x, y)) < 200)),
            "the text shows left of its cover"
        );
        assert!(
            (182..238).all(|x| (12..128).all(|y| pixel(&frame, x, y) == blue)),
            "interiors first {interiors_first}: the cover hides every glyph under it"
        );
    }
}

#[test]
fn a_long_text_over_a_held_chart_draws_above_its_fill() {
    for interiors_first in [true, false] {
        let Some((frame, _)) = capture(TextOverChartPage, 0, interiors_first) else {
            return;
        };
        assert_eq!(
            pixel(&frame, 20, 120),
            [0, 0, 255, 255],
            "the chart's fill shows below the text"
        );
        let text = (14..304)
            .flat_map(|x| (14..60).map(move |y| (x, y)))
            .filter(|(x, y)| luminance(pixel(&frame, *x, *y)) > 400)
            .count();
        assert!(
            text > 200,
            "interiors first {interiors_first}: the text shows over the fill, {text} pixels"
        );
    }
}

#[test]
fn an_image_of_another_blend_mode_draws_after_the_held_run_below_it() {
    const SIZE: u32 = 120;
    let rect = |x: f32, y: f32, width: f32, height: f32| Rect {
        x,
        y,
        width,
        height,
    };
    // A large opaque fill, which a pass lays down first, and small ones
    // after it: enough records for the run store to keep the command.
    let mut chart = vec![DrawPrimitive::Rect {
        rect: rect(0.0, 0.0, 100.0, 60.0),
        brush: Brush::solid(Color(1.0, 0.0, 0.0, 1.0)),
        stroke: None,
    }];
    chart.extend((0..70).map(|index| DrawPrimitive::Rect {
        rect: rect(
            (index % 10) as f32 * 10.0,
            70.0 + (index / 10) as f32 * 6.0,
            8.0,
            4.0,
        ),
        brush: Brush::solid(Color(1.0, 0.0, 0.0, 1.0)),
        stroke: None,
    }));
    let cut = DrawPrimitive::Blend {
        blend_mode: BlendMode::DstOut,
        primitive: std::boxed::Box::new(DrawPrimitive::Image {
            rect: rect(20.0, 10.0, 40.0, 30.0),
            image: ImageBitmap::from_rgba8(1, 1, vec![255; 4]).expect("one pixel"),
            alpha: 1.0,
            color_filter: None,
            sampling: ImageSampling::Nearest,
            src_rect: None,
        }),
    };
    let graph = RenderGraph::new(support::layer_node(
        Some(support::STORED_RUN_NODE),
        SIZE as f32,
        SIZE as f32,
        vec![
            RenderNode::DrawRun(DrawRunNode::for_command(
                PrimitivePhase::BeforeChildren,
                Some(DrawCommandId {
                    node_id: support::STORED_RUN_NODE,
                    command_index: 0,
                    placement: DrawPlacement::Behind,
                }),
                chart,
            )),
            RenderNode::DrawRun(DrawRunNode::new(PrimitivePhase::BeforeChildren, vec![cut])),
        ],
    ));
    for interiors_first in [true, false] {
        let Ok(mut renderer) = support::headless_renderer() else {
            return;
        };
        cranpose_render_wgpu::set_debug_toggle(
            NO_INTERIORS_FIRST,
            (!interiors_first).then_some("1"),
        );
        support::capture_graph(&mut renderer, graph.clone(), SIZE, SIZE);
        let frame = support::capture_graph(&mut renderer, graph.clone(), SIZE, SIZE);
        cranpose_render_wgpu::set_debug_toggle(NO_INTERIORS_FIRST, None);
        assert_eq!(pixel(&frame, 80, 30), [255, 0, 0, 255], "the run's fill");
        assert_eq!(
            pixel(&frame, 40, 25)[3],
            0,
            "interiors first {interiors_first}: the dst-out image cuts the run under it"
        );
    }
}
