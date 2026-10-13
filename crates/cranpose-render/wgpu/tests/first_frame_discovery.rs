//! A launch with no record of its first screen walks its first frame once,
//! drawing nothing, so the pipelines all of its passes need compile side by
//! side where the background compiler runs them so (Metal), and then draws
//! it. The frame it draws shows what the frames after it show.

use cranpose_render_common::graph::{ProjectiveTransform, RenderGraph, RenderNode};
use cranpose_ui_graphics::{Color, GraphicsLayer, Rect, Size};

use crate::{shared_test_support, support};

const WIDTH: u32 = 160;
const HEIGHT: u32 = 96;

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// A page with a label, and a tilted, translucent card holding a swatch and
/// a label of its own: the card draws in a pass of its own, which the page's
/// pass then composites.
fn page_with_tilted_card() -> RenderGraph {
    let frame = Rect::from_size(Size::new(WIDTH as f32, HEIGHT as f32));
    let card = shared_test_support::layer_node(
        rect(70.0, 20.0, 80.0, 60.0),
        ProjectiveTransform::identity(),
        GraphicsLayer {
            alpha: 0.7,
            rotation_z: 12.0,
            ..GraphicsLayer::default()
        },
        vec![
            support::solid_rect(rect(0.0, 0.0, 80.0, 60.0), Color(0.2, 0.4, 0.9, 1.0)),
            support::solid_rect(rect(8.0, 8.0, 24.0, 24.0), Color(0.9, 0.8, 0.1, 1.0)),
            support::text_node(9, "Card", rect(8.0, 36.0, 64.0, 20.0), 14.0),
        ],
    );
    let nodes: Vec<RenderNode> = vec![
        support::solid_rect(frame, Color::BLACK),
        support::solid_rect(rect(6.0, 6.0, 50.0, 50.0), Color(0.9, 0.2, 0.2, 1.0)),
        support::text_node(7, "Page", rect(6.0, 62.0, 60.0, 24.0), 16.0),
        RenderNode::Layer(Box::new(card)),
    ];
    RenderGraph::new(shared_test_support::layer_node(
        frame,
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        nodes,
    ))
}

#[test]
fn a_first_frame_drawn_after_learning_its_pipelines_shows_what_later_frames_show() {
    let Ok(mut renderer) = support::headless_renderer_compiling_in_background() else {
        return;
    };
    let first = support::capture_graph(&mut renderer, page_with_tilted_card(), WIDTH, HEIGHT);
    let later = support::capture_graph(&mut renderer, page_with_tilted_card(), WIDTH, HEIGHT);
    assert!(
        support::distinct_colors(&first.pixels) > 4,
        "the page, its label and the card show"
    );
    let differing = support::pixels_differing_beyond(WIDTH, &first.pixels, &later.pixels, 2);
    assert!(
        differing.is_empty(),
        "the first frame differs from the next: {}",
        support::describe_differing(&differing)
    );
}
