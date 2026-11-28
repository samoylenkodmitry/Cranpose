//! While pipelines compile on the background compiler, frames draw without
//! waiting for them: shapes as fallback triangles, and what has no fallback
//! left out. The renderer asks for frames until the pipelines land, and the
//! frame drawn then shows what a renderer that builds every pipeline where
//! it is first needed shows, though earlier frames kept layers drawn before.

use cranpose_core::NodeId;
use cranpose_render_common::{
    Renderer,
    graph::{CachePolicy, DrawRunNode, LayerNode, PrimitivePhase, ProjectiveTransform, RenderNode},
    layer_transform::layer_transform_to_parent,
};
use cranpose_ui_graphics::{
    Color, DrawScope, DrawScopeDefault, GraphicsLayer, Point, RUNTIME_SHADER_PRELUDE_WGSL, Rect,
    RenderEffect, RuntimeShader, ShaderPlaceholder, Size,
};

use crate::{
    shared_test_support,
    support::{self, LockedRenderer},
};

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
fn page_with_tilted_card() -> LayerNode {
    page_with_tilted_card_and(Vec::new())
}

/// [`page_with_tilted_card`] with `more` drawn over it.
fn page_with_tilted_card_and(more: Vec<RenderNode>) -> LayerNode {
    let frame = Rect::from_size(Size::new(WIDTH as f32, HEIGHT as f32));
    let mut card = shared_test_support::layer_node(
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
    // a node whose layer the cache may keep
    card.node_id = Some(NodeId(1));
    card.cache_policy = CachePolicy::Auto;
    let mut nodes: Vec<RenderNode> = vec![
        support::solid_rect(frame, Color::BLACK),
        support::solid_rect(rect(6.0, 6.0, 50.0, 50.0), Color(0.9, 0.2, 0.2, 1.0)),
        support::text_node(7, "Page", rect(6.0, 62.0, 60.0, 24.0), 16.0),
        RenderNode::Layer(Box::new(card)),
    ];
    nodes.extend(more);
    shared_test_support::layer_node(
        frame,
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        nodes,
    )
}

/// A pane drawn by a shader no other test draws, red while it compiles.
fn shader_pane() -> RenderNode {
    let mut shader = RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}
         // a pane beside a kept card {}
         @fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
             return vec4<f32>(0.0, 0.0, 1.0, 1.0);
         }}",
        std::process::id()
    ));
    shader.set_placeholder(Some(ShaderPlaceholder {
        color: Color(1.0, 0.0, 0.0, 1.0),
        shape: None,
    }));
    shader.set_position_independent(true);
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        rect(0.0, 0.0, 24.0, 16.0),
        ProjectiveTransform::translation(8.0, 70.0),
        GraphicsLayer {
            render_effect: Some(RenderEffect::runtime_shader(shader)),
            ..GraphicsLayer::default()
        },
        Vec::new(),
    )))
}

/// Solid, gradient and stroked shapes, each kind with a pipeline of its own.
fn shapes() -> RenderNode {
    let mut recording = cranpose_ui_graphics::CommandRecording::default();
    let mut scope = DrawScopeDefault::new(Size::new(WIDTH as f32, HEIGHT as f32), &mut recording);
    support::record_mixed_scene(&mut scope);
    RenderNode::DrawRun(DrawRunNode::new(
        PrimitivePhase::BeforeChildren,
        scope.into_primitives(),
    ))
}

/// The shapes on the page, and again in a layer at half opacity turned by
/// `degrees` about its centre: projected from its target while turned,
/// blitted one to one at zero.
fn turned(degrees: f32) -> LayerNode {
    let bounds = Rect::from_size(Size::new(WIDTH as f32, HEIGHT as f32));
    let turn = GraphicsLayer {
        alpha: 0.5,
        rotation_z: degrees,
        ..GraphicsLayer::default()
    };
    let layer = shared_test_support::layer_node(
        bounds,
        layer_transform_to_parent(
            bounds,
            Point::new(WIDTH as f32 / 2.0, HEIGHT as f32 / 2.0),
            &turn,
        ),
        turn,
        vec![shapes()],
    );
    support::page_graph(
        WIDTH,
        HEIGHT,
        vec![shapes(), RenderNode::Layer(Box::new(layer))],
    )
}

