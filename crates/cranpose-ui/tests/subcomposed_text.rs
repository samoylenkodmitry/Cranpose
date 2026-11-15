// Text a subcomposition composes lays out in the frame it composes in: on
// the first frame, and when a recomposition adds it under a parent the
// subcomposition holds.

use std::{cell::Cell, rc::Rc};

use cranpose_core::MutableState;
use cranpose_ui::{
    BoxWithConstraints, Column, ColumnSpec, LayoutBox, Modifier, Size, Text, TextStyle,
    run_test_composition,
};

use crate::text_measured_layout_integration::layout_composition;

fn find_text<'a>(node: &'a LayoutBox, text: &str) -> Option<&'a LayoutBox> {
    if node.node_data.modifier_slices().text_content().as_deref() == Some(text) {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_text(child, text))
}

#[test]
fn text_a_subcomposition_composes_lays_out_in_its_frame() {
    let viewport = Size::new(200.0, 200.0);
    let shown: Rc<Cell<Option<MutableState<bool>>>> = Rc::default();
    let handle = Rc::clone(&shown);
    let mut composition = run_test_composition(move || {
        let more =
            cranpose_core::remember(|| cranpose_core::mutableStateOf(false)).with(|state| *state);
        handle.set(Some(more));
        BoxWithConstraints(Modifier::empty(), move |_| {
            // A slot's own text, with no parent in the slot.
            Text("slot", Modifier::empty(), TextStyle::default());
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                Text("first", Modifier::empty(), TextStyle::default());
                if more.value() {
                    Text("second", Modifier::empty(), TextStyle::default());
                }
            });
            if more.value() {
                Text("added slot", Modifier::empty(), TextStyle::default());
            }
        });
    });
    let layout = layout_composition(&mut composition, viewport);
    for text in ["slot", "first"] {
        let laid = find_text(layout.root(), text).expect("a text of the first frame");
        assert!(laid.rect.height > 0.0, "{text}: {:?}", laid.rect);
    }
    let first = find_text(layout.root(), "first").expect("the first text");
    let first_bottom = first.rect.y + first.rect.height;

    shown.get().expect("the state").set(true);
    while composition.process_invalid_scopes().expect("recompose") {}
    let layout = layout_composition(&mut composition, viewport);
    for text in ["second", "added slot"] {
        let laid = find_text(layout.root(), text).expect("an added text");
        assert!(laid.rect.height > 0.0, "{text}: {:?}", laid.rect);
    }
    let second = find_text(layout.root(), "second").expect("the added text");
    assert!(
        second.rect.y >= first_bottom,
        "the added text sits below the first: {:?}",
        second.rect
    );
}
