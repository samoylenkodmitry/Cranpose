use std::{
    cell::{Cell, RefCell},
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::NodeId;
use cranpose_foundation::{
    DelegatableNode, FocusNode, FocusState, ModifierNode, ModifierNodeContext, ModifierNodeElement,
    NodeCapabilities, NodeState, impl_focus_node,
};

use crate::{focus_dispatch, render_state::AppContextId};

/// Focus direction for navigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

type FocusChangedCallback = Rc<dyn Fn(FocusState)>;

struct FocusTargetShared {
    focus_state: Cell<FocusState>,
    on_focus_changed: RefCell<Option<FocusChangedCallback>>,
}

impl FocusTargetShared {
    fn new(on_focus_changed: Option<FocusChangedCallback>) -> Self {
        Self {
            focus_state: Cell::new(FocusState::Inactive),
            on_focus_changed: RefCell::new(on_focus_changed),
        }
    }

    fn set_focus_state(&self, state: FocusState) {
        let old_state = self.focus_state.get();
        if old_state != state {
            self.focus_state.set(state);
            let callback = self.on_focus_changed.borrow().clone();
            if let Some(callback) = callback {
                callback(state);
            }
        }
    }
}

impl focus_dispatch::FocusTargetHandle for FocusTargetShared {
    fn set_focus_state(&self, state: FocusState) {
        FocusTargetShared::set_focus_state(self, state);
    }
}

pub struct FocusTargetNode {
    state: NodeState,
    shared: Rc<FocusTargetShared>,
    handle: Rc<dyn focus_dispatch::FocusTargetHandle>,
    registered_node_id: Cell<Option<NodeId>>,
}

impl FocusTargetNode {
    pub fn new() -> Self {
        Self::from_callback(None)
    }

    pub fn with_callback<F>(callback: F) -> Self
    where
        F: Fn(FocusState) + 'static,
    {
        Self::from_callback(Some(Rc::new(callback) as FocusChangedCallback))
    }

    fn from_callback(on_focus_changed: Option<FocusChangedCallback>) -> Self {
        let shared = Rc::new(FocusTargetShared::new(on_focus_changed));
        let handle: Rc<dyn focus_dispatch::FocusTargetHandle> = shared.clone();
        Self {
            state: NodeState::new(),
            shared,
            handle,
            registered_node_id: Cell::new(None),
        }
    }

    pub fn set_focus_state(&self, state: FocusState) {
        self.shared.set_focus_state(state);
    }

    pub fn clear_focus(&self) {
        self.set_focus_state(FocusState::Inactive);
    }

    fn set_callback(&self, callback: Option<FocusChangedCallback>) {
        *self.shared.on_focus_changed.borrow_mut() = callback;
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
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        self.state.set_attached(true);
        if let Some(node_id) = context.node_id() {
            self.registered_node_id.set(Some(node_id));
            focus_dispatch::register_focus_target(node_id, Rc::clone(&self.handle));
        }
    }

    fn on_detach(&mut self) {
        self.state.set_attached(false);
        if let Some(node_id) = self.registered_node_id.take() {
            focus_dispatch::unregister_focus_target(node_id, &self.handle);
        }
        self.clear_focus();
    }

    impl_focus_node!();
}

impl FocusNode for FocusTargetNode {
    fn focus_state(&self) -> FocusState {
        self.shared.focus_state.get()
    }

    fn on_focus_changed(&mut self, _context: &mut dyn ModifierNodeContext, state: FocusState) {
        self.set_focus_state(state);
    }
}

#[derive(Clone)]
pub struct FocusTargetElement {
    on_focus_changed: Option<FocusChangedCallback>,
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
        self.on_focus_changed.is_some() == other.on_focus_changed.is_some()
    }
}

impl Hash for FocusTargetElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "focus_target".hash(state);
        self.on_focus_changed.is_some().hash(state);
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
        node.set_callback(self.on_focus_changed.clone());
    }

    fn inspector_name(&self) -> &'static str {
        "focusTarget"
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::FOCUS
    }

    fn always_update(&self) -> bool {
        true
    }
}

/// Why [`FocusRequester::request_focus`] could not move focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusRequestError {
    /// The requester has never been attached to a node via
    /// [`Modifier::focus_requester`](super::Modifier::focus_requester), or the
    /// node it was attached to has since left composition.
    NotAttached,
    /// The requester's node is attached, but nothing on it — no
    /// [`Modifier::focus_target`](super::Modifier::focus_target), no
    /// [`Modifier::on_focus_changed`](super::Modifier::on_focus_changed), no
    /// text field — is registered to receive focus.
    NoFocusTarget,
}

