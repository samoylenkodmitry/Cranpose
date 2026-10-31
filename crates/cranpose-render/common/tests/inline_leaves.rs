// Leaves without layer properties of their own draw in their parent's
// layer. A scene update that gives such a leaf a layer, takes it away,
// gives it a child or changes what it draws leaves the scene as a fresh
// build makes it, and a cached layer above the leaf sees the change.

use std::cell::Cell;

use cranpose_core::MutableState;
use cranpose_render_common::graph::{
    LayerNode, PrimitiveNode, ProjectiveTransform, RenderGraph, RenderNode,
};
use cranpose_ui::{
    Box, BoxSpec, Color, Column, ColumnSpec, Modifier, Size, Text, TextStyle, run_test_composition,
};
use cranpose_ui_graphics::{CompositingStrategy, DrawPrimitive, GraphicsLayer};

use crate::scene_probe::{fresh_graph, initial_graph, update_scene};

const VIEWPORT: Size = Size::new(240.0, 160.0);
const FILL: Color = Color(0.3, 0.5, 0.9, 1.0);

// Each node the scene draws, whether it draws through a layer of its own or
// in its parent's, where it lands in the window, and each rectangle and text
// it paints there.
fn picture(graph: &RenderGraph) -> Vec<String> {
    fn nodes(children: &[RenderNode], transform: ProjectiveTransform, out: &mut Vec<String>) {
        for child in children {
            match child {
                RenderNode::Layer(layer) => walk_layer(layer, transform, out),
                RenderNode::Inline(inline) => {
                    let transform = inline.transform_to_parent().then(transform);
                    out.push(format!(
                        "inline {:?} at {:?}",
                        inline.node_id,
                        transform.bounds_for_rect(inline.node_rect())
                    ));
                    nodes(&inline.content, transform, out);
                }
                RenderNode::DrawRun(run) => {
                    for primitive in run.primitives() {
                        if let DrawPrimitive::Rect { rect, brush, .. } = primitive {
                            out.push(format!(
                                "rect {:?} {brush:?}",
                                transform.bounds_for_rect(rect)
                            ));
                        }
                    }
                }
                RenderNode::Primitive(entry) => {
                    if let PrimitiveNode::Text(text) = &entry.node {
                        out.push(format!(
                            "text {} at {:?}",
                            text.text.text,
                            transform.bounds_for_rect(text.rect)
                        ));
                    }
                }
            }
        }
    }
    fn walk_layer(layer: &LayerNode, parent: ProjectiveTransform, out: &mut Vec<String>) {
        let transform = layer.transform_to_parent.then(parent);
        out.push(format!(
            "layer {:?} at {:?} alpha {}",
            layer.node_id,
            transform.bounds_for_rect(layer.node_rect()),
            layer.opacity()
        ));
        nodes(&layer.children, transform, out);
    }
    let mut out = Vec::new();
    walk_layer(&graph.root, ProjectiveTransform::identity(), &mut out);
    out
}

// Whether the scene draws `text` through a layer of its own.
fn text_has_layer(graph: &RenderGraph, text: &str) -> bool {
    fn search(children: &[RenderNode], text: &str, in_layer: bool) -> Option<bool> {
        children.iter().find_map(|child| match child {
            RenderNode::Layer(layer) => search(&layer.children, text, layer.node_id.is_some()),
            RenderNode::Inline(inline) => search(&inline.content, text, false),
            RenderNode::Primitive(entry) => match &entry.node {
                PrimitiveNode::Text(drawn) if drawn.text.text == text => Some(in_layer),
                _ => None,
            },
            RenderNode::DrawRun(_) => None,
        })
    }
    search(&graph.root.children, text, true).expect("the text draws")
}

