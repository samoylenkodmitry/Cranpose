//! A launch with no record of its first screen starts building the
//! pipelines every screen draws with, flat solid rects and text on whole
//! pixels, as its renderer starts, while the first screen composes: its
//! first frame finds them built.

use cranpose_render_common::graph::{ProjectiveTransform, RenderGraph, RenderNode};
use cranpose_ui_graphics::{Color, GraphicsLayer, Rect, Size};

use crate::{shared_test_support, support};

const WIDTH: u32 = 160;
const HEIGHT: u32 = 48;

/// A cleared page with `children` on it.
fn page(children: Vec<RenderNode>) -> RenderGraph {
    let frame = Rect::from_size(Size::new(WIDTH as f32, HEIGHT as f32));
    let mut nodes = vec![support::solid_rect(frame, Color::BLACK)];
    nodes.extend(children);
    RenderGraph::new(shared_test_support::layer_node(
        frame,
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        nodes,
    ))
}

/// A card and a label beside it.
fn card_and_label() -> RenderGraph {
    page(vec![
        support::solid_rect(
            Rect {
                x: 4.0,
                y: 4.0,
                width: 40.0,
                height: 40.0,
            },
            Color(0.9, 0.2, 0.2, 1.0),
        ),
        support::text_node(
            7,
            "Cranpose",
            Rect {
                x: 52.0,
                y: 12.0,
                width: 100.0,
                height: 24.0,
            },
            16.0,
        ),
    ])
}

#[test]
fn a_launch_with_no_record_finds_its_rect_and_text_pipelines_built() {
    let Ok(mut renderer) = support::headless_renderer_compiling_in_background() else {
        return;
    };
    // A frame of an empty page builds what every capture draws with.
    let empty = RenderGraph::new(shared_test_support::layer_node(
        Rect::from_size(Size::new(WIDTH as f32, HEIGHT as f32)),
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        Vec::new(),
    ));
    support::capture_graph(&mut renderer, empty, WIDTH, HEIGHT);
    support::wait_for_background_compiler_idle();
    let built = cranpose_render_wgpu::pipelines_created();
    let frame = support::capture_graph(&mut renderer, card_and_label(), WIDTH, HEIGHT);
    assert!(
        support::distinct_colors(&frame.pixels) > 3,
        "the card and the label show"
    );
    assert_eq!(
        cranpose_render_wgpu::pipelines_created(),
        built,
        "the frame drew its rect and its text with pipelines built before it"
    );
}
