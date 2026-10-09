//! A text whose string changes, and nothing else about it, as a ticker's
//! does, draws its new string after a scene update, as a fresh scene does.

use cranpose_core::MutableState;
use cranpose_render_common::{
    graph::{LayerNode, PrimitiveNode, RenderGraph, RenderNode},
    text_paint::TextPaint,
};
use cranpose_ui::{
    Color, Column, ColumnSpec, Modifier, Size, Text, TextStyle, run_test_composition,
    text::SpanStyle,
};

use crate::scene_probe::{fresh_graph, initial_graph, painted_text, update_scene};

const VIEWPORT: Size = Size::new(240.0, 120.0);

fn painted(graph: &RenderGraph) -> Vec<String> {
    let mut out = Vec::new();
    painted_text(&graph.root, &mut out);
    out
}

#[test]
fn a_text_that_changes_only_its_string_draws_each_new_string() {
    let text = std::rc::Rc::new(std::cell::Cell::new(None::<MutableState<&'static str>>));
    let slot = std::rc::Rc::clone(&text);
    let mut composition = run_test_composition(move || {
        let slot = std::rc::Rc::clone(&slot);
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            let price = cranpose_core::rememberMutableStateOf(|| "$100.00");
            slot.set(Some(price));
            Text(
                price.get(),
                Modifier::empty().padding(4.0),
                TextStyle::default(),
            );
            Text("static", Modifier::empty(), TextStyle::default());
        });
    });
    let root = composition.root().expect("root");
    let mut graph = initial_graph(&mut composition, root, VIEWPORT);
    let price = text.get().expect("price state");
    // Back to an earlier string too: the node must not keep the one before.
    for value in ["$101.50", "$100.00", "$99.25"] {
        price.set(value);
        update_scene(&mut composition, root, VIEWPORT, &mut graph);
        let drawn = painted(&graph);
        assert!(
            drawn.iter().any(|text| text == value),
            "the scene draws {value}: {drawn:?}"
        );
        assert_eq!(
            drawn,
            painted(&fresh_graph(&mut composition, root)),
            "the updated scene draws what a fresh one does"
        );
    }
}

fn text_paints(layer: &LayerNode, out: &mut Vec<(String, TextPaint)>) {
    for child in &layer.children {
        match child {
            RenderNode::Layer(child) => text_paints(child, out),
            RenderNode::Primitive(entry) => {
                if let PrimitiveNode::Text(text) = &entry.node {
                    assert_eq!(
                        text.paint().style_hash,
                        text.text_style.render_hash(),
                        "{} paints under its own style's hash",
                        text.text.text
                    );
                    out.push((text.text.text.clone(), text.paint()));
                }
            }
            RenderNode::DrawRun(_) => {}
        }
    }
}

fn painted_with_paint(graph: &RenderGraph) -> Vec<(String, TextPaint)> {
    let mut out = Vec::new();
    text_paints(&graph.root, &mut out);
    out
}

#[test]
fn a_text_restyled_or_changed_in_place_paints_as_a_fresh_scene_does() {
    const RED: Color = Color(0.8, 0.1, 0.1, 1.0);
    const GREEN: Color = Color(0.1, 0.6, 0.2, 1.0);
    type Price = (MutableState<&'static str>, MutableState<Color>);
    let states = std::rc::Rc::new(std::cell::Cell::new(None::<Price>));
    let slot = std::rc::Rc::clone(&states);
    let mut composition = run_test_composition(move || {
        let slot = std::rc::Rc::clone(&slot);
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            let price = cranpose_core::rememberMutableStateOf(|| "$100.00");
            let color = cranpose_core::rememberMutableStateOf(|| RED);
            slot.set(Some((price, color)));
            Text(
                price.get(),
                Modifier::empty(),
                TextStyle::from_span_style(SpanStyle {
                    color: Some(color.get()),
                    ..SpanStyle::default()
                }),
            );
        });
    });
    let root = composition.root().expect("root");
    let mut graph = initial_graph(&mut composition, root, VIEWPORT);
    let (price, color) = states.get().expect("price states");
    for (value, tint) in [
        ("$100.00", GREEN),
        ("$99.25", GREEN),
        ("$98.00", RED),
        ("$98.00", GREEN),
    ] {
        price.set(value);
        color.set(tint);
        update_scene(&mut composition, root, VIEWPORT, &mut graph);
        let painted = painted_with_paint(&graph);
        assert_eq!(painted.len(), 1);
        assert_eq!(painted[0].0, value);
        assert_eq!(painted[0].1.color, tint, "{value}");
        assert_eq!(
            painted,
            painted_with_paint(&fresh_graph(&mut composition, root)),
            "the updated scene paints what a fresh one does"
        );
    }
}