impl std::fmt::Display for FocusRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FocusRequestError::NotAttached => {
                write!(f, "FocusRequester is not attached to a node in composition")
            }
            FocusRequestError::NoFocusTarget => write!(
                f,
                "FocusRequester's node has no focus target to move focus onto"
            ),
        }
    }
}

impl std::error::Error for FocusRequestError {}

#[derive(Clone, Copy)]
struct FocusRequesterBinding {
    app_context: AppContextId,
    node_id: NodeId,
}

/// A handle an app holds to move focus onto a node imperatively.
///
/// `remember` one, hang it on a node with
/// [`Modifier::focus_requester`](super::Modifier::focus_requester) next to a
/// [`Modifier::focus_target`](super::Modifier::focus_target) (or
/// [`Modifier::on_focus_changed`](super::Modifier::on_focus_changed)), and
/// call [`request_focus`](Self::request_focus) — from a click handler, an
/// effect that runs once a screen appears, wherever the app decides focus
/// should move.
///
/// ```ignore
/// let requester = remember(FocusRequester::new).with(Clone::clone);
/// // ... in the composition:
/// Modifier::empty().focus_requester(&requester).focus_target()
/// // ... later, imperatively:
/// requester.request_focus().ok();
/// ```
#[derive(Clone, Default)]
pub struct FocusRequester {
    binding: Rc<Cell<Option<FocusRequesterBinding>>>,
}

impl FocusRequester {
    pub fn new() -> Self {
        Self::default()
    }

    /// Moves focus onto the node this requester is attached to.
    ///
    /// See [`FocusRequestError`] for the two predictable ways this can fail
    /// instead of moving focus.
    pub fn request_focus(&self) -> Result<(), FocusRequestError> {
        let Some(binding) = self.binding.get() else {
            return Err(FocusRequestError::NotAttached);
        };
        match focus_dispatch::request_focus_for(binding.app_context, binding.node_id) {
            Some(true) => Ok(()),
            Some(false) => Err(FocusRequestError::NoFocusTarget),
            None => Err(FocusRequestError::NotAttached),
        }
    }

    /// The node this requester is attached to, if attached.
    pub fn node_id(&self) -> Option<NodeId> {
        self.binding.get().map(|binding| binding.node_id)
    }

    fn bind(&self, binding: Option<FocusRequesterBinding>) {
        self.binding.set(binding);
    }

    fn binding_here(node_id: Option<NodeId>) -> Option<FocusRequesterBinding> {
        Some(FocusRequesterBinding {
            app_context: crate::render_state::current_app_context_id_opt()?,
            node_id: node_id?,
        })
    }
}

impl std::fmt::Debug for FocusRequester {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusRequester")
            .field("node_id", &self.node_id())
            .finish()
    }
}

pub struct FocusRequesterNode {
    state: NodeState,
    requester: FocusRequester,
}

impl FocusRequesterNode {
    pub(crate) fn new(requester: FocusRequester) -> Self {
        Self {
            state: NodeState::new(),
            requester,
        }
    }
}

impl DelegatableNode for FocusRequesterNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for FocusRequesterNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        self.state.set_attached(true);
        self.requester
            .bind(FocusRequester::binding_here(context.node_id()));
    }

    fn on_detach(&mut self) {
        self.state.set_attached(false);
        self.requester.bind(None);
    }
}

/// Modifier element for [`FocusRequester`].
#[derive(Clone)]
pub struct FocusRequesterElement {
    requester: FocusRequester,
}

impl FocusRequesterElement {
    pub(crate) fn new(requester: FocusRequester) -> Self {
        Self { requester }
    }
}

impl std::fmt::Debug for FocusRequesterElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FocusRequesterElement")
    }
}

impl PartialEq for FocusRequesterElement {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.requester.binding, &other.requester.binding)
    }
}

impl Eq for FocusRequesterElement {}

impl Hash for FocusRequesterElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.requester.binding).hash(state);
    }
}

impl ModifierNodeElement for FocusRequesterElement {
    type Node = FocusRequesterNode;

    fn create(&self) -> Self::Node {
        FocusRequesterNode::new(self.requester.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        if !Rc::ptr_eq(&node.requester.binding, &self.requester.binding) {
            let bound = node.requester.binding.get();
            node.requester.bind(None);
            node.requester = self.requester.clone();
            node.requester.bind(bound);
        }
    }

    fn inspector_name(&self) -> &'static str {
        "focusRequester"
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::NONE
    }
}

#[cfg(test)]
#[path = "tests/focus_tests.rs"]
mod tests;
