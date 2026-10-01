use cranpose_render_common::graph::{ProjectiveTransform, RenderGraph, RenderNode};
use cranpose_ui_graphics::{
    Brush, Color, CompositingStrategy, CornerRadii, DrawPrimitive, GraphicsLayer, LayerShape,
    Point, Rect, RoundedCornerShape,
};
use support::{capture_graph_settled, draw_node, page_graph, solid_rect};

use crate::{shared_test_support, support};

const WIDTH: u32 = 160;
const HEIGHT: u32 = 80;
const CLIP: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 120.0,
    height: 40.0,
};
const AT: Point = Point { x: 20.0, y: 20.0 };
const RADIUS: f32 = 12.0;
const PAGE: Color = Color(0.95, 0.95, 0.92, 1.0);

/// A layer clipped to a rounded rect over `children`, drawn in place, or
/// through a surface of its own and the composite's rounded mask when
/// `offscreen`.
fn clipped(children: Vec<RenderNode>, offscreen: bool) -> RenderGraph {
    let layer = GraphicsLayer {
        clip: true,
        shape: LayerShape::Rounded(RoundedCornerShape::uniform(RADIUS)),
        compositing_strategy: if offscreen {
            CompositingStrategy::Offscreen
        } else {
            CompositingStrategy::Auto
        },
        ..GraphicsLayer::default()
    };
    page_graph(
        WIDTH,
        HEIGHT,
        vec![
            solid_rect(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: WIDTH as f32,
                    height: HEIGHT as f32,
                },
                PAGE,
            ),
            RenderNode::Layer(Box::new(shared_test_support::layer_node(
                CLIP,
                ProjectiveTransform::translation(AT.x, AT.y),
                layer,
                children,
            ))),
        ],
    )
}

/// Renders the layer both ways and returns the two frames' pixels and how
/// many layers each isolated.
fn both_ways(children: Vec<RenderNode>) -> Option<([Vec<u8>; 2], [u32; 2])> {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping rounded clip in place: {err}");
            return None;
        }
    };
    let mut frames = [Vec::new(), Vec::new()];
    let mut isolated = [0, 0];
    for (index, offscreen) in [false, true].into_iter().enumerate() {
        let captured = capture_graph_settled(
            &mut renderer,
            clipped(children.clone(), offscreen),
            WIDTH,
            HEIGHT,
        );
        isolated[index] = renderer
            .last_frame_stats()
            .expect("frame statistics")
            .isolated_layer_renders;
        frames[index] = captured.pixels;
    }
    Some((frames, isolated))
}

/// Whether the pixel at (x, y) lies in one of the clip's corner squares on
/// the page, where the clip's arcs are.
fn in_a_corner(x: u32, y: u32) -> bool {
    let (x, y) = (x as f32 + 0.5 - AT.x, y as f32 + 0.5 - AT.y);
    let near = |value: f32, extent: f32| value < RADIUS + 1.0 || value > extent - RADIUS - 1.0;
    near(x, CLIP.width) && near(y, CLIP.height) && x > -1.0 && y > -1.0
}

#[test]
fn a_fill_reaching_a_rounded_clips_corners_draws_in_place_as_its_surface_would() {
    let Some((frames, isolated)) = both_ways(vec![solid_rect(CLIP, Color(0.85, 0.15, 0.2, 1.0))])
    else {
        return;
    };
    assert_eq!(isolated, [0, 1], "in place, then through a surface");
    let differing = support::differing_pixels(WIDTH, &frames[0], &frames[1]);
    assert!(
        differing.is_empty(),
        "one fill takes the clip's coverage as the surface's mask does: {}",
        support::describe_differing(&differing)
    );
}

#[test]
fn a_bar_over_a_rounded_track_differs_from_its_surface_only_where_their_edges_meet_an_arc() {
    let track = DrawPrimitive::RoundRect {
        rect: CLIP,
        brush: Brush::solid(Color(0.85, 0.15, 0.2, 1.0)),
        radii: CornerRadii::uniform(RADIUS),
        stroke: None,
    };
    let Some((frames, isolated)) = both_ways(vec![
        draw_node(track, None),
        solid_rect(
            Rect {
                width: 70.0,
                ..CLIP
            },
            Color(0.1, 0.65, 0.3, 1.0),
        ),
    ]) else {
        return;
    };
    assert_eq!(isolated, [0, 1], "in place, then through a surface");
    let differing = support::differing_pixels(WIDTH, &frames[0], &frames[1]);
    let outside: Vec<_> = differing
        .iter()
        .filter(|(x, y, _, _)| !in_a_corner(*x as u32, *y as u32))
        .copied()
        .collect();
    assert!(
        outside.is_empty(),
        "away from the arcs, the records drawn in place match the surface: {}",
        support::describe_differing(&outside)
    );
}
