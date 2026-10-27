// #1947's instant tier: while their own pipelines compile, images, text,
// turned text, elevation shadows and composited layers draw through the
// pipelines a renderer builds as it starts, and none is left out. The frame
// once their own pipelines landed shows what the instant draws showed,
// within the tier's approximations.

use cranpose_render_common::{
    graph::{DrawRunNode, PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode},
    layer_transform::layer_transform_to_parent,
};
use cranpose_render_wgpu::RenderStatsSnapshot;
use cranpose_ui_graphics::{
    Color, DrawPrimitive, GraphicsLayer, ImageBitmap, ImageSampling, Point, Rect, Size,
};

use crate::{shared_test_support, support};

const WIDTH: u32 = 160;
const HEIGHT: u32 = 96;

// Levels a pixel of an instant draw may differ by from its own pipeline's:
// the instant pipeline blends premultiplied colours, which a draw blending
// straight ones rounds differently.
const ROUNDING: u8 = 2;

// Levels a pixel of an instant shadow may differ by: its falloff runs
// straight from its edge to its umbra, where the shadow pipeline's curves.
const FALLOFF: u8 = 4;

// Where the lifted card's elevation shadow reaches.
const LIFT_SHADOW: Rect = Rect {
    x: 96.0,
    y: 0.0,
    width: 64.0,
    height: 44.0,
};

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

// An opaque page holding `children`: its fill gives a pass interiors to lay
// down first.
fn page(children: Vec<RenderNode>) -> RenderGraph {
    let frame = Rect::from_size(Size::new(WIDTH as f32, HEIGHT as f32));
    let mut nodes = vec![support::solid_rect(frame, Color(0.1, 0.12, 0.2, 1.0))];
    nodes.extend(children);
    RenderGraph::new(shared_test_support::layer_node(
        frame,
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        nodes,
    ))
}

// A colour image and an alpha mask, side by side, scaled up three times.
fn images() -> RenderNode {
    let checker = (0..64u32)
        .flat_map(|texel| match (texel % 8 + texel / 8) % 2 {
            0 => [230, 80, 40, 255],
            _ => [40, 160, 220, 200],
        })
        .collect();
    let mask = (0..64u8).map(|texel| texel * 4).collect();
    let image = |rect, image| DrawPrimitive::Image {
        rect,
        image,
        alpha: 1.0,
        color_filter: None,
        sampling: ImageSampling::Nearest,
        src_rect: None,
    };
    RenderNode::DrawRun(DrawRunNode::new(
        PrimitivePhase::BeforeChildren,
        vec![
            image(
                rect(4.0, 4.0, 24.0, 24.0),
                ImageBitmap::from_rgba8(8, 8, checker).expect("checker"),
            ),
            image(
                rect(32.0, 4.0, 24.0, 24.0),
                ImageBitmap::from_alpha8(8, 8, [250, 240, 120], mask).expect("mask"),
            ),
        ],
    ))
}

// A layer at `bounds` drawn with `layer`, holding `children`, which its
// own space places.
fn layer(bounds: Rect, layer: GraphicsLayer, children: Vec<RenderNode>) -> RenderNode {
    let local = Rect::from_size(Size::new(bounds.width, bounds.height));
    let placement = Point::new(bounds.x, bounds.y);
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        local,
        layer_transform_to_parent(local, placement, &layer),
        layer,
        children,
    )))
}

// A layer holding a card and a label.
fn card(bounds: Rect, graphics: GraphicsLayer, label: &str) -> RenderNode {
    let local = Rect::from_size(Size::new(bounds.width, bounds.height));
    let card = support::solid_rect(local, Color(0.85, 0.85, 0.8, 1.0));
    layer(
        bounds,
        graphics,
        vec![card, support::text_node(label.len(), label, local, 12.0)],
    )
}

