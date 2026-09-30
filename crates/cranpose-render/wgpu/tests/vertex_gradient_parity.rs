use cranpose_render_common::{
    graph::{
        DrawCommandId, DrawPrimitiveNode, DrawRunNode, LayerNode, PrimitiveEntry, PrimitiveNode,
        PrimitivePhase, RenderGraph, RenderNode,
    },
    layer_transform::layer_transform_to_parent,
    style_shared::DrawPlacement,
};
use cranpose_ui_graphics::{
    Brush, Color, CommandRecording, CornerRadii, DrawPrimitive, DrawScope, DrawScopeDefault,
    GraphicsLayer, Point, Rect, Size, TileMode,
};

use crate::{shared_test_support, support};

const SIDE: u32 = 256;
const TOGGLE: &str = "CRANPOSE_SHAPE_VARIANTS";
const MAX_DIVERGING_BYTES: usize = 16;
const PAGE: Color = Color(0.02, 0.02, 0.05, 1.0);
const BAR: Color = Color(1.0, 1.0, 1.0, 0.45);
const RAMPS: [(Color, Color); 4] = [
    (Color(0.0, 0.0, 0.0, 1.0), Color(1.0, 1.0, 1.0, 1.0)),
    (Color(0.9, 0.3, 0.1, 1.0), Color(0.1, 0.2, 0.8, 1.0)),
    (Color(1.0, 1.0, 1.0, 0.9), Color(0.2, 0.9, 0.4, 0.35)),
    (Color(0.05, 0.05, 0.1, 1.0), Color(0.3, 0.25, 0.9, 1.0)),
];

fn full(side: f32) -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width: side,
        height: side,
    }
}

fn ramp(index: usize, rect: Rect) -> Brush {
    let (from, to) = RAMPS[index % RAMPS.len()];
    let colors = vec![from, to];
    match index % 6 {
        0 => Brush::vertical_gradient(colors, 0.0, rect.height),
        1 => Brush::horizontal_gradient(colors, 0.0, rect.width),
        2 => Brush::linear_gradient(colors),
        3 => Brush::vertical_gradient(colors, rect.height, 0.0),
        4 => Brush::vertical_gradient_stops(
            vec![(0.2, from), (0.8, to)],
            -rect.height * 0.5,
            rect.height * 1.5,
            TileMode::Clamp,
        ),
        _ => Brush::vertical_gradient_tiled(colors, -3.0, rect.height + 5.0, TileMode::Mirror),
    }
}

fn scene(side: f32, columns: usize, rows: usize) -> Vec<DrawPrimitive> {
    let mut scope = DrawScopeDefault::new(Size::new(side, side));
    scope.draw_rect_at(full(side), Brush::solid(PAGE));
    let (cell_width, cell_height) = (side / columns as f32, side / rows as f32);
    for index in 0..columns * rows {
        let rect = Rect {
            x: (index % columns) as f32 * cell_width + 1.37,
            y: (index / columns) as f32 * cell_height + 2.61,
            width: cell_width - 3.1,
            height: cell_height - 3.7,
        };
        if index % 2 == 0 {
            scope.draw_rect_at(rect, ramp(index, rect));
        } else {
            let radius = rect.width.min(rect.height) * 0.2;
            scope.draw_round_rect_at(rect, ramp(index, rect), CornerRadii::uniform(radius));
        }
        if index % 3 == 0 {
            scope.draw_rect_at(
                Rect {
                    x: rect.x + rect.width * 0.3,
                    y: rect.y + rect.height * 0.4,
                    width: rect.width * 0.2,
                    height: rect.height * 0.5,
                },
                Brush::solid(BAR),
            );
        }
    }
    let primitives = scope.into_primitives();
    let recording = CommandRecording::from_primitives(primitives.clone());
    let shaded = (0..recording.shapes().len())
        .filter_map(|index| recording.shapes().get(index))
        .filter(cranpose_ui_graphics::ShapeRecord::is_vertex_gradient)
        .count();
    assert_eq!(
        shaded,
        columns * rows,
        "every ramp of the scene must be one its vertices shade"
    );
    primitives
}

fn layer(node_id: Option<cranpose_core::NodeId>, children: Vec<RenderNode>) -> RenderGraph {
    RenderGraph::new(LayerNode {
        node_id,
        local_bounds: full(SIDE as f32),
        children,
        translated_content_context: true,
        ..LayerNode::default()
    })
}

fn arena_graph(primitives: Vec<DrawPrimitive>) -> RenderGraph {
    layer(
        None,
        vec![RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            primitives,
        ))],
    )
}

fn clipped_graph(primitives: Vec<DrawPrimitive>, clip: Rect) -> RenderGraph {
    let children = primitives
        .into_iter()
        .map(|primitive| {
            RenderNode::Primitive(PrimitiveEntry {
                phase: PrimitivePhase::BeforeChildren,
                node: PrimitiveNode::Draw(Box::new(DrawPrimitiveNode {
                    primitive,
                    clip: Some(clip),
                })),
            })
        })
        .collect();
    layer(None, children)
}

