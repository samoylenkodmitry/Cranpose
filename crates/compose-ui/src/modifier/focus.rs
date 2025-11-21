//! Focus modifier nodes for Compose-RS.
//!
//! This module implements focus management that mirrors Jetpack Compose's
//! focus system. Focus nodes participate in focus traversal, track focus state,
//! and integrate with the modifier chain lifecycle.

use std::cell::Cell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use compose_foundation::{
    impl_focus_node, DelegatableNode, FocusNode, FocusState, ModifierNode, ModifierNodeContext,
    ModifierNodeElement, NodeCapabilities, NodeState,
};

/// Focus direction for navigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)] // TODO: used in future focus manager integration
pub enum FocusDirection {
    /// Enter focus from outside.
    Enter,
    /// Exit focus to outside.
    Exit,
    /// Move to next focusable.
    Next,
    /// Move to previous focusable.
    Previous,
    /// Move up (2D navigation).
    Up,
    /// Move down (2D navigation).
    Down,
    /// Move left (2D navigation).
    Left,
    /// Move right (2D navigation).
    Right,
}

/// A focus target node that can receive focus.
///
/// This is the core building block for focusable components. Each focus target
/// tracks its own focus state and participates in the focus traversal system.
pub struct FocusTargetNode {
    state: NodeState,
    focus_state: Cell<FocusState>,
    on_focus_changed: Option<Rc<dyn Fn(FocusState)>>,
}

impl FocusTargetNode {
    pub fn new() -> Self {
        Self {
            state: NodeState::new(),
            focus_state: Cell::new(FocusState::Inactive),
            on_focus_changed: None,
        }
    }

    pub fn with_callback<F>(callback: F) -> Self
    where
        F: Fn(FocusState) + 'static,
    {
        Self {
            state: NodeState::new(),
            focus_state: Cell::new(FocusState::Inactive),
            on_focus_changed: Some(Rc::new(callback)),
        }
    }

    /// Sets the focus state for this node.
    pub fn set_focus_state(&self, state: FocusState) {
        let old_state = self.focus_state.get();
        if old_state != state {
            self.focus_state.set(state);
            if let Some(callback) = &self.on_focus_changed {
                callback(state);
            }
        }
    }

    /// Requests focus for this node.
    #[allow(dead_code)] // TODO: used in future focus manager integration
    pub fn request_focus(&self) -> bool {
        // This will be wired up to the focus manager in the next phase
        true
    }

    /// Clears focus from this node.
    pub fn clear_focus(&self) {
        self.set_focus_state(FocusState::Inactive);
    }
}

impl Default for FocusTargetNode {
    fn default() -> Self {
        Self::new()
    }
}

impl DelegatableNode for FocusTargetNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for FocusTargetNode {
    fn on_attach(&mut self, _context: &mut dyn ModifierNodeContext) {
        self.state.set_attached(true);
    }

    fn on_detach(&mut self) {
        self.state.set_attached(false);
        self.clear_focus();
    }

    // Capability-driven implementation using helper macro
    impl_focus_node!();
}

impl FocusNode for FocusTargetNode {
    fn focus_state(&self) -> FocusState {
        self.focus_state.get()
    }

    fn on_focus_changed(&mut self, _context: &mut dyn ModifierNodeContext, state: FocusState) {
        self.set_focus_state(state);
    }
}

/// Modifier element for focus targets.
///
/// Creates a focusable modifier that can receive and track focus.
#[derive(Clone)]
pub struct FocusTargetElement {
    on_focus_changed: Option<Rc<dyn Fn(FocusState)>>,
}

impl FocusTargetElement {
    pub fn new() -> Self {
        Self {
            on_focus_changed: None,
        }
    }

    pub fn with_callback<F>(callback: F) -> Self
    where
        F: Fn(FocusState) + 'static,
    {
        Self {
            on_focus_changed: Some(Rc::new(callback)),
        }
    }
}

impl Default for FocusTargetElement {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FocusTargetElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusTargetElement")
            .field("has_callback", &self.on_focus_changed.is_some())
            .finish()
    }
}

impl PartialEq for FocusTargetElement {
    fn eq(&self, other: &Self) -> bool {
        // Compare callback pointers if both present
        match (&self.on_focus_changed, &other.on_focus_changed) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}

impl Hash for FocusTargetElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash the address of the callback Rc if present
        match &self.on_focus_changed {
            Some(rc) => {
                let ptr = Rc::as_ptr(rc);
                (ptr as *const ()).hash(state);
            }
            None => {
                0usize.hash(state);
            }
        }
    }
}

impl ModifierNodeElement for FocusTargetElement {
    type Node = FocusTargetNode;

    fn create(&self) -> Self::Node {
        if let Some(callback) = &self.on_focus_changed {
            FocusTargetNode::with_callback({
                let callback = callback.clone();
                move |state| callback(state)
            })
        } else {
            FocusTargetNode::new()
        }
    }

    fn update(&self, node: &mut Self::Node) {
        node.on_focus_changed = self.on_focus_changed.clone();
    }

    fn inspector_name(&self) -> &'static str {
        "focusTarget"
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::FOCUS
    }
}

