use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{MutableState, NodeId};
use cranpose_render_common::{
    SceneUpdates,
    graph::{LayerNode, PrimitiveNode, ProjectiveTransform, RenderGraph, RenderNode},
    scene_builder::{
        build_graph_from_applier, clear_command_recordings_for_tests, update_graph_from_applier,
    },
};
use cranpose_ui::{
    Box, BoxSpec, Brush, Canvas, Color, GraphicsLayer, LayoutEngine, Modifier, Size,
    TestComposition, Text, TextStyle, composable, run_test_composition,
};
use cranpose_ui_graphics::{DrawPrimitive, Rect};

const VIEWPORT: Size = Size::new(240.0, 180.0);

type Rectangles = Vec<(Option<NodeId>, Rect, Color)>;
type Texts = Vec<(NodeId, Rect, String)>;

fn initial_graph(composition: &mut TestComposition, root: NodeId) -> RenderGraph {
    let runtime = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(runtime);
    applier
        .compute_layout(root, VIEWPORT)
        .expect("composition lays out");
    let graph = build_graph_from_applier(&applier, root, 1.0).expect("initial scene graph");
    applier.clear_runtime_handle();
    graph
}

fn initial_root_graph(composition: &mut TestComposition) -> (NodeId, RenderGraph) {
    let root = composition.root().expect("root");
    let graph = initial_graph(composition, root);
    (root, graph)
}

#[composable]
fn RotatingParent<F>(rotation: Rc<Cell<f32>>, parent_id: Rc<Cell<Option<NodeId>>>, content: F)
where
    F: FnMut() + 'static,
{
    let parent = Box(
        Modifier::empty()
            .size(Size::new(120.0, 80.0))
            .graphics_layer(move || GraphicsLayer {
                rotation_z: rotation.get(),
                ..GraphicsLayer::default()
            }),
        BoxSpec::default(),
        content,
    );
    parent_id.set(Some(parent));
}

fn fresh_graph(composition: &mut TestComposition, root: NodeId) -> RenderGraph {
    let runtime = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(runtime);
    let graph = build_graph_from_applier(&applier, root, 1.0).expect("fresh scene graph");
    applier.clear_runtime_handle();
    graph
}

fn apply_updates(
    composition: &mut TestComposition,
    graph: &mut RenderGraph,
    updates: SceneUpdates<'_>,
) {
    let runtime = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(runtime);
    assert!(update_graph_from_applier(&applier, graph, updates, 1.0));
    applier.clear_runtime_handle();
}

fn graph_output(
    layer: &LayerNode,
    parent_transform: ProjectiveTransform,
    rectangles: &mut Rectangles,
    texts: &mut Texts,
) {
    let transform = layer.transform_to_parent.then(parent_transform);
    for child in &layer.children {
        match child {
            RenderNode::Layer(child) => graph_output(child, transform, rectangles, texts),
            RenderNode::DrawRun(run) => {
                let owner = run.command.map(|command| command.node_id);
                rectangles.extend(run.primitives().filter_map(|primitive| match primitive {
                    DrawPrimitive::Rect {
                        rect,
                        brush: Brush::Solid(color),
                        ..
                    } => Some((owner, transform.bounds_for_rect(rect), color)),
                    _ => None,
                }));
            }
            RenderNode::Primitive(entry) => match &entry.node {
                PrimitiveNode::Draw(draw) => {
                    if let DrawPrimitive::Rect {
                        rect,
                        brush: Brush::Solid(color),
                        ..
                    } = &draw.primitive
                    {
                        rectangles.push((None, transform.bounds_for_rect(*rect), *color));
                    }
                }
                PrimitiveNode::Text(text) => texts.push((
                    text.node_id,
                    transform.bounds_for_rect(text.rect),
                    text.text.text.clone(),
                )),
            },
        }
    }
}

fn graph_output_owned(layer: &LayerNode) -> (Rectangles, Texts) {
    let mut rectangles = Vec::new();
    let mut texts = Vec::new();
    graph_output(
        layer,
        ProjectiveTransform::identity(),
        &mut rectangles,
        &mut texts,
    );
    (rectangles, texts)
}

