use std::{
    fmt,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{Applier, MemoryApplier, NodeId};
use cranpose_foundation::{
    DelegatableNode, ModifierChainNodeRef, ModifierNode, ModifierNodeElement, NodeCapabilities,
    NodeState,
};
use smallvec::SmallVec;

use super::Modifier;
use crate::{KeyEvent, active_focus_target, widgets::nodes::layout_node::LayoutNode};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum KeyPass {
    Preview,
    Bubble,
}

type KeyHandler = Rc<dyn Fn(&KeyEvent) -> bool>;

impl Modifier {
    /// Intercepts hardware keys before the focused component handles them.
    ///
    /// Ancestors run from the root toward the focused component, with modifiers
    /// on each component running in declaration order. Return `true` to consume
    /// the event and prevent editing, control activation and further handlers.
    /// Only the focused component and its ancestors receive the event.
    pub fn on_preview_key_event(self, handler: impl Fn(&KeyEvent) -> bool + 'static) -> Self {
        self.then(Self::with_element(KeyInputElement {
            pass: KeyPass::Preview,
            handler: Rc::new(handler),
        }))
    }

    /// Handles hardware keys the focused component did not consume.
    ///
    /// Events bubble from the focused component toward the root, with modifiers
    /// on each component running in reverse declaration order. Return `true` to
    /// consume the event and stop propagation to outer handlers.
    pub fn on_key_event(self, handler: impl Fn(&KeyEvent) -> bool + 'static) -> Self {
        self.then(Self::with_element(KeyInputElement {
            pass: KeyPass::Bubble,
            handler: Rc::new(handler),
        }))
    }
}

#[derive(Clone)]
struct KeyInputElement {
    pass: KeyPass,
    handler: KeyHandler,
}

impl fmt::Debug for KeyInputElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyInputElement")
            .field("pass", &self.pass)
            .finish_non_exhaustive()
    }
}

impl PartialEq for KeyInputElement {
    fn eq(&self, other: &Self) -> bool {
        self.pass == other.pass
    }
}

impl Hash for KeyInputElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pass.hash(state);
    }
}

impl ModifierNodeElement for KeyInputElement {
    type Node = KeyInputNode;

    fn create(&self) -> Self::Node {
        KeyInputNode {
            state: NodeState::new(),
            pass: self.pass,
            handler: self.handler.clone(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        node.pass = self.pass;
        node.handler = self.handler.clone();
    }

    fn always_update(&self) -> bool {
        true
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::KEY_INPUT
    }
}

struct KeyInputNode {
    state: NodeState,
    pass: KeyPass,
    handler: KeyHandler,
}

impl DelegatableNode for KeyInputNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for KeyInputNode {}

/// The focused component's keyboard route within a receiving surface or modal.
///
/// Platform shells create one route per hardware event, dispatch preview, run
/// their existing focused-control/editor handling, then dispatch unconsumed
/// events in the bubble phase. The route follows live parent links and stops at
/// `root`; a focused node belonging to another window is excluded.
pub struct KeyEventRoute {
    focused: Option<NodeId>,
    handlers: SmallVec<[NodeId; 8]>,
    in_scope: bool,
}

impl KeyEventRoute {
    /// Captures the ancestor route for the currently focused component.
    pub fn new(applier: &mut MemoryApplier, root: Option<NodeId>) -> Self {
        let mut route = Self {
            focused: active_focus_target(),
            handlers: SmallVec::new(),
            in_scope: false,
        };
        route.in_scope = visit_focus_ancestors(applier, route.focused, root, |id, node| {
            if node
                .modifier_chain()
                .capabilities()
                .contains(NodeCapabilities::KEY_INPUT)
            {
                route.handlers.push(id);
            }
        });
        if !route.in_scope {
            route.handlers.clear();
        }
        route
    }

    /// Whether the focused component belongs to the receiving surface or modal.
    ///
    /// Platform clipboard and IME entry points use this before accessing a
    /// focused editor. This check does not create a keyboard handler route.
    pub fn contains_focus(applier: &mut MemoryApplier, root: Option<NodeId>) -> bool {
        visit_focus_ancestors(applier, active_focus_target(), root, |_, _| {})
    }

    /// Whether focused-control dispatch still belongs to this event's target.
    ///
    /// A preview callback can move or clear focus. The event must not then edit
    /// the newly focused field or activate a control on another surface.
    pub fn focus_is_current(&self) -> bool {
        self.focused == active_focus_target() && (self.focused.is_none() || self.in_scope)
    }

    /// Offers the event to ancestors before the focused component.
    pub fn dispatch_preview(&self, applier: &mut MemoryApplier, event: &KeyEvent) -> bool {
        self.handlers
            .iter()
            .rev()
            .any(|&node| dispatch_node(applier, node, event, KeyPass::Preview))
    }

    /// Offers an unconsumed event from the focused component to its ancestors.
    pub fn dispatch_bubble(&self, applier: &mut MemoryApplier, event: &KeyEvent) -> bool {
        self.handlers
            .iter()
            .any(|&node| dispatch_node(applier, node, event, KeyPass::Bubble))
    }
}

fn visit_focus_ancestors(
    applier: &mut MemoryApplier,
    focused: Option<NodeId>,
    root: Option<NodeId>,
    mut visit: impl FnMut(NodeId, &LayoutNode),
) -> bool {
    let (Some(mut current), Some(root)) = (focused, root) else {
        return false;
    };
    for _ in 0..100_000 {
        let Ok(node) = applier.get_mut(current) else {
            return false;
        };
        let parent = node.parent();
        let window_root = if let Some(layout) = node.as_any_mut().downcast_mut::<LayoutNode>() {
            visit(current, layout);
            layout.is_window_root()
        } else {
            false
        };
        if current == root {
            return true;
        }
        if window_root {
            return false;
        }
        let Some(parent) = parent else {
            return false;
        };
        current = parent;
    }
    false
}

fn dispatch_node(
    applier: &mut MemoryApplier,
    node: NodeId,
    event: &KeyEvent,
    pass: KeyPass,
) -> bool {
    applier
        .with_node::<LayoutNode, _>(node, |node| {
            let chain = node.modifier_chain().chain();
            let visit = |entry: ModifierChainNodeRef<'_>| {
                entry.kind_set().contains(NodeCapabilities::KEY_INPUT)
                    && entry
                        .with_node(|node| {
                            let node: &dyn std::any::Any = node;
                            node.downcast_ref::<KeyInputNode>()
                                .is_some_and(|node| node.pass == pass && (node.handler)(event))
                        })
                        .unwrap_or(false)
            };
            match pass {
                KeyPass::Preview => chain.head_to_tail().any(visit),
                KeyPass::Bubble => chain.tail_to_head().any(visit),
            }
        })
        .unwrap_or(false)
}