fn stored_graph(primitives: Vec<DrawPrimitive>) -> RenderGraph {
    layer(
        Some(support::STORED_RUN_NODE),
        vec![RenderNode::DrawRun(DrawRunNode::for_command(
            PrimitivePhase::BeforeChildren,
            Some(DrawCommandId {
                node_id: support::STORED_RUN_NODE,
                command_index: 0,
                placement: DrawPlacement::Behind,
            }),
            primitives,
        ))],
    )
}

fn turned_graph(primitives: Vec<DrawPrimitive>, degrees: f32) -> RenderGraph {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 160.0,
        height: 160.0,
    };
    let turn = GraphicsLayer {
        rotation_z: degrees,
        ..GraphicsLayer::default()
    };
    let turned = shared_test_support::layer_node(
        bounds,
        layer_transform_to_parent(bounds, Point::new(48.0, 48.0), &turn),
        turn,
        vec![RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            primitives,
        ))],
    );
    let mut page = DrawScopeDefault::new(Size::new(SIDE as f32, SIDE as f32));
    page.draw_rect_at(full(SIDE as f32), Brush::solid(PAGE));
    layer(
        None,
        vec![
            RenderNode::DrawRun(DrawRunNode::new(
                PrimitivePhase::BeforeChildren,
                page.into_primitives(),
            )),
            RenderNode::Layer(Box::new(turned)),
        ],
    )
}

fn vertex_and_fragment_frames(
    graph: &RenderGraph,
    scale: f32,
    in_place: bool,
) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping vertex gradient parity: headless WGPU init failed: {err}");
            return None;
        }
    };
    let mut frame = |toggle: Option<&str>| {
        cranpose_render_wgpu::set_debug_toggle(TOGGLE, toggle);
        support::stable_settled_capture(&mut renderer, |renderer| {
            let captured =
                support::capture_graph_with_scale(renderer, graph.clone(), SIDE, SIDE, scale);
            let stats = renderer.last_frame_stats().expect("frame statistics");
            assert!(
                !in_place || stats.isolated_layer_renders == 0,
                "the turned layer must draw in place: {stats:?}"
            );
            captured
        })
    };
    let vertex = frame(None);
    let fragment = frame(Some("0"));
    cranpose_render_wgpu::set_debug_toggle(TOGGLE, None);
    Some((vertex, fragment))
}

fn assert_shades_as_the_fragment_gradient(
    name: &str,
    graph: RenderGraph,
    scale: f32,
    (in_place, max_bytes): (bool, usize),
) {
    let Some((vertex, fragment)) = vertex_and_fragment_frames(&graph, scale, in_place) else {
        return;
    };
    assert!(
        support::distinct_colors(&vertex) > 256,
        "{name}: the ramps must draw"
    );
    let bytes = vertex.iter().zip(&fragment).filter(|(a, b)| a != b).count();
    let worst = support::max_channel_delta(&vertex, &fragment);
    let differing = support::differing_pixels(SIDE, &vertex, &fragment);
    let account = if differing.is_empty() {
        String::new()
    } else {
        support::describe_differing(&differing)
    };
    eprintln!(
        "{name}: vertex-vs-fragment gradient differing {bytes} bytes worst {worst} {account}"
    );
    assert!(
        bytes <= max_bytes && worst <= 1,
        "{name}: {bytes} bytes diverged (worst {worst}), over a bound of {max_bytes} bytes \
         and one level; a ramp its vertices shade must land on the bytes the fragment \
         stage samples: {account}"
    );
}

#[test]
fn vertex_gradients_over_rects_and_rounded_rects_shade_as_the_fragment_gradient() {
    assert_shades_as_the_fragment_gradient(
        "rects",
        arena_graph(scene(SIDE as f32, 4, 3)),
        1.0,
        (false, MAX_DIVERGING_BYTES),
    );
}

#[test]
fn clipped_vertex_gradients_shade_as_the_fragment_gradient() {
    let clip = Rect {
        x: 20.5,
        y: 30.25,
        width: 180.0,
        height: 170.5,
    };
    assert_shades_as_the_fragment_gradient(
        "clipped",
        clipped_graph(scene(SIDE as f32, 4, 3), clip),
        1.0,
        (false, MAX_DIVERGING_BYTES),
    );
}

#[test]
fn scaled_vertex_gradients_shade_as_the_fragment_gradient() {
    assert_shades_as_the_fragment_gradient(
        "scaled",
        arena_graph(scene(SIDE as f32, 4, 3)),
        1.75,
        (false, MAX_DIVERGING_BYTES),
    );
}

#[test]
fn unaligned_gradient_geometry_keeps_fragment_sampling() {
    let mut graph = arena_graph(scene(SIDE as f32, 4, 3));
    graph.root.translated_content_context = false;
    assert_shades_as_the_fragment_gradient("unaligned", graph, 1.0, (false, 0));
}

#[test]
fn stored_vertex_gradients_shade_as_the_fragment_gradient() {
    assert_shades_as_the_fragment_gradient(
        "stored",
        stored_graph(scene(SIDE as f32, 8, 9)),
        1.0,
        (false, MAX_DIVERGING_BYTES),
    );
}

#[test]
fn turned_vertex_gradients_shade_as_the_fragment_gradient() {
    assert_shades_as_the_fragment_gradient(
        "turned",
        turned_graph(scene(160.0, 3, 3), 23.0),
        1.0,
        (true, 0),
    );
}
