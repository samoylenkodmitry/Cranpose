//! Modifier slices that hold raw pointer handlers, for tests that dispatch
//! events to a hit target without composing a gesture.

use std::{
    fmt,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_foundation::{
    DelegatableNode, ModifierNode, ModifierNodeElement, NodeCapabilities, NodeState, PointerEvent,
    PointerInputNode, impl_pointer_input_node,
};
use cranpose_ui::{AppContext, Modifier, ModifierNodeSlices, collect_slices_from_modifier};

type PointerHandler = Rc<dyn Fn(PointerEvent)>;

struct RawPointerElement(PointerHandler);

impl fmt::Debug for RawPointerElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RawPointerElement")
    }
}

impl PartialEq for RawPointerElement {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Hash for RawPointerElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).cast::<()>().hash(state);
    }
}

impl ModifierNodeElement for RawPointerElement {
    type Node = RawPointerNode;

    fn create(&self) -> Self::Node {
        RawPointerNode {
            handler: Rc::clone(&self.0),
            state: NodeState::new(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        node.handler = Rc::clone(&self.0);
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::POINTER_INPUT
    }
}

struct RawPointerNode {
    handler: PointerHandler,
    state: NodeState,
}

impl DelegatableNode for RawPointerNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for RawPointerNode {
    impl_pointer_input_node!();
}

impl PointerInputNode for RawPointerNode {
    fn pointer_input_handler(&self) -> Option<PointerHandler> {
        Some(Rc::clone(&self.handler))
    }
}

/// Slices whose pointer inputs are `handlers`, in order.
pub(crate) fn pointer_slices(handlers: &[PointerHandler]) -> Rc<ModifierNodeSlices> {
    let modifier = handlers
        .iter()
        .fold(Modifier::empty(), |modifier, handler| {
            modifier.then(Modifier::with_element(RawPointerElement(Rc::clone(
                handler,
            ))))
        });
    let app_context = AppContext::new();
    Rc::new(app_context.enter(|| collect_slices_from_modifier(&modifier)))
}
