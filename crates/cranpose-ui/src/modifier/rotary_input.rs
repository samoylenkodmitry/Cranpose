use std::{
    cell::Cell,
    fmt,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_foundation::{
    DelegatableNode, ModifierNode, ModifierNodeElement, NodeCapabilities, NodeState, PointerEvent,
    PointerEventKind, PointerInputNode, RotaryScrollEvent, impl_pointer_input_node,
};

use super::{Modifier, inspector_metadata};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum RotaryPass {
    Pre,
    Bubble,
}

impl RotaryPass {
    fn matches(self, kind: PointerEventKind) -> bool {
        matches!(
            (self, kind),
            (RotaryPass::Pre, PointerEventKind::RotaryScrollPre)
                | (RotaryPass::Bubble, PointerEventKind::RotaryScroll)
        )
    }
}

type RotaryHandler = Rc<dyn Fn(RotaryScrollEvent) -> bool>;

impl Modifier {
    /// Handles rotary scroll events (Pixel Watch crown, Galaxy Watch rotating
    /// bezel) during the **bubble** pass.
    ///
    /// The handler receives a [`RotaryScrollEvent`] whose scroll amounts are
    /// already in pixels. Return `true` to consume the event and stop it
    /// propagating to ancestors; return `false` to let it keep bubbling.
    ///
    /// Equivalent to Compose's `Modifier.onRotaryScrollEvent`.
    ///
    /// ```ignore
    /// Modifier::new().on_rotary_scroll_event(move |event| {
    ///     offset.set(offset.get() + event.vertical_scroll_pixels);
    ///     true
    /// })
    /// ```
    pub fn on_rotary_scroll_event<F>(self, handler: F) -> Self
    where
        F: Fn(RotaryScrollEvent) -> bool + 'static,
    {
        self.rotary_element(RotaryPass::Bubble, Rc::new(handler), "onRotaryScrollEvent")
    }

    /// Handles rotary scroll events during the **capture** pass, before the
    /// focused node sees them.
    ///
    /// Return `true` to consume the event and stop it reaching descendants.
    ///
    /// Equivalent to Compose's `Modifier.onPreRotaryScrollEvent`.
    pub fn on_pre_rotary_scroll_event<F>(self, handler: F) -> Self
    where
        F: Fn(RotaryScrollEvent) -> bool + 'static,
    {
        self.rotary_element(RotaryPass::Pre, Rc::new(handler), "onPreRotaryScrollEvent")
    }

    fn rotary_element(
        self,
        pass: RotaryPass,
        handler: RotaryHandler,
        inspector_name: &'static str,
    ) -> Self {
        let element = RotaryInputElement::new(pass, handler);
        let handler_id = element.handler_id;
        self.then(
            Self::with_element(element).with_inspector_metadata(inspector_metadata(
                inspector_name,
                move |info| {
                    info.add_property("handlerId", handler_id.to_string());
                },
            )),
        )
    }
}

#[derive(Clone)]
struct RotaryInputElement {
    pass: RotaryPass,
    handler: RotaryHandler,
    handler_id: u64,
}

impl RotaryInputElement {
    fn new(pass: RotaryPass, handler: RotaryHandler) -> Self {
        let handler_id = Rc::as_ptr(&handler) as *const () as usize as u64;
        Self {
            pass,
            handler,
            handler_id,
        }
    }
}

impl fmt::Debug for RotaryInputElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RotaryInputElement")
            .field("pass", &self.pass)
            .field("handler_id", &self.handler_id)
            .finish()
    }
}

impl PartialEq for RotaryInputElement {
    fn eq(&self, other: &Self) -> bool {
        self.pass == other.pass
    }
}

impl Eq for RotaryInputElement {}

impl Hash for RotaryInputElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pass.hash(state);
    }
}

impl ModifierNodeElement for RotaryInputElement {
    type Node = RotaryInputModifierNode;

    fn create(&self) -> Self::Node {
        RotaryInputModifierNode::new(self.pass, self.handler.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        node.handler.set(Some(self.handler.clone()));
    }

    fn always_update(&self) -> bool {
        true
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::POINTER_INPUT
    }
}

/// Modifier node that forwards rotary scroll events to an app handler.
///
/// Named after Compose's `RotaryInputModifierNode`. It rides the existing
/// pointer dispatch path: the shell sends rotary passes as
/// [`PointerEventKind::RotaryScrollPre`]/[`PointerEventKind::RotaryScroll`]
/// pointer events, and this node filters for its own pass.
pub struct RotaryInputModifierNode {
    handler: Rc<Cell<Option<RotaryHandler>>>,
    dispatch: Rc<dyn Fn(PointerEvent)>,
    state: NodeState,
}

impl RotaryInputModifierNode {
    fn new(pass: RotaryPass, handler: RotaryHandler) -> Self {
        let handler_cell: Rc<Cell<Option<RotaryHandler>>> = Rc::new(Cell::new(Some(handler)));
        let handler_for_dispatch = Rc::clone(&handler_cell);
        let dispatch = Rc::new(move |event: PointerEvent| {
            if !pass.matches(event.kind) || event.is_consumed() {
                return;
            }
            let Some(rotary) = event.rotary_scroll_event() else {
                return;
            };
            let Some(handler) = handler_for_dispatch.take() else {
                return;
            };
            let consumed = handler(rotary);
            if handler_for_dispatch.take().is_none() {
                handler_for_dispatch.set(Some(handler));
            }
            if consumed {
                event.consume();
            }
        });

        Self {
            handler: handler_cell,
            dispatch,
            state: NodeState::new(),
        }
    }
}

impl ModifierNode for RotaryInputModifierNode {
    impl_pointer_input_node!();
}

impl DelegatableNode for RotaryInputModifierNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl PointerInputNode for RotaryInputModifierNode {
    fn pointer_input_handler(&self) -> Option<Rc<dyn Fn(PointerEvent)>> {
        Some(Rc::clone(&self.dispatch))
    }
}

#[cfg(test)]
#[path = "tests/rotary_input_tests.rs"]
mod tests;
