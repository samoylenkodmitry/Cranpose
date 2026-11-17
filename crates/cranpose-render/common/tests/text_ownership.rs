// A text's string travels from the app's call to the scene as one string:
// the scene draws the room of an owned string the app gave and the shared
// content of an annotated string, and a borrowed static string is copied
// once, into the string the node and the scene share.

use std::{cell::Cell, rc::Rc, sync::Arc};

use cranpose_core::{MutableState, NodeId, rememberMutableStateOf};
use cranpose_render_common::{
    graph_scene::Scene,
    scene_store::{ItemRef, LayerRef},
};
use cranpose_ui::{
    Color, LayoutNode, Modifier, Size, TestComposition, Text, TextStyle, run_test_composition,
    text::{AnnotatedString, RenderString, SpanStyle},
};

use crate::scene_probe::{initial_graph, with_applier};

const VIEWPORT: Size = Size::new(240.0, 120.0);

// The string of the one text `layer` and the records beneath it draw, and
// the node that shows it.
fn drawn_text(layer: LayerRef<'_>) -> Option<(NodeId, Arc<RenderString>)> {
    layer.items().find_map(|item| match item {
        ItemRef::Text(text) => Some((text.node_id, Arc::clone(&text.render_text))),
        ItemRef::Layer(child) | ItemRef::Leaf(child) => drawn_text(child),
        ItemRef::Run(_) | ItemRef::Draw(_) | ItemRef::Shape(_) => None,
    })
}

// The string the scene of `composition` draws, after checking that the
// text node shows the same one.
fn scene_text(composition: &mut TestComposition) -> Arc<RenderString> {
    let root = composition.root().expect("a composed root");
    let scene: Scene = initial_graph(composition, root, VIEWPORT);
    let (node, drawn) = drawn_text(scene.root_layer().expect("a scene")).expect("a text");
    let shown = with_applier(composition, |applier| {
        applier
            .with_node::<LayoutNode, _>(node, |node| {
                let slices = node.modifier_slices_snapshot();
                let text = slices.annotated_text().expect("the node shows a text");
                Arc::clone(text.render_string())
            })
            .expect("the text's node")
    });
    assert!(
        Arc::ptr_eq(&drawn, &shown),
        "the scene draws the node's string, not a copy"
    );
    drawn
}

// A composition of one `Text` of `text`, given once.
fn text_composition<S>(text: S) -> TestComposition
where
    S: cranpose_ui::widgets::IntoTextSource + Clone + PartialEq + 'static,
{
    let given = Rc::new(Cell::new(Some(text)));
    run_test_composition(move || {
        if let Some(text) = given.take() {
            Text(text, Modifier::empty(), TextStyle::default());
        }
    })
}

#[test]
fn an_owned_string_reaches_the_scene_in_its_own_room() {
    let text = String::from("owned label");
    let room = text.as_ptr();
    let drawn = scene_text(&mut text_composition(text));
    assert_eq!(drawn.text(), "owned label");
    assert_eq!(
        drawn.text().as_ptr(),
        room,
        "the scene draws the bytes of the app's string"
    );
}

#[test]
fn an_annotated_string_reaches_the_scene_shared() {
    let text = AnnotatedString::builder()
        .append("plain ")
        .push_style(SpanStyle {
            color: Some(Color::RED),
            ..SpanStyle::default()
        })
        .append("red")
        .pop()
        .to_annotated_string();
    let content = Arc::clone(text.render_string());
    let drawn = scene_text(&mut text_composition(text));
    assert!(
        Arc::ptr_eq(&drawn, &content),
        "the scene draws the app's annotated string, spans included"
    );
    assert_eq!(drawn.span_styles().len(), 1);
}

// A text whose string changes each frame, as a counter's, keeps drawing the
// bytes of the string the app gave it, though it takes the shared block of
// a string it showed before.
#[test]
fn a_changed_string_reaches_the_scene_in_its_own_room() {
    let given = Rc::new(Cell::new(Some(String::from("label 0"))));
    let frame: Rc<Cell<Option<MutableState<u32>>>> = Rc::default();
    let (shown, kept) = (Rc::clone(&given), Rc::clone(&frame));
    let mut composition = run_test_composition(move || {
        let frame = rememberMutableStateOf(|| 0u32);
        kept.set(Some(frame));
        let count = frame.get();
        let label = shown.take().unwrap_or_else(|| format!("label {count}"));
        Text(label, Modifier::empty(), TextStyle::default());
    });
    let frame = frame.get().expect("the frame state");
    for count in 1..=6 {
        let text = format!("label {count}");
        let room = text.as_ptr();
        given.set(Some(text));
        frame.set(count);
        while composition.process_invalid_scopes().expect("recompose") {}
        let drawn = scene_text(&mut composition);
        assert_eq!(drawn.text(), format!("label {count}"));
        assert_eq!(
            drawn.text().as_ptr(),
            room,
            "frame {count} draws the bytes of the app's string"
        );
    }
}

#[test]
fn a_static_string_is_copied_once() {
    let drawn = scene_text(&mut text_composition("static label"));
    assert_eq!(drawn.text(), "static label");
}
