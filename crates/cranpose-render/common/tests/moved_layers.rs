//! A node the layout pass only moves keeps what its layer drew: the scene
//! update moves the layer, as the app shell routes such nodes.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{MemoryApplier, MutableState, NodeId};
use cranpose_render_common::{
    SceneUpdates,
    graph::{LayerNode, PrimitiveNode, ProjectiveTransform, RenderGraph, RenderNode},
    scene_builder::{
        build_graph_from_applier, clear_command_recordings_for_tests, update_graph_from_applier,
    },
};
use cranpose_ui::{
    Box, BoxSpec, Brush, Canvas, Color, Column, ColumnSpec, LayoutEngine, Modifier, Size,
    TestComposition, Text, TextStyle, composable, run_test_composition,
};
use cranpose_ui_graphics::{DrawPrimitive, Rect};

const VIEWPORT: Size = Size::new(240.0, 240.0);

/// What a test reads back from its composition.
#[derive(Default)]
struct Probe {
    spacer_height: Option<MutableState<f32>>,
    canvas_color: Option<MutableState<Color>>,
    canvas: Option<NodeId>,
    label_window_rect: Option<MutableState<Rect>>,
}

/// A box as tall as `height`, read in its own scope: a new height
/// recomposes only the spacer, as the gauntlet's tickers do.
#[composable]
fn Spacer(height: MutableState<f32>) {
    Box(
        Modifier::empty().size(Size::new(40.0, height.get())),
        BoxSpec::default(),
        || {},
    );
}

/// A column whose spacer pushes down a canvas and a label that reports its
/// window rect; `draws` counts the canvas's draw calls.
fn moving_column(probe: Rc<RefCell<Probe>>, draws: Rc<Cell<usize>>) -> TestComposition {
    run_test_composition(move || {
        let probe = Rc::clone(&probe);
        let draws = Rc::clone(&draws);
        Column(
            Modifier::empty().size(Size::new(200.0, 200.0)),
            ColumnSpec::default(),
            move || {
                let height = cranpose_core::rememberMutableStateOf(|| 20.0_f32);
                let color = cranpose_core::rememberMutableStateOf(|| Color::RED);
                let window_rect =
                    cranpose_core::rememberMutableStateOf(|| Rect::from_size(Size::ZERO));
                Spacer(height);
                let draws = Rc::clone(&draws);
                let canvas = Canvas(
                    Modifier::empty().size(Size::new(40.0, 24.0)),
                    move |scope| {
                        draws.set(draws.get() + 1);
                        scope.draw_rect(Brush::solid(color.get()));
                    },
                );
                Text(
                    "moved label",
                    Modifier::empty()
                        .size(Size::new(80.0, 20.0))
                        .report_window_rect_state(window_rect),
                    TextStyle::default(),
                );
                *probe.borrow_mut() = Probe {
                    spacer_height: Some(height),
                    canvas_color: Some(color),
                    canvas: Some(canvas),
                    label_window_rect: Some(window_rect),
                };
            },
        );
    })
}

fn with_applier<R>(
    composition: &mut TestComposition,
    read: impl FnOnce(&mut MemoryApplier) -> R,
) -> R {
    let runtime = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(runtime);
    let result = read(&mut applier);
    applier.clear_runtime_handle();
    result
}

fn initial_graph(composition: &mut TestComposition, root: NodeId) -> RenderGraph {
    with_applier(composition, |applier| {
        applier.compute_layout(root, VIEWPORT).expect("layout");
        let graph = build_graph_from_applier(applier, root, 1.0).expect("initial graph");
        let _ = cranpose_ui::take_geometry_scene_nodes();
        let _ = cranpose_ui::take_draw_repass_nodes();
        let _ = applier.take_structural_change_parents_attached_to(root);
        graph
    })
}

/// Recomposes, lays out and updates `graph` with the nodes the app shell
/// would route: content for reshaped, redrawn and restructured nodes, and
/// the moved ones apart. Returns the moved nodes.
fn update_frame(
    composition: &mut TestComposition,
    root: NodeId,
    graph: &mut RenderGraph,
) -> Vec<NodeId> {
    while composition.process_invalid_scopes().expect("recompose") {}
    with_applier(composition, |applier| {
        applier.compute_layout(root, VIEWPORT).expect("relayout");
        let geometry = cranpose_ui::take_geometry_scene_nodes();
        let mut content = geometry.reshaped;
        content.extend(cranpose_ui::take_draw_repass_nodes());
        content.extend(applier.take_structural_change_parents_attached_to(root));
        content.sort_unstable();
        content.dedup();
        let moved = geometry.moved;
        assert!(
            update_graph_from_applier(
                applier,
                graph,
                SceneUpdates {
                    content: &content,
                    layers: &[],
                    moved: &moved,
                },
                1.0,
            ),
            "the scoped update applies"
        );
        moved
    })
}

