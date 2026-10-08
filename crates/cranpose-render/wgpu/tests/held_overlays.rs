//! A pass holds images, glyphs and stored runs back while the shapes after
//! them fill one arena chunk, and draws the round rect shadows of cards set
//! apart as one batch below them: a list of cards draws in a number of calls
//! that grows only by what each card's image and stored run need. Held and
//! joined draws keep their order against every draw they share a pixel
//! with.

use std::sync::atomic::{AtomicUsize, Ordering};

use cranpose_render_common::graph::{DrawRunNode, PrimitivePhase, RenderGraph, RenderNode};
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot};
use cranpose_ui::{
    Alignment, Canvas, ContentScale, Image,
    text::{SpanStyle, TextUnit},
};
use cranpose_ui_graphics::{
    BlendMode, Brush, DrawPrimitive, DrawScope, ImageBitmap, ImageSampling, LayerShape, Point,
    Rect, Stroke,
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
/// The draws a card adds: one for its image, one for its stored run.
const CARD_DRAWS: u32 = 2;
/// The draws of a list of cards past theirs: the page and cards' shapes,
/// their opaque interiors, their glyphs and their shadows.
const LIST_DRAWS: u32 = 6;

/// How many cards [`CardsPage`] composes.
static CARDS: AtomicUsize = AtomicUsize::new(0);
/// The elevation of the second card [`OverlapPage`] composes.
static ELEVATION: AtomicUsize = AtomicUsize::new(0);

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

fn solid_image(rgba: [u8; 4]) -> ImageBitmap {
    ImageBitmap::from_rgba8(2, 2, rgba.repeat(4)).expect("a solid image")
}

fn shadowed(rect: [f32; 4], elevation: f32) -> Modifier {
    rect_modifier(rect)
        .shadow_with(
            elevation,
            LayerShape::Rectangle,
            false,
            Color::BLACK,
            Color::BLACK,
        )
        .background(Color::WHITE)
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

/// A card as a list shows it: its shadow, its background, an avatar, a
/// title, a chart and a chip.
#[composable]
fn Card(index: usize) {
    let [x, y] = card_origin(index);
    Box(
        shadowed([x, y, CARD[0], CARD[1]], 6.0),
        BoxSpec::default(),
        move || {
            Image(
                solid_image([200, 60, 40, 255]),
                None,
                rect_modifier([6.0, 6.0, 14.0, 14.0]),
                Alignment::CENTER,
                ContentScale::FillBounds,
                1.0,
                None,
            );
            Text(
                format!("card {index}"),
                rect_modifier([26.0, 6.0, 64.0, 14.0]),
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

/// An image, a chart kept in the run store and a text, each with an opaque
/// cover drawn after it over its right part.
#[composable]
fn CoveredPage() {
    FramePage(FRAME_WIDTH, FRAME_HEIGHT, Color::WHITE, || {
        Image(
            solid_image([255, 0, 0, 255]),
            None,
            rect_modifier([10.0, 10.0, 40.0, 40.0]),
            Alignment::CENTER,
            ContentScale::FillBounds,
            1.0,
            None,
        );
        Box(
            rect_modifier([30.0, 10.0, 20.0, 40.0]).background(COVER),
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
        Box(
            rect_modifier([110.0, 10.0, 30.0, 40.0]).background(COVER),
            BoxSpec::default(),
            || {},
        );
        Text(
            "MMMM",
            rect_modifier([150.0, 10.0, 90.0, 40.0]),
            label(Color::BLACK, 28.0),
        );
        Box(
            rect_modifier([180.0, 10.0, 60.0, 40.0]).background(COVER),
            BoxSpec::default(),
            || {},
        );
    });
}

/// A card with a shadow, a card without one, and below it a card whose
/// shadow, at [`ELEVATION`], falls on the card above.
#[composable]
fn OverlapPage() {
    FramePage(FRAME_WIDTH, FRAME_HEIGHT, PAGE, || {
        Box(
            shadowed([20.0, 20.0, 50.0, 40.0], 6.0),
            BoxSpec::default(),
            || {},
        );
        Box(
            rect_modifier([120.0, 20.0, 80.0, 60.0]).background(Color::WHITE),
            BoxSpec::default(),
            || {},
        );
        Box(
            shadowed(
                [120.0, 84.0, 80.0, 60.0],
                ELEVATION.load(Ordering::Relaxed) as f32,
            ),
            BoxSpec::default(),
            || {},
        );
    });
}

/// `page` captured warm with `cards` cards and the second card of
/// [`OverlapPage`] at `elevation`, its opaque interiors laid down first or
/// not.
fn capture(
    page: fn(),
    (cards, elevation): (usize, usize),
    interiors_first: bool,
) -> Option<(CapturedFrame, RenderStatsSnapshot)> {
    let (_lock, mut shell) = support::app_shell_for(
        page,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        |_| {
            CARDS.store(cards, Ordering::Relaxed);
            ELEVATION.store(elevation, Ordering::Relaxed);
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
fn a_list_of_cards_draws_in_the_calls_its_images_and_stored_runs_need() {
    let Some((first, _)) = capture(CardsPage, (8, 0), true) else {
        return;
    };
    for interiors_first in [true, false] {
        for cards in [2, 8] {
            let (frame, stats) =
                capture(CardsPage, (cards, 0), interiors_first).expect("headless WGPU");
            assert_eq!(stats.isolated_layer_renders, 0, "the cards draw in place");
            assert!(
                stats.draw_calls <= LIST_DRAWS + CARD_DRAWS * cards as u32,
                "{cards} cards, interiors first {interiors_first}: {} draws",
                stats.draw_calls
            );
            let page = luminance(pixel(&frame, 2, 2));
            for card in 0..cards {
                let [x, y] = card_origin(card);
                let below = pixel(
                    &frame,
                    (x + CARD[0] / 2.0) as u32,
                    (y + CARD[1] + 2.0) as u32,
                );
                assert!(
                    luminance(below) < page,
                    "card {card}'s shadow shows below it: {below:?}"
                );
                let inside = pixel(&frame, (x + CARD[0] - 3.0) as u32, (y + 3.0) as u32);
                assert_eq!(inside, [255; 4], "card {card} covers its own shadow");
            }
        }
    }
    let (without, _) = capture(CardsPage, (8, 0), false).expect("headless WGPU");
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
        let Some((frame, _)) = capture(CoveredPage, (0, 0), interiors_first) else {
            return;
        };
        let blue = [0, 0, 255, 255];
        let image = pixel(&frame, 20, 30);
        assert!(
            image[0] > 200 && image[1] < 40 && image[2] < 40,
            "the image shows left of its cover: {image:?}"
        );
        assert_eq!(pixel(&frame, 40, 30), blue, "the cover hides the image");
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
    }
}

#[test]
fn a_shadow_over_an_earlier_card_draws_above_it() {
    let probe = (160, 77);
    for interiors_first in [true, false] {
        let Some((flat, _)) = capture(OverlapPage, (0, 0), interiors_first) else {
            return;
        };
        let (raised, _) = capture(OverlapPage, (0, 16), interiors_first).expect("headless WGPU");
        let (flat, raised) = (
            pixel(&flat, probe.0, probe.1),
            pixel(&raised, probe.0, probe.1),
        );
        assert_eq!(flat, [255; 4], "the card above is white at the probe");
        assert!(
            luminance(raised) < luminance(flat),
            "the lower card's shadow falls on the card above it, not below it: {raised:?}"
        );
    }
}

#[test]
fn an_image_of_another_blend_mode_draws_after_the_held_draws_below_it() {
    const SIZE: u32 = 80;
    let image = |x: f32| DrawPrimitive::Image {
        rect: Rect {
            x,
            y: 10.0,
            width: 40.0,
            height: 40.0,
        },
        image: solid_image([255, 0, 0, 255]),
        alpha: 1.0,
        color_filter: None,
        sampling: ImageSampling::Nearest,
        src_rect: None,
    };
    let graph = RenderGraph::new(support::layer_node(
        None,
        SIZE as f32,
        SIZE as f32,
        vec![RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            vec![
                DrawPrimitive::Rect {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: SIZE as f32,
                        height: SIZE as f32,
                    },
                    brush: Brush::solid(Color::WHITE),
                    stroke: None,
                },
                image(10.0),
                DrawPrimitive::Blend {
                    blend_mode: BlendMode::DstOut,
                    primitive: std::boxed::Box::new(image(30.0)),
                },
            ],
        ))],
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
        assert_eq!(
            pixel(&frame, 20, 30),
            [255, 0, 0, 255],
            "the src-over image alone"
        );
        assert_eq!(
            pixel(&frame, 40, 30)[3],
            0,
            "the dst-out image cuts the held image under it"
        );
        assert_eq!(pixel(&frame, 60, 30)[3], 0, "and the page under it");
        assert_eq!(pixel(&frame, 75, 30), [255; 4], "the page beside them");
    }
}