#[test]
fn a_label_that_takes_and_drops_a_layer_draws_as_a_fresh_scene_does() {
    // Whether the label is faded, and the box's width.
    type States = (MutableState<bool>, MutableState<f32>);
    let states: std::rc::Rc<Cell<Option<States>>> = std::rc::Rc::new(Cell::new(None));
    let slot = std::rc::Rc::clone(&states);
    let mut composition = run_test_composition(move || {
        let faded = cranpose_core::rememberMutableStateOf(|| false);
        let width = cranpose_core::rememberMutableStateOf(|| 80.0_f32);
        slot.set(Some((faded, width)));
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            let label = if faded.get() {
                Modifier::empty().graphics_layer_value(GraphicsLayer {
                    alpha: 0.5,
                    ..GraphicsLayer::default()
                })
            } else {
                Modifier::empty()
            };
            Text("label", label, TextStyle::default());
            Box(
                Modifier::empty()
                    .size_points(width.get(), 20.0)
                    .background(FILL),
                BoxSpec::default(),
                || {},
            );
            Text("below", Modifier::empty(), TextStyle::default());
        });
    });
    let root = composition.root().expect("root");
    let mut graph = initial_graph(&mut composition, root, VIEWPORT);
    assert!(
        !text_has_layer(&graph, "label"),
        "a label without layer properties draws in its parent's layer"
    );
    let (faded, width) = states.get().expect("states");
    for (fade, wide) in [(true, 80.0), (true, 120.0), (false, 120.0), (false, 60.0)] {
        faded.set(fade);
        width.set(wide);
        update_scene(&mut composition, root, VIEWPORT, &mut graph);
        assert_eq!(
            text_has_layer(&graph, "label"),
            fade,
            "the label draws through a layer exactly while it is faded"
        );
        assert_eq!(
            picture(&graph),
            picture(&fresh_graph(&mut composition, root)),
            "faded {fade}, box {wide} wide: the updated scene is the one a fresh build makes"
        );
    }
}

#[test]
fn a_leaf_that_gains_and_loses_a_child_draws_as_a_fresh_scene_does() {
    let state: std::rc::Rc<Cell<Option<MutableState<bool>>>> = std::rc::Rc::new(Cell::new(None));
    let slot = std::rc::Rc::clone(&state);
    let mut composition = run_test_composition(move || {
        let nested = cranpose_core::rememberMutableStateOf(|| false);
        slot.set(Some(nested));
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            Box(
                Modifier::empty().size_points(100.0, 40.0).background(FILL),
                BoxSpec::default(),
                move || {
                    if nested.get() {
                        Text("inside", Modifier::empty(), TextStyle::default());
                    }
                },
            );
            Text("after", Modifier::empty(), TextStyle::default());
        });
    });
    let root = composition.root().expect("root");
    let mut graph = initial_graph(&mut composition, root, VIEWPORT);
    let nested = state.get().expect("state");
    for show in [true, false, true] {
        nested.set(show);
        update_scene(&mut composition, root, VIEWPORT, &mut graph);
        assert_eq!(
            picture(&graph),
            picture(&fresh_graph(&mut composition, root)),
            "child shown {show}: the updated scene is the one a fresh build makes"
        );
    }
}

#[test]
fn a_cached_layer_misses_when_a_leaf_it_draws_changes_its_text() {
    let state: std::rc::Rc<Cell<Option<MutableState<&'static str>>>> =
        std::rc::Rc::new(Cell::new(None));
    let slot = std::rc::Rc::clone(&state);
    let mut composition = run_test_composition(move || {
        let label = cranpose_core::rememberMutableStateOf(|| "alpha");
        slot.set(Some(label));
        Column(
            Modifier::empty().graphics_layer_value(GraphicsLayer {
                compositing_strategy: CompositingStrategy::Offscreen,
                ..GraphicsLayer::default()
            }),
            ColumnSpec::default(),
            move || {
                Text(label.get(), Modifier::empty(), TextStyle::default());
                Text("stays", Modifier::empty(), TextStyle::default());
            },
        );
    });
    let root = composition.root().expect("root");
    let mut graph = initial_graph(&mut composition, root, VIEWPORT);
    let cached = |graph: &RenderGraph| {
        fn find(layer: &LayerNode) -> Option<&LayerNode> {
            if layer.isolation.explicit_offscreen {
                return Some(layer);
            }
            layer.children.iter().find_map(|child| match child {
                RenderNode::Layer(child) => find(child),
                _ => None,
            })
        }
        find(&graph.root)
            .expect("the offscreen column")
            .target_content_hash()
    };
    let before = cached(&graph);
    let label = state.get().expect("state");
    // A text of the same width: only what the leaf draws changes.
    label.set("omega");
    update_scene(&mut composition, root, VIEWPORT, &mut graph);
    assert_ne!(
        cached(&graph),
        before,
        "the column's cached pixels hold the old text, so its key changes with it"
    );
    label.set("alpha");
    update_scene(&mut composition, root, VIEWPORT, &mut graph);
    assert_eq!(
        cached(&graph),
        before,
        "the same text gives the same key, so the cache can hit again"
    );
    assert_eq!(
        picture(&graph),
        picture(&fresh_graph(&mut composition, root))
    );
}
