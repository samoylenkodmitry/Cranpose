mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::{
    graph::{ProjectiveTransform, RenderGraph, RenderNode},
    image_compare::image_difference_stats,
};
use cranpose_ui_graphics::{
    Color, GraphicsLayer, LayerShape, Point, Rect, RenderEffect, RoundedCornerShape, TileMode,
};
use support::{capture_graph, page_graph, region_pixels, solid_rect};

const FRAME: u32 = 200;
const CARD_WIDTH: f32 = 400.0;
const CARD_HEIGHT: f32 = 300.0;
const CARD_BLUR: f32 = 8.0;
const BUTTON_BLUR: f32 = 6.0;
/// How far the crop may reach past the frame: the button's blur reads that
/// far into the card's page, plus a pixel of slack.
const CROP_REACH: u32 = BUTTON_BLUR as u32 + 1;
/// Pixels this far from the frame's edge are compared: nearer ones read
/// past the smaller frame's page, which the whole card's page still holds.
const EDGE: u32 = (CARD_BLUR + BUTTON_BLUR) as u32 + 2;

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// A glass layer: a blurred backdrop under rounded corners, which its
/// full-size content reaches, so the layer is isolated as a card is.
fn glass(blur: f32) -> GraphicsLayer {
    GraphicsLayer {
        backdrop_effect: Some(RenderEffect::blur_with_edge_treatment(
            blur,
            TileMode::Clamp,
        )),
        clip: true,
        shape: LayerShape::Rounded(RoundedCornerShape::new(16.0, 16.0, 16.0, 16.0)),
        ..GraphicsLayer::default()
    }
}

fn glass_layer(bounds: Rect, at: Point, blur: f32, children: Vec<RenderNode>) -> RenderNode {
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::translation(at.x, at.y),
        glass(blur),
        children,
    )))
}

/// A backdrop-reading card of `CARD_WIDTH` x `CARD_HEIGHT` with a glass
/// button inside it, at `card` in a frame of `width` x `height` whose
/// background blocks start at `origin`, so the same pixels land under the
/// card wherever the frame's edge falls.
fn scene(width: u32, height: u32, origin: Point, card: Point) -> RenderGraph {
    let block = |x: f32, y: f32, color: Color| {
        solid_rect(rect(origin.x + x, origin.y + y, 200.0, 150.0), color)
    };
    let button_rect = rect(0.0, 0.0, 90.0, 50.0);
    let button = glass_layer(
        button_rect,
        Point::new(170.0, 130.0),
        BUTTON_BLUR,
        vec![solid_rect(button_rect, Color(1.0, 1.0, 1.0, 0.25))],
    );
    let card_rect = rect(0.0, 0.0, CARD_WIDTH, CARD_HEIGHT);
    let card = glass_layer(
        card_rect,
        card,
        CARD_BLUR,
        vec![solid_rect(card_rect, Color(0.1, 0.1, 0.2, 0.35)), button],
    );
    page_graph(
        width,
        height,
        vec![
            block(0.0, 0.0, Color(0.95, 0.35, 0.20, 1.0)),
            block(200.0, 0.0, Color(0.15, 0.40, 0.90, 1.0)),
            block(0.0, 150.0, Color(0.20, 0.85, 0.35, 1.0)),
            block(200.0, 150.0, Color(0.90, 0.80, 0.15, 1.0)),
            card,
        ],
    )
}

/// A backdrop-reading card that overflows the frame renders only the part
/// of its surface the frame shows, grown by what its glasses read past it,
/// and those pixels match the card rendered whole.
#[test]
fn a_backdrop_reading_card_larger_than_the_frame_renders_its_visible_part() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping viewport crop: {err}");
            return;
        }
    };
    let offset = Point::new(-100.0, -50.0);
    let cropped = capture_graph(
        &mut renderer,
        scene(FRAME, FRAME, offset, offset),
        FRAME,
        FRAME,
    );
    let stats = renderer.last_frame_stats().expect("stats");
    assert_eq!(
        stats.isolated_layer_renders, 2,
        "the card and its button are isolated: {stats:?}"
    );
    let budget = u64::from(FRAME + 2 * CROP_REACH).pow(2) + 90 * 50;
    assert!(
        stats.isolated_layer_pixels <= budget,
        "the card and button surfaces are {} pixels; the frame with the glasses' reach plus the \
         button is {budget}",
        stats.isolated_layer_pixels
    );
    let whole = capture_graph(
        &mut renderer,
        scene(
            CARD_WIDTH as u32,
            CARD_HEIGHT as u32,
            Point::default(),
            Point::default(),
        ),
        CARD_WIDTH as u32,
        CARD_HEIGHT as u32,
    );
    assert!(
        support::distinct_colors(&cropped.pixels) > 8,
        "the card must show its content"
    );
    let side = (FRAME - 2 * EDGE) as f32;
    let inner = region_pixels(&cropped, rect(EDGE as f32, EDGE as f32, side, side));
    let reference = region_pixels(
        &whole,
        rect(-offset.x + EDGE as f32, -offset.y + EDGE as f32, side, side),
    );
    let difference = image_difference_stats(&inner, &reference, side as u32, side as u32, 0);
    assert_eq!(
        difference.differing_pixels, 0,
        "the cropped card differs from the whole card: differing={} max={} first={:?}",
        difference.differing_pixels, difference.max_difference, difference.first_difference
    );
}