fn fresh_graph(composition: &mut TestComposition, root: NodeId) -> RenderGraph {
    with_applier(composition, |applier| {
        build_graph_from_applier(applier, root, 1.0).expect("fresh graph")
    })
}

/// Every solid rectangle and text the graph paints, in window space.
fn painted(layer: &LayerNode, parent: ProjectiveTransform, out: &mut Vec<(Rect, String)>) {
    let transform = layer.transform_to_parent.then(parent);
    for child in &layer.children {
        match child {
            RenderNode::Layer(child) => painted(child, transform, out),
            RenderNode::DrawRun(run) => {
                out.extend(run.primitives().filter_map(|primitive| match primitive {
                    DrawPrimitive::Rect {
                        rect,
                        brush: Brush::Solid(color),
                        ..
                    } => Some((transform.bounds_for_rect(rect), format!("{color:?}"))),
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
                        out.push((transform.bounds_for_rect(*rect), format!("{color:?}")));
                    }
                }
                PrimitiveNode::Text(text) => {
                    out.push((transform.bounds_for_rect(text.rect), text.text.text.clone()));
                }
            },
        }
    }
}

fn painted_output(graph: &RenderGraph) -> Vec<(Rect, String)> {
    let mut out = Vec::new();
    painted(&graph.root, ProjectiveTransform::identity(), &mut out);
    out
}

#[test]
fn a_node_that_only_moved_keeps_what_it_drew() {
    clear_command_recordings_for_tests();
    let probe = Rc::new(RefCell::new(Probe::default()));
    let draws = Rc::new(Cell::new(0));
    let mut composition = moving_column(Rc::clone(&probe), Rc::clone(&draws));
    let root = composition.root().expect("root");
    let mut graph = initial_graph(&mut composition, root);
    let (height, canvas, window_rect) = {
        let probe = probe.borrow();
        (
            probe.spacer_height.expect("spacer height"),
            probe.canvas.expect("canvas"),
            probe.label_window_rect.expect("label window rect"),
        )
    };
    let drawn = draws.get();
    let label_before = window_rect.get();

    height.set(50.0);
    let moved = update_frame(&mut composition, root, &mut graph);

    assert!(moved.contains(&canvas), "the canvas only moved: {moved:?}");
    assert_eq!(draws.get(), drawn, "a moved canvas keeps its recording");
    let label_after = window_rect.get();
    assert_eq!(
        label_after.y - label_before.y,
        30.0,
        "the moved label publishes its new window rect"
    );
    let fresh = fresh_graph(&mut composition, root);
    assert_eq!(window_rect.get(), label_after, "a fresh scene agrees");
    assert_eq!(
        painted_output(&graph),
        painted_output(&fresh),
        "the moved scene paints what a fresh one does"
    );
}

#[test]
fn a_node_that_moved_and_changed_what_it_draws_draws_again() {
    clear_command_recordings_for_tests();
    let probe = Rc::new(RefCell::new(Probe::default()));
    let draws = Rc::new(Cell::new(0));
    let mut composition = moving_column(Rc::clone(&probe), Rc::clone(&draws));
    let root = composition.root().expect("root");
    let mut graph = initial_graph(&mut composition, root);
    let (height, color) = {
        let probe = probe.borrow();
        (
            probe.spacer_height.expect("spacer height"),
            probe.canvas_color.expect("canvas color"),
        )
    };
    let drawn = draws.get();

    height.set(50.0);
    color.set(Color::GREEN);
    update_frame(&mut composition, root, &mut graph);

    assert_eq!(draws.get(), drawn + 1, "the canvas draws its new color");
    let fresh = fresh_graph(&mut composition, root);
    assert_eq!(
        painted_output(&graph),
        painted_output(&fresh),
        "the scene paints the moved canvas in its new color"
    );
}