/// What a renderer that builds each pipeline where it is first needed draws
/// for `graph`, after `before`.
fn built_where_needed(before: Option<&LayerNode>, graph: &LayerNode) -> Vec<u8> {
    let mut reference = LockedRenderer::beside_locked().expect("reference renderer");
    if let Some(before) = before {
        support::capture_graph(&mut reference, before, WIDTH, HEIGHT);
    }
    support::capture_graph(&mut reference, graph, WIDTH, HEIGHT).pixels
}

/// Draws `before`, if any, and then `graph` until its pipelines landed,
/// asserting the first frame found them still compiling and asked for the
/// frames after it; the frame once they landed.
fn landed(before: Option<&LayerNode>, graph: &LayerNode) -> Vec<u8> {
    let mut renderer = support::headless_renderer_compiling_in_background().expect("GPU required");
    let first = before.unwrap_or(graph);
    support::capture_graph(&mut renderer, first, WIDTH, HEIGHT);
    let stats = renderer.last_frame_stats().expect("frame statistics");
    assert!(
        !stats.pipelines_settled(),
        "the first frame draws while its pipelines compile: {stats:?}"
    );
    assert!(
        renderer.needs_frame_warmup(),
        "a frame drawn without its pipelines asks for the frames after it"
    );
    support::capture_graph_settled(&mut renderer, graph, WIDTH, HEIGHT).pixels
}

#[test]
fn a_layer_kept_while_its_pipelines_compiled_is_drawn_again_once_they_land() {
    let graph = page_with_tilted_card();
    let frame = landed(None, &graph);
    assert!(
        support::distinct_colors(&frame) > 4,
        "the page, its label and the card show"
    );
    support::assert_same_bytes(
        "the frame once the pipelines landed",
        WIDTH,
        &frame,
        &built_where_needed(None, &graph),
    );
}

#[test]
fn a_layer_turned_back_onto_the_pixel_grid_while_its_blit_compiled_draws_it_once_it_lands() {
    let (turned, back) = (turned(12.0), turned(0.0));
    support::assert_same_bytes(
        "the layer back on the grid once its blit landed",
        WIDTH,
        &landed(Some(&turned), &back),
        &built_where_needed(Some(&turned), &back),
    );
}

#[test]
fn a_kept_layer_drawn_with_its_own_pipelines_stays_kept_when_another_pipeline_lands() {
    let (card, beside) = (
        page_with_tilted_card(),
        page_with_tilted_card_and(vec![shader_pane()]),
    );
    let mut renderer = support::headless_renderer_compiling_in_background().expect("GPU required");
    support::capture_graph_settled(&mut renderer, &card, WIDTH, HEIGHT);
    for _ in 0..3 {
        support::capture_graph(&mut renderer, &card, WIDTH, HEIGHT);
    }
    let kept = renderer.last_frame_stats().expect("frame statistics");
    assert!(kept.layer_cache_hits > 0, "the card is kept: {kept:?}");
    support::capture_graph(&mut renderer, &beside, WIDTH, HEIGHT);
    let waiting = renderer.last_frame_stats().expect("frame statistics");
    assert_eq!(
        waiting.placeholder_draws, 1,
        "the pane waits for its shader"
    );
    support::wait_for_background_compiler_idle();
    let landed = support::capture_graph(&mut renderer, &beside, WIDTH, HEIGHT);
    let stats = renderer.last_frame_stats().expect("frame statistics");
    drop(renderer);
    assert_eq!(stats.placeholder_draws, 0, "the pane's shader landed");
    assert!(
        stats.layer_cache_hits > 0,
        "the card holds no stand-in, so the landing keeps it: {stats:?}"
    );
    support::assert_same_bytes(
        "the frame once the pane's shader landed",
        WIDTH,
        &landed.pixels,
        &built_where_needed(Some(&card), &beside),
    );
}
