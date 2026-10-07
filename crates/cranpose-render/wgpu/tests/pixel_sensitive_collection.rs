use std::rc::Rc;

use cranpose_render_common::graph::{
    DrawRunNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform, RenderGraph,
    RenderNode, TextPrimitiveNode,
};
use cranpose_ui::{
    TextLayoutOptions,
    text::{AnnotatedString, SpanStyle, TextStyle, TextUnit},
};
use cranpose_ui_graphics::{
    Color, CompositingStrategy, DrawPrimitive, DrawTextStyle, GraphicsLayer, Rect, TextPrimitive,
};

use crate::{shared_test_support, support};

const LOGICAL_WIDTH: u32 = 96;
const LOGICAL_HEIGHT: u32 = 64;
const ROOT_SCALE: f32 = 1.25;
const PIXEL_WIDTH: u32 = 120;
const PIXEL_HEIGHT: u32 = 80;

fn hidden_text(clip: Option<Rect>) -> RenderNode {
    hidden_text_at(
        Rect {
            x: 4.0,
            y: 4.0,
            width: 20.0,
            height: 22.0,
        },
        clip,
    )
}

fn hidden_text_at(rect: Rect, clip: Option<Rect>) -> RenderNode {
    let annotated = AnnotatedString {
        text: "M".to_owned(),
        ..AnnotatedString::default()
    };
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
            node_id: 51_001,
            rect,
            render_text: std::sync::Arc::new(annotated.render_string()),
            text: Rc::new(annotated),
            text_style: std::sync::Arc::new(TextStyle::from_span_style(SpanStyle {
                color: Some(Color::BLACK),
                font_size: TextUnit::Sp(18.0),
                ..Default::default()
            })),
            font_size: 18.0,
            layout_options: TextLayoutOptions::default(),
            clip,
            paint: Default::default(),
        })),
    })
}

fn visible_content() -> RenderNode {
    support::solid_rect(
        Rect {
            x: 8.35,
            y: 7.6,
            width: 19.25,
            height: 13.0,
        },
        Color(0.15, 0.35, 0.9, 1.0),
    )
}

fn sensitive_text_run() -> RenderNode {
    RenderNode::DrawRun(DrawRunNode::new(
        PrimitivePhase::BeforeChildren,
        vec![DrawPrimitive::Text(Box::new(TextPrimitive {
            rect: Rect {
                x: 5.0,
                y: 4.0,
                width: 22.0,
                height: 22.0,
            },
            text: Rc::from("M"),
            style: DrawTextStyle::new(18.0),
            color: Color::BLACK,
        }))],
    ))
}

fn graph_with_direct_clipped_text() -> RenderGraph {
    let wrapper = shared_test_support::layer_node(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 48.0,
            height: 36.0,
        },
        ProjectiveTransform::translation(1.35, 2.25),
        GraphicsLayer::default(),
        vec![hidden_text(Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        }))],
    );
    let outer = shared_test_support::layer_node(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 48.0,
            height: 36.0,
        },
        ProjectiveTransform::uniform_scale(1.1).then(ProjectiveTransform::translation(12.35, 9.65)),
        GraphicsLayer {
            compositing_strategy: CompositingStrategy::Offscreen,
            ..Default::default()
        },
        vec![visible_content(), RenderNode::Layer(Box::new(wrapper))],
    );
    support::page_graph(
        LOGICAL_WIDTH,
        LOGICAL_HEIGHT,
        vec![
            support::solid_rect(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: LOGICAL_WIDTH as f32,
                    height: LOGICAL_HEIGHT as f32,
                },
                Color::WHITE,
            ),
            RenderNode::Layer(Box::new(outer)),
        ],
    )
}

fn replace_hidden_descendant(graph: &mut RenderGraph, descendant: RenderNode) {
    replace_hidden_descendant_with_transform(
        graph,
        descendant,
        ProjectiveTransform::translation(1.35, 2.25),
    );
}

fn replace_hidden_descendant_with_transform(
    graph: &mut RenderGraph,
    descendant: RenderNode,
    transform: ProjectiveTransform,
) {
    let Some(RenderNode::Layer(outer)) = graph.root.children.get_mut(1) else {
        panic!("expected the isolated outer layer");
    };
    let empty_clip = shared_test_support::layer_node(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        },
        transform,
        GraphicsLayer {
            clip: true,
            ..Default::default()
        },
        vec![descendant],
    );
    let wrapper = shared_test_support::layer_node(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 48.0,
            height: 36.0,
        },
        ProjectiveTransform::translation(1.35, 2.25),
        GraphicsLayer::default(),
        vec![RenderNode::Layer(Box::new(empty_clip))],
    );
    outer.children = vec![visible_content(), RenderNode::Layer(Box::new(wrapper))];
}

fn replace_clipped_out_descendant(graph: &mut RenderGraph, descendant: RenderNode) {
    let Some(RenderNode::Layer(outer)) = graph.root.children.get_mut(1) else {
        panic!("expected the isolated outer layer");
    };
    outer.graphics_layer.clip = true;
    let mut child = shared_test_support::layer_node(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 22.0,
        },
        ProjectiveTransform::translation(80.0, 4.0),
        GraphicsLayer::default(),
        vec![descendant],
    );
    child.draws_within_bounds = true;
    outer.children = vec![visible_content(), RenderNode::Layer(Box::new(child))];
}

fn replace_with_direct_translation(graph: &mut RenderGraph, descendant: RenderNode) {
    let Some(RenderNode::Layer(outer)) = graph.root.children.get_mut(1) else {
        panic!("expected the isolated outer layer");
    };
    let wrapper = shared_test_support::layer_node(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 48.0,
            height: 36.0,
        },
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![descendant],
    );
    outer.children = vec![visible_content(), RenderNode::Layer(Box::new(wrapper))];
}