fn layer_for(layer: &LayerNode, node_id: NodeId) -> Option<&LayerNode> {
    if layer.node_id == Some(node_id) || layer.wraps == Some(node_id) {
        return Some(layer);
    }
    layer.children.iter().find_map(|child| match child {
        RenderNode::Layer(child) => layer_for(child, node_id),
        RenderNode::Primitive(_) | RenderNode::DrawRun(_) => None,
    })
}

#[test]
fn layer_only_update_keeps_outer_draw_and_child_canvas_recordings() {
    clear_command_recordings_for_tests();
    let rotation = Rc::new(Cell::new(0.0_f32));
    let parent_id = Rc::new(Cell::new(None));
    let child_ids = Rc::new(RefCell::new(Vec::new()));
    let outer_behind = Rc::new(Cell::new(0));
    let outer_overlay = Rc::new(Cell::new(0));
    let behind_canvas_draws = Rc::new(Cell::new(0));
    let overlay_canvas_draws = Rc::new(Cell::new(0));

    let draw_rotation = Rc::clone(&rotation);
    let draw_parent_id = Rc::clone(&parent_id);
    let draw_child_ids = Rc::clone(&child_ids);
    let draw_outer_behind = Rc::clone(&outer_behind);
    let draw_outer_overlay = Rc::clone(&outer_overlay);
    let draw_behind_canvas_draws = Rc::clone(&behind_canvas_draws);
    let draw_overlay_canvas_draws = Rc::clone(&overlay_canvas_draws);
    let mut composition = run_test_composition(move || {
        let layer_rotation = Rc::clone(&draw_rotation);
        let before = Rc::clone(&draw_outer_behind);
        let after = Rc::clone(&draw_outer_overlay);
        let parent = Box(
            Modifier::empty()
                .size(Size::new(120.0, 80.0))
                .draw_with_content(move |scope| {
                    before.set(before.get() + 1);
                    scope.draw_rect(Brush::solid(Color::BLUE));
                    scope.draw_content();
                    after.set(after.get() + 1);
                    scope.draw_rect(Brush::solid(Color::WHITE));
                })
                .graphics_layer(move || GraphicsLayer {
                    rotation_z: layer_rotation.get(),
                    ..GraphicsLayer::default()
                }),
            BoxSpec::default(),
            {
                let children = Rc::clone(&draw_child_ids);
                let behind_draws = Rc::clone(&draw_behind_canvas_draws);
                let overlay_draws = Rc::clone(&draw_overlay_canvas_draws);
                move || {
                    let behind_draws = Rc::clone(&behind_draws);
                    let behind = Canvas(
                        Modifier::empty().size(Size::new(40.0, 24.0)),
                        move |scope| {
                            behind_draws.set(behind_draws.get() + 1);
                            scope.draw_rect(Brush::solid(Color::RED));
                        },
                    );
                    let overlay_draws = Rc::clone(&overlay_draws);
                    let overlay = Canvas(
                        Modifier::empty().size(Size::new(40.0, 24.0)),
                        move |scope| {
                            overlay_draws.set(overlay_draws.get() + 1);
                            scope.draw_rect(Brush::solid(Color::GREEN));
                        },
                    );
                    children.borrow_mut().clear();
                    children.borrow_mut().extend([behind, overlay]);
                }
            },
        );
        draw_parent_id.set(Some(parent));
    });

    let (root, mut graph) = initial_root_graph(&mut composition);
    let parent = parent_id.get().expect("outer Box node");
    let children = child_ids.borrow().clone();
    let draws_before = (
        outer_behind.get(),
        outer_overlay.get(),
        behind_canvas_draws.get(),
        overlay_canvas_draws.get(),
    );
    rotation.set(37.0);
    apply_updates(
        &mut composition,
        &mut graph,
        SceneUpdates {
            content: &[],
            layers: &[parent],
        },
    );
    assert_eq!(
        (
            outer_behind.get(),
            outer_overlay.get(),
            behind_canvas_draws.get(),
            overlay_canvas_draws.get(),
        ),
        draws_before,
        "a layer-only update must retain both outer draw phases and child Canvas recordings"
    );
    assert_eq!(children.len(), 2);
    let fresh = fresh_graph(&mut composition, root);
    assert_eq!(
        graph_output_owned(&graph.root),
        graph_output_owned(&fresh.root),
        "patched output including transforms matches fresh lowering"
    );
}

