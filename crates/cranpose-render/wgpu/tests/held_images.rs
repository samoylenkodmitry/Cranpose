//! A pass holds src-over images beside the held glyphs while the shapes
//! after them fill one arena chunk: a list of cards draws in a number of
//! calls that grows only by each card's image draw. Held images keep their
//! order against every draw they share a pixel with.

use std::sync::atomic::{AtomicUsize, Ordering};

use cranpose_render_common::graph::{DrawRunNode, PrimitivePhase, RenderGraph, RenderNode};
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot};
use cranpose_ui::{
    Alignment, ContentScale, Image,
    text::{SpanStyle, TextUnit},
};
use cranpose_ui_graphics::{BlendMode, Brush, DrawPrimitive, ImageBitmap, ImageSampling, Rect};
use support::page::*;

use crate::support;

const FRAME_WIDTH: u32 = 480;
const FRAME_HEIGHT: u32 = 240;
const CARD: [f32; 2] = [96.0, 84.0];
const GAP: f32 = 24.0;
const COLUMNS: usize = 4;
const PAGE: Color = Color(0.86, 0.88, 0.9, 1.0);
const INK: Color = Color(0.1, 0.12, 0.2, 1.0);
const COVER: Color = Color(0.0, 0.0, 1.0, 1.0);
const CHIP: Color = Color(0.85, 0.93, 0.86, 1.0);
const NO_INTERIORS_FIRST: &str = "CRANPOSE_NO_INTERIORS_FIRST";
/// The draws a list of cards takes past those of their images: the page
/// and cards' shapes, their opaque interiors and their glyphs.
const LIST_DRAWS: u32 = 3;

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

fn solid_image(rgba: [u8; 4]) -> ImageBitmap {
    ImageBitmap::from_rgba8(2, 2, rgba.repeat(4)).expect("a solid image")
}

#[composable]
fn Avatar(rect: [f32; 4], rgba: [u8; 4]) {
    Image(
        solid_image(rgba),
        None,
        rect_modifier(rect),
        Alignment::CENTER,
        ContentScale::FillBounds,
        1.0,
        None,
    );
}

/// A card as a list shows it: its background, an avatar, a title and a chip.
#[composable]
fn Card(index: usize) {
    let [x, y] = card_origin(index);
    Box(
        rect_modifier([x, y, CARD[0], CARD[1]]).background(Color::WHITE),
        BoxSpec::default(),
        move || {
            Avatar([6.0, 6.0, 14.0, 14.0], [200, 60, 40, 255]);
            Text(
                format!("card {index}"),
                rect_modifier([26.0, 6.0, 64.0, 14.0]),
                label(INK, 10.0),
            );
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

/// An image with a text drawn over its left part and an opaque cover over
/// its right part.
#[composable]
fn CoveredImagePage() {
    FramePage(FRAME_WIDTH, FRAME_HEIGHT, Color::WHITE, || {
        Avatar([10.0, 10.0, 120.0, 60.0], [255, 0, 0, 255]);
        Text(
            "MMM",
            rect_modifier([12.0, 12.0, 60.0, 40.0]),
            label(Color::WHITE, 24.0),
        );
        Box(
            rect_modifier([80.0, 10.0, 50.0, 60.0]).background(COVER),
            BoxSpec::default(),
            || {},
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

#[test]
fn a_list_of_cards_draws_in_the_calls_its_images_need() {
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
fn a_held_image_keeps_its_place_under_a_text_and_a_shape() {
    for interiors_first in [true, false] {
        let Some((frame, _)) = capture(CoveredImagePage, 0, interiors_first) else {
            return;
        };
        assert_eq!(pixel(&frame, 76, 60), [255, 0, 0, 255], "the image shows");
        assert_eq!(
            pixel(&frame, 100, 40),
            [0, 0, 255, 255],
            "interiors first {interiors_first}: the cover hides the image"
        );
        let text = (12..72)
            .flat_map(|x| (12..52).map(move |y| (x, y)))
            .filter(|(x, y)| pixel(&frame, *x, *y)[1] > 200)
            .count();
        assert!(
            text > 50,
            "interiors first {interiors_first}: the text shows over the image, {text} pixels"
        );
    }
}

#[test]
fn an_image_of_another_blend_mode_draws_after_the_held_image_below_it() {
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
