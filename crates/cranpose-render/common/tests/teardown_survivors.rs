//! Texts that stay while most of the tree goes, as a tab row does when the
//! user leaves a tab of thousands of nodes, paint as before: the applier
//! moves such survivors onto fresh storage, and they measure and draw there
//! on the grid and in the direction they were composed in.

use std::{cell::Cell, rc::Rc};

use cranpose_core::MutableState;
use cranpose_render_common::graph::{LayerNode, PrimitiveNode, RenderGraph, RenderNode};
use cranpose_ui::{
    Box as UiBox, BoxSpec, Color, Column, ColumnSpec, LayoutDirection, Modifier,
    ProvideLayoutDirection, Row, RowSpec, Size, Spacer, Text, TextStyle,
    density::{Density, ProvideDensity},
    run_test_composition,
};

use crate::scene_probe::{fresh_graph, initial_graph, update_scene};

const VIEWPORT: Size = Size::new(800.0, 600.0);
const LABELS: [&str; 3] = ["Counter App", "Recursive Layout", "Glass Tiles"];
const DROPPED_NODES: usize = 17 * 1024;

type Controls = (MutableState<usize>, MutableState<bool>);

fn geometry(layer: &LayerNode, out: &mut Vec<String>) {
    out.push(format!(
        "layer {:?} at {:?} bounds {:?} content {:?}",
        layer.node_id, layer.origin_in_parent, layer.local_bounds, layer.content_offset
    ));
    for child in &layer.children {
        match child {
            RenderNode::Layer(child) => geometry(child, out),
            RenderNode::Primitive(entry) => {
                if let PrimitiveNode::Text(text) = &entry.node {
                    out.push(format!("text {:?} at {:?}", text.text.text, text.rect));
                }
            }
            RenderNode::DrawRun(_) => {}
        }
    }
}

fn painted(graph: &RenderGraph) -> Vec<String> {
    let mut out = Vec::new();
    geometry(&graph.root, &mut out);
    out
}

#[test]
fn texts_that_outlive_a_large_teardown_paint_as_before() {
    let controls = Rc::new(Cell::new(None::<Controls>));
    let slot = Rc::clone(&controls);
    let mut composition = run_test_composition(move || {
        let slot = Rc::clone(&slot);
        ProvideDensity(Density::new(2.0, 1.0), || {
            ProvideLayoutDirection(LayoutDirection::Rtl, || {
                Column(Modifier::empty(), ColumnSpec::default(), move || {
                    let selected = cranpose_core::rememberMutableStateOf(|| 0usize);
                    let deep = cranpose_core::rememberMutableStateOf(|| false);
                    slot.set(Some((selected, deep)));
                    Row(Modifier::empty(), RowSpec::default(), move || {
                        for (index, label) in LABELS.into_iter().enumerate() {
                            let active = selected.get() == index;
                            UiBox(
                                Modifier::empty()
                                    .background(if active {
                                        Color(0.2, 0.45, 0.9, 1.0)
                                    } else {
                                        Color(0.3, 0.3, 0.3, 0.5)
                                    })
                                    .padding(10.0),
                                BoxSpec::default(),
                                move || {
                                    Text(
                                        label,
                                        Modifier::empty().offset(3.0, 0.25).padding(4.0),
                                        TextStyle::default(),
                                    );
                                },
                            );
                        }
                    });
                    if deep.get() {
                        Column(Modifier::empty(), ColumnSpec::default(), || {
                            for _ in 0..DROPPED_NODES {
                                Spacer(Modifier::empty().size_points(4.0, 1.0));
                            }
                        });
                    }
                });
            });
        });
    });
    let root = composition.root().expect("root");
    let mut graph = initial_graph(&mut composition, root, VIEWPORT);
    let before = painted(&graph);
    assert_eq!(graph.root.painted_text(), LABELS);
    let (selected, deep) = controls.get().expect("controls");

    for round in 1..=2 {
        selected.set(1);
        deep.set(true);
        update_scene(&mut composition, root, VIEWPORT, &mut graph);
        selected.set(0);
        deep.set(false);
        update_scene(&mut composition, root, VIEWPORT, &mut graph);

        assert_eq!(
            graph.root.painted_text(),
            LABELS,
            "round {round}: the labels paint after the teardown"
        );
        assert_eq!(
            painted(&graph),
            before,
            "round {round}: the labels paint where they did before the teardown"
        );
        assert_eq!(
            painted(&fresh_graph(&mut composition, root)),
            before,
            "round {round}: a fresh scene paints them there too"
        );
    }
}
