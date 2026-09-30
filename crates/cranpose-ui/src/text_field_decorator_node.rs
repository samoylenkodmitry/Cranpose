//! The decoration box of a decorated text field. As in Compose, where
//! `TextFieldDecoratorModifier` sits on the box around the inner field, the
//! box takes the field's pointer input, focus and semantics, so a press
//! anywhere in it edits the field; the inner field only lays out and draws.

use std::{
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::NodeId;
use cranpose_foundation::{
    DelegatableNode, InvalidationKind, ModifierNode, ModifierNodeContext, ModifierNodeElement,
    NodeCapabilities, NodeState, PointerEvent, PointerInputNode, SemanticsConfiguration,
    SemanticsNode,
    text::{TextFieldLineLimits, TextFieldState},
};

use crate::{
    focus_dispatch::FocusTargetHandle,
    text::TextStyle,
    text_field_modifier_node::{
        TextFieldFocusBridge, TextFieldModifierNode, TextFieldRefs, merge_text_field_semantics,
    },
};

/// Element of a decoration box whose inner field shares `refs`.
#[derive(Clone)]
pub(crate) struct TextFieldDecoratorElement {
    state: TextFieldState,
    refs: TextFieldRefs,
    style: TextStyle,
    line_limits: TextFieldLineLimits,
    modal_depth: usize,
}

impl TextFieldDecoratorElement {
    pub(crate) fn new(
        state: TextFieldState,
        refs: TextFieldRefs,
        style: TextStyle,
        line_limits: TextFieldLineLimits,
        modal_depth: usize,
    ) -> Self {
        Self {
            state,
            refs,
            style,
            line_limits,
            modal_depth,
        }
    }

    /// The field's own pointer handler, fed each event at the position it
    /// has in the inner field, whose window transform layout publishes.
    fn pointer_handler(&self) -> Rc<dyn Fn(PointerEvent)> {
        let field = TextFieldModifierNode::create_handler(
            self.state,
            self.refs.clone(),
            self.line_limits,
            self.style.clone(),
            self.modal_depth,
        );
        let field_transform = Rc::clone(&self.refs.local_to_window);
        Rc::new(move |event: PointerEvent| {
            if let Some(inverse) = field_transform.get().inverse() {
                field(event.copy_with_local_position(inverse.map_point(event.global_position)));
            }
        })
    }
}

impl std::fmt::Debug for TextFieldDecoratorElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextFieldDecoratorElement")
            .field("text", &self.state.text())
            .field("line_limits", &self.line_limits)
            .finish()
    }
}

impl Hash for TextFieldDecoratorElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.state.id().hash(state);
        self.refs.key().hash(state);
        self.style.render_hash().hash(state);
        self.line_limits.hash(state);
        self.modal_depth.hash(state);
    }
}

impl PartialEq for TextFieldDecoratorElement {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
            && self.refs == other.refs
            && self.style == other.style
            && self.line_limits == other.line_limits
            && self.modal_depth == other.modal_depth
    }
}

impl Eq for TextFieldDecoratorElement {}

impl ModifierNodeElement for TextFieldDecoratorElement {
    type Node = TextFieldDecoratorNode;

    fn create(&self) -> Self::Node {
        TextFieldDecoratorNode {
            handler: self.pointer_handler(),
            field: self.clone(),
            focus_target: None,
            node_state: NodeState::new(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        if node.field != *self {
            node.handler = self.pointer_handler();
            node.field = self.clone();
        }
    }

    /// The node shares its inner field's refs, so it is never handed to the
    /// box of another field.
    fn key(&self) -> Option<u64> {
        Some(self.refs.key())
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::SEMANTICS | NodeCapabilities::POINTER_INPUT
    }
}

pub(crate) struct TextFieldDecoratorNode {
    field: TextFieldDecoratorElement,
    handler: Rc<dyn Fn(PointerEvent)>,
    focus_target: Option<(NodeId, Rc<dyn FocusTargetHandle>)>,
    node_state: NodeState,
}

impl DelegatableNode for TextFieldDecoratorNode {
    fn node_state(&self) -> &NodeState {
        &self.node_state
    }
}

impl ModifierNode for TextFieldDecoratorNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(InvalidationKind::Semantics);
        let Some(node_id) = context.node_id() else {
            return;
        };
        let field = &self.field;
        field.refs.focus_node.set(Some(node_id));
        let bridge = TextFieldFocusBridge::handle(
            field.state,
            field.refs.clone(),
            field.style.clone(),
            field.line_limits,
        );
        crate::focus_dispatch::register_focus_target(node_id, Rc::clone(&bridge));
        self.focus_target = Some((node_id, bridge));
    }

    fn on_detach(&mut self) {
        if let Some((node_id, bridge)) = self.focus_target.take() {
            crate::focus_dispatch::unregister_focus_target(node_id, &bridge);
        }
    }

    fn as_semantics_node(&self) -> Option<&dyn SemanticsNode> {
        Some(self)
    }

    fn as_semantics_node_mut(&mut self) -> Option<&mut dyn SemanticsNode> {
        Some(self)
    }

    fn as_pointer_input_node(&self) -> Option<&dyn PointerInputNode> {
        Some(self)
    }

    fn as_pointer_input_node_mut(&mut self) -> Option<&mut dyn PointerInputNode> {
        Some(self)
    }
}

impl SemanticsNode for TextFieldDecoratorNode {
    fn merge_semantics(&self, config: &mut SemanticsConfiguration) {
        merge_text_field_semantics(self.field.state, self.field.line_limits, config);
    }
}

impl PointerInputNode for TextFieldDecoratorNode {
    fn on_pointer_event(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        _event: &PointerEvent,
    ) -> bool {
        false
    }

    fn pointer_input_handler(&self) -> Option<Rc<dyn Fn(PointerEvent)>> {
        Some(Rc::clone(&self.handler))
    }
}

#[cfg(test)]
#[path = "tests/text_field_decorator_node_tests.rs"]
mod tests;