// What a launch's later screens bring: images, a label turned in place, a
// card casting an elevation shadow, a translucent card and a tilted one.
fn new_content() -> RenderGraph {
    page(vec![
        images(),
        layer(
            rect(64.0, 6.0, 40.0, 18.0),
            GraphicsLayer {
                rotation_z: 10.0,
                ..GraphicsLayer::default()
            },
            vec![support::text_node(
                4,
                "Turn",
                rect(0.0, 0.0, 40.0, 18.0),
                12.0,
            )],
        ),
        card(
            rect(112.0, 8.0, 36.0, 20.0),
            GraphicsLayer {
                shadow_elevation: 8.0,
                ..GraphicsLayer::default()
            },
            "Lift",
        ),
        card(
            rect(8.0, 48.0, 56.0, 36.0),
            GraphicsLayer {
                alpha: 0.7,
                ..GraphicsLayer::default()
            },
            "Fade",
        ),
        card(
            rect(84.0, 46.0, 56.0, 36.0),
            GraphicsLayer {
                alpha: 0.7,
                rotation_z: 12.0,
                ..GraphicsLayer::default()
            },
            "Tilt",
        ),
    ])
}

fn capture(
    renderer: &mut support::LockedRenderer,
    graph: &RenderGraph,
) -> (Vec<u8>, RenderStatsSnapshot) {
    let frame = support::capture_graph(renderer, graph.clone(), WIDTH, HEIGHT);
    let stats = renderer.last_frame_stats().expect("frame statistics");
    (frame.pixels, stats)
}

// Asserts `first` drew every draw, some through the instant pipelines, and
// shows within ROUNDING of `landed`, the frame once their own landed, and
// within FALLOFF where `shadows` reach.
fn assert_stood_in(first: (&[u8], RenderStatsSnapshot), landed: &[u8], shadows: Option<Rect>) {
    let (pixels, stats) = first;
    assert!(
        stats.instant_draws > 0,
        "the frame stands in for draws whose pipelines compile: {stats:?}"
    );
    assert_eq!(stats.placeholder_draws, 0, "no draw is left out: {stats:?}");
    let shadowed = |x: usize, y: usize| {
        shadows.is_some_and(|reach| {
            let (x, y) = (x as f32, y as f32);
            x >= reach.x && x < reach.x + reach.width && y >= reach.y && y < reach.y + reach.height
        })
    };
    let mut apart = support::pixels_differing_beyond(WIDTH, pixels, landed, ROUNDING);
    apart.retain(|&(x, y, _, _)| !shadowed(x, y));
    let shadow_apart = support::pixels_differing_beyond(WIDTH, pixels, landed, FALLOFF);
    for apart in [apart, shadow_apart] {
        assert!(
            apart.is_empty(),
            "the instant draws differ from their own pipelines': {}",
            support::describe_differing(&apart)
        );
    }
}

// The first frame of `graph` on a renderer `before` prepared, once its
// compiler is idle, with its statistics, and the frame once the graph's
// pipelines landed.
fn first_and_landed(
    before: impl FnOnce(&mut support::LockedRenderer),
    graph: &RenderGraph,
) -> ((Vec<u8>, RenderStatsSnapshot), Vec<u8>) {
    let mut renderer = support::headless_renderer_compiling_in_background().expect("GPU required");
    before(&mut renderer);
    support::wait_for_background_compiler_idle();
    let first = capture(&mut renderer, graph);
    let landed = support::capture_graph_settled(&mut renderer, graph, WIDTH, HEIGHT);
    drop(renderer);
    (first, landed.pixels)
}

#[test]
fn a_first_frame_draws_images_through_the_instant_pipeline() {
    // The instant tier builds while an app composes its first screen.
    let ((first, stats), landed) = first_and_landed(|_| {}, &page(vec![images()]));
    assert_stood_in((&first, stats), &landed, None);
}

#[test]
fn a_pass_laying_interiors_first_draws_new_content_through_the_instant_pipeline() {
    // An opaque page opens the depth pre-pass with the first frame that
    // ends after its pipelines landed; no pipeline the new content draws
    // with in that pass is built then.
    let opening = page(Vec::new());
    let open = |renderer: &mut support::LockedRenderer| {
        support::capture_graph_settled(renderer, &opening, WIDTH, HEIGHT);
        support::wait_for_background_compiler_idle();
        capture(renderer, &opening);
    };
    let ((first, stats), landed) = first_and_landed(open, &new_content());
    assert!(
        stats.depth_passes > 0,
        "the new content draws in a pass laying interiors first: {stats:?}"
    );
    assert_stood_in((&first, stats), &landed, Some(LIFT_SHADOW));
}
