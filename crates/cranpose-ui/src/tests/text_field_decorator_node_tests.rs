use std::cell::Cell;

use cranpose_core::{Composition, MemoryApplier, location_key};
use cranpose_foundation::{PointerEventKind, text::TextRange};
use cranpose_ui_graphics::{Point, Size};

use super::*;
use crate::{
    focus_order::collect_focus_order,
    layout::{LayoutBox, LayoutEngine, LayoutTree},
    modifier::Modifier,
    text_field_focus::{clear_focus, focused_field_node, focused_field_target, has_focused_field},
    widgets::{
        BoxSpec,
        basic_text_field::{BasicTextFieldDecorated, BasicTextFieldOptions},
        box_widget::Box,
    },
};

/// A decorated "hello" field: 10 of padding on the decoration box, and
/// the inner field 5 further in.
struct DecoratedField {
    _composition: Composition<MemoryApplier>,
    state: TextFieldState,
    layout: LayoutTree,
    decoration: NodeId,
    inner: NodeId,
}

fn decorated_field() -> DecoratedField {
    let mut composition = Composition::new(MemoryApplier::new());
    let state = TextFieldState::new("hello");
    let decoration = Rc::new(Cell::new(None));
    let inner = Rc::new(Cell::new(None));
    let mut content = {
        let decoration = Rc::clone(&decoration);
        let inner = Rc::clone(&inner);
        move || {
            let inner = Rc::clone(&inner);
            decoration.set(Some(BasicTextFieldDecorated(
                state,
                Modifier::empty().padding(10.0),
                BasicTextFieldOptions::default(),
                move |field| {
                    let inner = Rc::clone(&inner);
                    Box(
                        Modifier::empty().padding(5.0),
                        BoxSpec::default(),
                        move || inner.set(Some(field.inner_text_field())),
                    );
                },
            )));
        }
    };
    let key = location_key(file!(), line!(), column!());
    composition.render(key, &mut content).expect("render");
    let root = composition.root().expect("root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let layout = applier
        .compute_layout(root, Size::new(400.0, 200.0))
        .expect("layout");
    applier.clear_runtime_handle();
    drop(applier);
    DecoratedField {
        _composition: composition,
        state,
        layout,
        decoration: decoration.get().expect("decoration composed"),
        inner: inner.get().expect("inner field composed"),
    }
}

fn find_in(node: &LayoutBox, id: NodeId) -> Option<&LayoutBox> {
    if node.node_id == id {
        return Some(node);
    }
    node.children.iter().find_map(|child| find_in(child, id))
}

impl DecoratedField {
    fn find(&self, id: NodeId) -> &LayoutBox {
        find_in(self.layout.root(), id).expect("node placed in the layout")
    }

    /// Presses the decoration box at `(x, y)` in its own space.
    fn press(&self, x: f32, y: f32) {
        let decoration = self.find(self.decoration);
        let handler = decoration
            .node_data
            .modifier_slices()
            .pointer_inputs()
            .first()
            .cloned()
            .expect("the decoration box takes the pointer");
        let local = Point { x, y };
        let global = Point {
            x: decoration.rect.x + x,
            y: decoration.rect.y + y,
        };
        handler(PointerEvent::new(PointerEventKind::Down, local, global));
        handler(PointerEvent::new(PointerEventKind::Up, local, global));
    }
}

#[test]
fn a_press_on_the_decoration_edits_the_field_where_the_text_is() {
    let _app_context = crate::render_state::app_context_test_scope();
    let field = decorated_field();
    let inner = field.find(field.inner).rect;
    assert_eq!((inner.x, inner.y), (15.0, 15.0));

    field.press(field.find(field.decoration).rect.width - 1.0, 1.0);
    assert!(
        has_focused_field(),
        "a press in the padding focuses the field"
    );
    assert_eq!(field.state.selection(), TextRange::cursor(5));

    field.press(inner.x + 1.0, inner.y + 1.0);
    assert_eq!(
        field.state.selection(),
        TextRange::cursor(0),
        "a press at the text's start puts the caret there, not a padding further on"
    );
    clear_focus();
}

#[test]
fn the_decoration_is_the_one_node_focus_and_accessibility_know() {
    let _app_context = crate::render_state::app_context_test_scope();
    let field = decorated_field();
    let decoration = field.find(field.decoration);
    let inner = field.find(field.inner);
    assert!(
        decoration
            .node_data
            .semantics()
            .is_some_and(|config| config.is_editable_text)
    );
    assert!(
        inner
            .node_data
            .modifier_slices()
            .pointer_inputs()
            .is_empty()
    );
    let order: Vec<NodeId> = collect_focus_order(&field.layout)
        .iter()
        .map(|entry| entry.node_id)
        .collect();
    assert_eq!(order, [field.decoration]);
}

#[test]
fn clearing_focus_releases_the_decoration_it_focused() {
    let _app_context = crate::render_state::app_context_test_scope();
    let field = decorated_field();
    field.press(1.0, 1.0);
    assert_eq!(focused_field_target(), Some(field.decoration));
    assert_eq!(focused_field_node(), Some(field.inner));
    assert_eq!(
        crate::focus_dispatch::active_focus_target(),
        Some(field.decoration)
    );

    clear_focus();
    assert!(!has_focused_field());
    assert_eq!(crate::focus_dispatch::active_focus_target(), None);
}

#[test]
fn the_decorated_field_and_its_box_are_keyed_by_the_refs_they_share() {
    let _composition = Composition::new(MemoryApplier::new());
    let state = TextFieldState::new("");
    let refs = TextFieldRefs::new();
    let decorator = TextFieldDecoratorElement::new(
        state,
        refs.clone(),
        TextStyle::default(),
        TextFieldLineLimits::default(),
        0,
    );
    let field = crate::text_field_modifier_node::TextFieldElement::new(state, TextStyle::default())
        .decorated_by(refs.clone());
    assert_eq!(decorator.key(), Some(refs.key()));
    assert_eq!(field.key(), Some(refs.key()));
    assert_ne!(TextFieldRefs::new().key(), refs.key());
    assert_eq!(
        crate::text_field_modifier_node::TextFieldElement::new(state, TextStyle::default()).key(),
        None
    );
}
