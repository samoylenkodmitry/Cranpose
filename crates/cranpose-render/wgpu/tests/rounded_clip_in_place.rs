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

/// A layer clipped to a rounded rect of `radius`, drawn in place, or
/// through a surface of its own and the composite's rounded mask when
/// `offscreen`.
fn rounded(radius: f32, offscreen: bool) -> GraphicsLayer {
    GraphicsLayer {
        clip: true,
        shape: LayerShape::Rounded(RoundedCornerShape::uniform(radius)),
        compositing_strategy: if offscreen {
            CompositingStrategy::Offscreen
        } else {
            CompositingStrategy::Auto
        },
        ..GraphicsLayer::default()
    }
}

/// The page under `layer`, placed at `at`.
fn on_page(
    bounds: Rect,
    at: Point,
    layer: GraphicsLayer,
    children: Vec<RenderNode>,
) -> RenderGraph {
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
                bounds,
                ProjectiveTransform::translation(at.x, at.y),
                layer,
                children,
            ))),
        ],
    )
}

/// The rounded layer over `children`, drawn in place or `offscreen`.
fn clipped(children: Vec<RenderNode>, offscreen: bool) -> RenderGraph {
    on_page(CLIP, AT, rounded(RADIUS, offscreen), children)
}

/// Renders the layer both ways and returns the two frames' pixels and how
/// many layers each isolated.
fn both_ways(children: Vec<RenderNode>) -> Option<([Vec<u8>; 2], [u32; 2])> {
    both_graphs(|offscreen| clipped(children.clone(), offscreen))
}

/// Renders the graph `scene` builds drawn in place, then with its rounded
/// layer offscreen, and returns the two frames and their isolated counts.
fn both_graphs(scene: impl Fn(bool) -> RenderGraph) -> Option<([Vec<u8>; 2], [u32; 2])> {
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
        let captured = capture_graph_settled(&mut renderer, scene(offscreen), WIDTH, HEIGHT);
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

/// The workspace port's bid/ask bar in miniature: a rounded clip at a
/// fractional place inside a panel that clips, `panel_width` wide.
fn in_panel(panel_width: f32, offscreen: bool) -> RenderGraph {
    let bar = Rect {
        x: 0.0,
        y: 0.0,
        width: 100.333_33,
        height: 6.0,
    };
    let panel = GraphicsLayer {
        clip: true,
        ..GraphicsLayer::default()
    };
    on_page(
        Rect {
            x: 0.0,
            y: 0.0,
            width: panel_width,
            height: 40.0,
        },
        Point::new(10.25, 10.5),
        panel,
        vec![RenderNode::Layer(Box::new(
            shared_test_support::layer_node(
                bar,
                ProjectiveTransform::translation(9.333_33, 7.666_67),
                rounded(3.0, offscreen),
                vec![solid_rect(bar, Color(0.1, 0.65, 0.3, 1.0))],
            ),
        ))],
    )
}

/// Whether the pixel at (x, y) lies within a pixel of the edge of the bar
/// `in_panel` places, at (19.583, 18.167) on the page.
fn on_the_bar_edge(x: usize, y: usize) -> bool {
    let (left, top) = (10.25 + 9.333_33, 10.5 + 7.666_67);
    let (right, bottom) = (left + 100.333_33, top + 6.0);
    let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
    let near = |value: f32, edge: f32| (value - edge).abs() < 1.0;
    let within = |value: f32, from: f32, to: f32| value > from - 1.0 && value < to + 1.0;
    (within(x, left, right) && (near(y, top) || near(y, bottom)))
        || (within(y, top, bottom) && (near(x, left) || near(x, right)))
}

#[test]
fn a_rounded_bar_at_a_fractional_place_inside_a_panel_that_holds_it_draws_in_place() {
    let Some((frames, isolated)) = both_graphs(|offscreen| in_panel(140.0, offscreen)) else {
        return;
    };
    assert_eq!(isolated, [0, 1], "in place, then through a surface");
    // Off the pixel grid the two differ only on the bar's edge: in place the
    // fill's own edge coverage meets the clip's, as Skia's analytic rounded
    // clip multiplies them, where the surface resamples a whole-pixel raster.
    let differing = support::differing_pixels(WIDTH, &frames[0], &frames[1]);
    let off_edge: Vec<_> = differing
        .iter()
        .filter(|(x, y, _, _)| !on_the_bar_edge(*x, *y))
        .copied()
        .collect();
    assert!(
        off_edge.is_empty(),
        "nothing leaks, moves or drops off the bar's edge: {}",
        support::describe_differing(&off_edge)
    );
}

#[test]
fn a_rounded_bar_a_panel_cuts_keeps_its_surface() {
    let Some((frames, isolated)) = both_graphs(|offscreen| in_panel(60.0, offscreen)) else {
        return;
    };
    assert_eq!(
        isolated,
        [1, 1],
        "a clip that cuts the rounded one leaves a surface"
    );
    let differing = support::differing_pixels(WIDTH, &frames[0], &frames[1]);
    assert!(
        differing.is_empty(),
        "both draw through the surface: {}",
        support::describe_differing(&differing)
    );
}