/// A focus requester node that can request focus for associated targets.
///
/// This node allows programmatic focus requests and is typically used to
/// trigger focus on specific components in response to user actions or
/// app logic.
pub struct FocusRequesterNode {
    state: NodeState,
    requester_id: usize,
}

impl FocusRequesterNode {
    pub fn new(requester_id: usize) -> Self {
        Self {
            state: NodeState::new(),
            requester_id,
        }
    }

    /// Requests focus for the associated target.
    #[allow(dead_code)] // TODO: used in future focus manager integration
    pub fn request_focus(&self) -> bool {
        // This will be wired up to the focus manager in the next phase
        true
    }
}

impl DelegatableNode for FocusRequesterNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for FocusRequesterNode {
    fn on_attach(&mut self, _context: &mut dyn ModifierNodeContext) {
        self.state.set_attached(true);
    }

    fn on_detach(&mut self) {
        self.state.set_attached(false);
    }
}

/// Modifier element for focus requesters.
///
/// Creates a modifier that can be used to programmatically request focus.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FocusRequesterElement {
    requester_id: usize,
}

impl FocusRequesterElement {
    pub fn new(requester_id: usize) -> Self {
        Self { requester_id }
    }
}

impl ModifierNodeElement for FocusRequesterElement {
    type Node = FocusRequesterNode;

    fn create(&self) -> Self::Node {
        FocusRequesterNode::new(self.requester_id)
    }

    fn update(&self, node: &mut Self::Node) {
        node.requester_id = self.requester_id;
    }

    fn inspector_name(&self) -> &'static str {
        "focusRequester"
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::FOCUS
    }
}

/// A handle for requesting focus programmatically.
///
/// This mirrors Jetpack Compose's FocusRequester class and provides
/// an API for triggering focus changes from application code.
#[derive(Clone, Debug, Default)]
pub struct FocusRequester {
    id: usize,
}

impl FocusRequester {
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    pub fn id(&self) -> usize {
        self.id
    }

    /// Requests focus for components associated with this requester.
    pub fn request_focus(&self) -> bool {
        // This will be wired up to the focus manager in the next phase
        true
    }

    /// Captures focus, preventing other components from taking focus.
    pub fn capture_focus(&self) -> bool {
        // This will be wired up to the focus manager in the next phase
        true
    }

    /// Releases captured focus.
    pub fn free_focus(&self) -> bool {
        // This will be wired up to the focus manager in the next phase
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compose_foundation::{BasicModifierNodeContext, ModifierNodeChain};

    #[test]
    fn focus_target_node_lifecycle() {
        let mut node = FocusTargetNode::new();
        let mut context = BasicModifierNodeContext::new();

        assert_eq!(node.focus_state(), FocusState::Inactive);
        assert!(!node.node_state().is_attached());

        node.on_attach(&mut context);
        assert!(node.node_state().is_attached());

        node.set_focus_state(FocusState::Active);
        assert_eq!(node.focus_state(), FocusState::Active);
        assert!(node.focus_state().is_focused());

        node.on_detach();
        assert!(!node.node_state().is_attached());
        assert_eq!(node.focus_state(), FocusState::Inactive);
    }

    #[test]
    fn focus_target_callback_invoked() {
        use std::cell::RefCell;
        let states = Rc::new(RefCell::new(Vec::new()));
        let states_clone = states.clone();

        let node = FocusTargetNode::with_callback(move |state| {
            states_clone.borrow_mut().push(state);
        });

        node.set_focus_state(FocusState::Active);
        node.set_focus_state(FocusState::ActiveParent);
        node.set_focus_state(FocusState::Inactive);

        let recorded = states.borrow();
        assert_eq!(recorded.len(), 3);
        assert_eq!(recorded[0], FocusState::Active);
        assert_eq!(recorded[1], FocusState::ActiveParent);
        assert_eq!(recorded[2], FocusState::Inactive);
    }

    #[test]
    fn focus_element_creates_node() {
        let element = FocusTargetElement::new();
        let node = element.create();
        assert_eq!(node.focus_state(), FocusState::Inactive);
    }

    #[test]
    fn focus_chain_integration() {
        let element = FocusTargetElement::new();
        let dyn_element = compose_foundation::modifier_element(element);

        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();

        chain.update(vec![dyn_element], &mut context);

        assert_eq!(chain.len(), 1);
        assert!(chain.has_capability(NodeCapabilities::FOCUS));
    }

    #[test]
    fn focus_requester_unique_ids() {
        let req1 = FocusRequester::new();
        let req2 = FocusRequester::new();
        assert_ne!(req1.id(), req2.id());
    }

    #[test]
    fn focus_state_predicates() {
        assert!(FocusState::Active.is_focused());
        assert!(FocusState::Captured.is_focused());
        assert!(!FocusState::Inactive.is_focused());
        assert!(!FocusState::ActiveParent.is_focused());

        assert!(FocusState::Active.has_focus());
        assert!(FocusState::ActiveParent.has_focus());
        assert!(FocusState::Captured.has_focus());
        assert!(!FocusState::Inactive.has_focus());

        assert!(FocusState::Captured.is_captured());
        assert!(!FocusState::Active.is_captured());
    }
}
