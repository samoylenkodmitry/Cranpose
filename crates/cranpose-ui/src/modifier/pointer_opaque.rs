use std::rc::Rc;

use cranpose_foundation::{
    DelegatableNode, ModifierNode, ModifierNodeElement, NodeCapabilities, NodeState, PointerEvent,
    PointerEventKind, PointerInputNode, impl_pointer_input_node,
};

use super::{Modifier, inspector_metadata};

impl Modifier {
    /// Keeps the presses that land on this node from reaching the nodes
    /// drawn below it, as a Compose `Surface` does for a dialog: a tap on the
    /// dialog's text does not reach the scrim, which would close it.
    ///
    /// A press goes to every pointer target under it, topmost first, until
    /// one consumes it; this node consumes the press after the controls
    /// inside it saw it. The press does not reach this node's ancestors
    /// either, so it belongs on the surface of an overlay, not inside a list
    /// that scrolls under a finger.
    pub(crate) fn pointer_opaque(self) -> Self {
        self.then(
            Self::with_element(PointerOpaqueElement)
                .with_inspector_metadata(inspector_metadata("pointerOpaque", |_| {})),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct PointerOpaqueElement;

impl ModifierNodeElement for PointerOpaqueElement {
    type Node = PointerOpaqueNode;

    fn create(&self) -> Self::Node {
        PointerOpaqueNode {
            dispatch: Rc::new(|event: PointerEvent| {
                if event.kind == PointerEventKind::Down {
                    event.consume();
                }
            }),
            state: NodeState::new(),
        }
    }

    fn update(&self, _node: &mut Self::Node) {}

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::POINTER_INPUT
    }
}

struct PointerOpaqueNode {
    dispatch: Rc<dyn Fn(PointerEvent)>,
    state: NodeState,
}

impl ModifierNode for PointerOpaqueNode {
    impl_pointer_input_node!();
}

impl DelegatableNode for PointerOpaqueNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl PointerInputNode for PointerOpaqueNode {
    fn pointer_input_handler(&self) -> Option<Rc<dyn Fn(PointerEvent)>> {
        Some(Rc::clone(&self.dispatch))
    }
}