fn capture(renderer: &mut support::LockedRenderer, graph: &RenderGraph) -> Vec<u8> {
    support::capture_settled(renderer, |renderer| {
        support::capture_graph_with_scale(
            renderer,
            graph.clone(),
            PIXEL_WIDTH,
            PIXEL_HEIGHT,
            ROOT_SCALE,
        )
    })
    .pixels
}

#[test]
fn nested_empty_clip_text_keeps_fractional_isolated_pixels_rigid_after_graph_edit() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(error) => {
            eprintln!("skipping pixel-sensitive collection check: {error}");
            return;
        }
    };

    let mut graph = graph_with_direct_clipped_text();
    let direct_reference = capture(&mut renderer, &graph);

    replace_hidden_descendant(&mut graph, hidden_text(None));
    let nested = capture(&mut renderer, &graph);
    support::assert_same_bytes(
        "text below a direct-translation empty clip",
        PIXEL_WIDTH,
        &nested,
        &direct_reference,
    );

    replace_hidden_descendant(
        &mut graph,
        support::solid_rect(
            Rect {
                x: 4.0,
                y: 4.0,
                width: 20.0,
                height: 22.0,
            },
            Color::BLACK,
        ),
    );
    let nested_shape_only = capture(&mut renderer, &graph);
    assert!(
        nested_shape_only != nested,
        "removing the pixel-sensitive descendant must visibly remove the rigid snap phase"
    );

    let non_direct =
        ProjectiveTransform::uniform_scale(1.15).then(ProjectiveTransform::translation(1.35, 2.25));
    replace_hidden_descendant_with_transform(&mut graph, hidden_text(None), non_direct);
    let non_direct_text = capture(&mut renderer, &graph);
    replace_hidden_descendant_with_transform(
        &mut graph,
        support::solid_rect(
            Rect {
                x: 4.0,
                y: 4.0,
                width: 20.0,
                height: 22.0,
            },
            Color::BLACK,
        ),
        non_direct,
    );
    let non_direct_shape = capture(&mut renderer, &graph);
    support::assert_same_bytes(
        "non-direct transform blocks pixel sensitivity",
        PIXEL_WIDTH,
        &non_direct_text,
        &non_direct_shape,
    );

    let mut clipped_graph = graph_with_direct_clipped_text();
    let Some(RenderNode::Layer(outer)) = clipped_graph.root.children.get_mut(1) else {
        panic!("expected the isolated outer layer");
    };
    outer.graphics_layer.clip = true;
    outer.children = vec![
        visible_content(),
        hidden_text_at(
            Rect {
                x: 80.0,
                y: 4.0,
                width: 20.0,
                height: 22.0,
            },
            None,
        ),
    ];
    let clipped_direct_reference = capture(&mut renderer, &clipped_graph);

    let wrapper = shared_test_support::layer_node(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 22.0,
        },
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![hidden_text(None)],
    );
    replace_clipped_out_descendant(&mut clipped_graph, RenderNode::Layer(Box::new(wrapper)));
    let clipped_nested = capture(&mut renderer, &clipped_graph);
    support::assert_same_bytes(
        "pixel-sensitive text in a layer culled by its parent clip",
        PIXEL_WIDTH,
        &clipped_nested,
        &clipped_direct_reference,
    );

    let mut run_graph = graph_with_direct_clipped_text();
    let Some(RenderNode::Layer(outer)) = run_graph.root.children.get_mut(1) else {
        panic!("expected the isolated outer layer");
    };
    outer.children = vec![visible_content(), sensitive_text_run()];
    let direct_run = capture(&mut renderer, &run_graph);

    replace_with_direct_translation(&mut run_graph, sensitive_text_run());
    let nested_run = capture(&mut renderer, &run_graph);
    support::assert_same_bytes(
        "recorded text run below a direct-translation layer",
        PIXEL_WIDTH,
        &nested_run,
        &direct_run,
    );
}

fn graph_with_nested_isolated(descendant: RenderNode) -> RenderGraph {
    let mut graph = graph_with_direct_clipped_text();
    let Some(RenderNode::Layer(outer)) = graph.root.children.get_mut(1) else {
        panic!("expected the isolated outer layer");
    };
    // The outer layer's content walks translated, so the inner isolated
    // layer snaps on its own and only the outer one asks about the text.
    outer.translated_content_context = true;
    let empty_clip = shared_test_support::layer_node(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        },
        ProjectiveTransform::translation(1.35, 2.25),
        GraphicsLayer {
            clip: true,
            ..Default::default()
        },
        vec![descendant],
    );
    let inner = shared_test_support::layer_node(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 48.0,
            height: 36.0,
        },
        ProjectiveTransform::translation(1.35, 2.25),
        GraphicsLayer {
            compositing_strategy: CompositingStrategy::Offscreen,
            ..Default::default()
        },
        vec![RenderNode::Layer(Box::new(empty_clip))],
    );
    outer.children = vec![visible_content(), RenderNode::Layer(Box::new(inner))];
    graph
}

#[test]
fn text_hidden_in_a_nested_isolated_layer_keeps_the_outer_layer_rigid() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(error) => {
            eprintln!("skipping pixel-sensitive collection check: {error}");
            return;
        }
    };

    let with_text = capture(
        &mut renderer,
        &graph_with_nested_isolated(hidden_text(None)),
    );
    let with_shape = capture(
        &mut renderer,
        &graph_with_nested_isolated(support::solid_rect(
            Rect {
                x: 4.0,
                y: 4.0,
                width: 20.0,
                height: 22.0,
            },
            Color::BLACK,
        )),
    );
    assert!(
        with_text != with_shape,
        "text below the nested isolated layer must snap the outer layer"
    );
}
