//! A text whose string changes, and nothing else about it, as a ticker's
//! does, draws its new string after a scene update, as a fresh scene does.

use cranpose_core::MutableState;
use cranpose_render_common::graph::RenderGraph;
use cranpose_ui::{Column, ColumnSpec, Modifier, Size, Text, TextStyle, run_test_composition};

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