#[test]
fn parent_layer_update_republishes_child_text_viewport_geometry() {
    clear_command_recordings_for_tests();
    let rotation = Rc::new(Cell::new(0.0_f32));
    let parent_id = Rc::new(Cell::new(None));
    let window_rect = Rc::new(RefCell::new(None::<MutableState<Rect>>));
    let draw_rotation = Rc::clone(&rotation);
    let draw_parent_id = Rc::clone(&parent_id);
    let draw_window_rect = Rc::clone(&window_rect);
    let mut composition = run_test_composition(move || {
        let window_rect = Rc::clone(&draw_window_rect);
        RotatingParent(
            Rc::clone(&draw_rotation),
            Rc::clone(&draw_parent_id),
            move || {
                let sink = cranpose_core::rememberMutableStateOf(|| Rect::from_size(Size::ZERO));
                *window_rect.borrow_mut() = Some(sink);
                Text(
                    "child label",
                    Modifier::empty()
                        .size(Size::new(60.0, 24.0))
                        .report_window_rect_state(sink),
                    TextStyle::default(),
                );
            },
        );
    });

    let (root, mut graph) = initial_root_graph(&mut composition);
    let parent = parent_id.get().expect("parent Box");
    let sink = window_rect.borrow().expect("Text reports its window rect");
    let before = sink.get();
    let before_transform = layer_for(&graph.root, parent)
        .expect("parent layer")
        .transform_to_parent;
    rotation.set(90.0);
    apply_updates(
        &mut composition,
        &mut graph,
        SceneUpdates {
            content: &[],
            layers: &[parent],
        },
    );
    let after = sink.get();
    assert_ne!(after, before, "the child's viewport moves with its parent");
    let fresh = fresh_graph(&mut composition, root);
    assert_eq!(
        sink.get(),
        after,
        "fresh lowering publishes the same window rect"
    );
    assert_ne!(
        layer_for(&graph.root, parent)
            .expect("updated parent layer")
            .transform_to_parent,
        before_transform
    );
    assert_eq!(
        layer_for(&graph.root, parent)
            .expect("updated parent layer")
            .transform_to_parent,
        layer_for(&fresh.root, parent)
            .expect("fresh parent layer")
            .transform_to_parent
    );
    assert_eq!(
        graph_output_owned(&graph.root),
        graph_output_owned(&fresh.root),
        "child text and viewport match fresh lowering"
    );
}

#[test]
fn parent_layer_and_child_content_dirt_are_both_applied() {
    clear_command_recordings_for_tests();
    let rotation = Rc::new(Cell::new(0.0_f32));
    let color = Rc::new(Cell::new(Color::RED));
    let parent_id = Rc::new(Cell::new(None::<NodeId>));
    let child_id = Rc::new(Cell::new(None::<NodeId>));
    let draw_rotation = Rc::clone(&rotation);
    let draw_color = Rc::clone(&color);
    let draw_parent_id = Rc::clone(&parent_id);
    let draw_child_id = Rc::clone(&child_id);
    let mut composition = run_test_composition(move || {
        let draw_color = Rc::clone(&draw_color);
        let draw_child_id = Rc::clone(&draw_child_id);
        RotatingParent(
            Rc::clone(&draw_rotation),
            Rc::clone(&draw_parent_id),
            move || {
                let canvas_color = Rc::clone(&draw_color);
                let child = Canvas(
                    Modifier::empty().size(Size::new(40.0, 24.0)),
                    move |scope| scope.draw_rect(Brush::solid(canvas_color.get())),
                );
                draw_child_id.set(Some(child));
            },
        );
    });

    let (root, mut graph) = initial_root_graph(&mut composition);
    let parent = parent_id.get().expect("parent Box id");
    let child = child_id.get().expect("Canvas id");
    let old_transform = layer_for(&graph.root, parent)
        .expect("parent layer")
        .transform_to_parent;
    rotation.set(43.0);
    color.set(Color::GREEN);
    apply_updates(
        &mut composition,
        &mut graph,
        SceneUpdates {
            content: &[child],
            layers: &[parent],
        },
    );
    assert_ne!(
        layer_for(&graph.root, parent)
            .expect("updated parent layer")
            .transform_to_parent,
        old_transform,
        "the parent's layer dirt is applied"
    );
    let fresh = fresh_graph(&mut composition, root);
    let actual = graph_output_owned(&graph.root);
    let expected = graph_output_owned(&fresh.root);
    assert_eq!(actual, expected, "child content survives parent layer dirt");
    assert!(
        actual.0.iter().any(|(_, _, drawn)| *drawn == Color::GREEN),
        "the child Canvas records its updated color"
    );
}

#[test]
fn parent_content_and_child_layer_dirt_match_a_fresh_rebuild() {
    clear_command_recordings_for_tests();
    let parent_color = Rc::new(Cell::new(Color::RED));
    let child_rotation = Rc::new(Cell::new(0.0_f32));
    let parent_id = Rc::new(Cell::new(None::<NodeId>));
    let child_id = Rc::new(Cell::new(None::<NodeId>));
    let draw_color = Rc::clone(&parent_color);
    let draw_rotation = Rc::clone(&child_rotation);
    let draw_parent_id = Rc::clone(&parent_id);
    let draw_child_id = Rc::clone(&child_id);
    let mut composition = run_test_composition(move || {
        let color = Rc::clone(&draw_color);
        let parent = Box(
            Modifier::empty()
                .size(Size::new(120.0, 80.0))
                .draw_behind(move |scope| scope.draw_rect(Brush::solid(color.get()))),
            BoxSpec::default(),
            {
                let rotation = Rc::clone(&draw_rotation);
                let draw_child_id = Rc::clone(&draw_child_id);
                move || {
                    let layer_rotation = Rc::clone(&rotation);
                    let child = Box(
                        Modifier::empty()
                            .size(Size::new(40.0, 24.0))
                            .graphics_layer(move || GraphicsLayer {
                                rotation_z: layer_rotation.get(),
                                ..GraphicsLayer::default()
                            }),
                        BoxSpec::default(),
                        || {
                            Text("child", Modifier::empty(), TextStyle::default());
                        },
                    );
                    draw_child_id.set(Some(child));
                }
            },
        );
        draw_parent_id.set(Some(parent));
    });

    let (root, mut graph) = initial_root_graph(&mut composition);
    let parent = parent_id.get().expect("parent Box id");
    let child = child_id.get().expect("child Box id");
    let old_transform = layer_for(&graph.root, child)
        .expect("child graphics layer")
        .transform_to_parent;
    parent_color.set(Color::BLUE);
    child_rotation.set(61.0);
    apply_updates(
        &mut composition,
        &mut graph,
        SceneUpdates {
            content: &[parent],
            layers: &[child],
        },
    );
    assert_ne!(
        layer_for(&graph.root, child)
            .expect("updated child layer")
            .transform_to_parent,
        old_transform,
        "parent content rebuilding must not lose the child's layer change"
    );
    let fresh = fresh_graph(&mut composition, root);
    let actual = graph_output_owned(&graph.root);
    let expected = graph_output_owned(&fresh.root);
    assert_eq!(
        actual, expected,
        "content-dirty parent matches a fresh graph"
    );
    assert!(
        actual.0.iter().any(|(_, _, color)| *color == Color::BLUE),
        "the parent draw refreshes its changed color"
    );
}
