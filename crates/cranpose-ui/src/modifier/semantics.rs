use std::{
    cell::Cell,
    fmt,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::NodeId;
use cranpose_foundation::{
    DelegatableNode, ModifierNode, ModifierNodeChain, ModifierNodeContext, ModifierNodeElement,
    NodeCapabilities, NodeState, SemanticsConfiguration, SemanticsNode as SemanticsNodeTrait,
};

use super::{Modifier, ModifierChainHandle};
use crate::render_state::AppContextId;

/// A handle an app holds to say that what its semantics recorder would report
/// has changed.
///
/// A `.semantics()` recorder is re-run on every collection, so it always reports
/// the app's current answer. It cannot say *when* that answer changed, and a
/// tree is only re-collected when some node marks its semantics dirty — which
/// until now meant attaching a node, rebuilding its modifier chain, or a layout
/// pass. A screen that is one `Canvas` does none of those: its layout never
/// changes and its frame loop draws rather than recomposes, so the tree it
/// published at boot was the tree a screen reader kept reading.
///
/// This is the way out, and it is Jetpack Compose's:
/// `SemanticsModifierNode.invalidateSemantics()`. Remember one requester, hang
/// it on the same node as the recorder, and call [`invalidate`] when the content
/// changes — no recomposition, no layout pass.
///
/// ```ignore
/// let semantics = remember(SemanticsRequester::new).with(Clone::clone);
/// // ... in the composition:
/// Modifier::empty()
///     .semantics_requester(&semantics)
///     .semantics(move |config| { /* reads live app state */ })
/// // ... and in the frame loop, guarded by a revision so an unchanged tree is
/// // not republished every frame:
/// if revision != last_revision {
///     last_revision = revision;
///     semantics.invalidate();
/// }
/// ```
///
/// [`invalidate`]: SemanticsRequester::invalidate
#[derive(Clone, Default)]
pub struct SemanticsRequester {
    binding: Rc<Cell<Option<SemanticsBinding>>>,
}

#[derive(Clone, Copy)]
struct SemanticsBinding {
    app_context: AppContextId,
    node_id: NodeId,
}

impl SemanticsRequester {
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks the attached node's semantics for re-collection on the next frame.
    ///
    /// Idempotent within a frame and cheap: the node joins a set the shell
    /// drains once per frame. Before the node attaches, and after it detaches,
    /// this does nothing — there is no tree to mark.
    pub fn invalidate(&self) {
        if let Some(binding) = self.binding.get() {
            crate::semantics_dispatch::schedule_semantics_invalidation_in(
                binding.app_context,
                binding.node_id,
            );
        }
    }

    /// The layout node this requester is bound to, if it is attached.
    pub fn node_id(&self) -> Option<NodeId> {
        self.binding.get().map(|binding| binding.node_id)
    }

    fn bind(&self, binding: Option<SemanticsBinding>) {
        self.binding.set(binding);
    }

    fn binding_here(node_id: Option<NodeId>) -> Option<SemanticsBinding> {
        Some(SemanticsBinding {
            app_context: crate::render_state::current_app_context_id_opt()?,
            node_id: node_id?,
        })
    }
}

impl fmt::Debug for SemanticsRequester {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SemanticsRequester")
            .field("node_id", &self.node_id())
            .finish()
    }
}

pub struct SemanticsModifierNode {
    recorder: Rc<dyn Fn(&mut SemanticsConfiguration)>,
    state: NodeState,
}

impl SemanticsModifierNode {
    pub fn new(recorder: Rc<dyn Fn(&mut SemanticsConfiguration)>) -> Self {
        Self {
            recorder,
            state: NodeState::new(),
        }
    }
}

impl DelegatableNode for SemanticsModifierNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for SemanticsModifierNode {
    fn as_semantics_node(&self) -> Option<&dyn SemanticsNodeTrait> {
        Some(self)
    }

    fn as_semantics_node_mut(&mut self) -> Option<&mut dyn SemanticsNodeTrait> {
        Some(self)
    }
}

pub struct SemanticsRequesterNode {
    state: NodeState,
    requester: SemanticsRequester,
}

impl SemanticsRequesterNode {
    pub(crate) fn new(requester: SemanticsRequester) -> Self {
        Self {
            state: NodeState::new(),
            requester,
        }
    }
}

impl DelegatableNode for SemanticsRequesterNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for SemanticsRequesterNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        self.state.set_attached(true);
        self.requester
            .bind(SemanticsRequester::binding_here(context.node_id()));
    }

    fn on_detach(&mut self) {
        self.state.set_attached(false);
        self.requester.bind(None);
    }
}

/// Modifier element for [`SemanticsRequester`].
#[derive(Clone)]
pub struct SemanticsRequesterElement {
    requester: SemanticsRequester,
}

impl SemanticsRequesterElement {
    pub(crate) fn new(requester: SemanticsRequester) -> Self {
        Self { requester }
    }
}

impl fmt::Debug for SemanticsRequesterElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SemanticsRequesterElement")
    }
}

impl PartialEq for SemanticsRequesterElement {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.requester.binding, &other.requester.binding)
    }
}

impl Eq for SemanticsRequesterElement {}

impl Hash for SemanticsRequesterElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.requester.binding).hash(state);
    }
}

impl ModifierNodeElement for SemanticsRequesterElement {
    type Node = SemanticsRequesterNode;

    fn create(&self) -> Self::Node {
        SemanticsRequesterNode::new(self.requester.clone())
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
        "semanticsRequester"
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::NONE
    }
}

impl SemanticsNodeTrait for SemanticsModifierNode {
    fn merge_semantics(&self, config: &mut SemanticsConfiguration) {
        (self.recorder)(config);
    }
}

#[derive(Clone)]
pub struct SemanticsElement {
    recorder: Rc<dyn Fn(&mut SemanticsConfiguration)>,
}

impl SemanticsElement {
    pub fn new(recorder: Rc<dyn Fn(&mut SemanticsConfiguration)>) -> Self {
        Self { recorder }
    }
}

impl fmt::Debug for SemanticsElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SemanticsElement")
    }
}

impl PartialEq for SemanticsElement {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for SemanticsElement {}

impl Hash for SemanticsElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "semantics".hash(state);
    }
}

impl ModifierNodeElement for SemanticsElement {
    type Node = SemanticsModifierNode;

    fn create(&self) -> Self::Node {
        SemanticsModifierNode::new(self.recorder.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        node.recorder = self.recorder.clone();
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::SEMANTICS
    }

    fn always_update(&self) -> bool {
        true
    }
}

fn merge_semantics_from_node(node: &dyn ModifierNode, config: &mut SemanticsConfiguration) -> bool {
    let mut merged = false;

    if let Some(semantics) = node.as_semantics_node() {
        semantics.merge_semantics(config);
        merged = true;
    }

    node.for_each_delegate(&mut |delegate| {
        if merge_semantics_from_node(delegate, config) {
            merged = true;
        }
    });

    merged
}

/// Collects semantics contributed by a reconciled modifier chain.
pub fn collect_semantics_from_chain(chain: &ModifierNodeChain) -> Option<SemanticsConfiguration> {
    if !chain.has_capability(NodeCapabilities::SEMANTICS) {
        return None;
    }

    let mut config = SemanticsConfiguration::default();
    let mut merged = false;
    chain.for_each_node_with_capability(NodeCapabilities::SEMANTICS, |_ref, node| {
        if merge_semantics_from_node(node, &mut config) {
            merged = true;
        }
    });

    if merged { Some(config) } else { None }
}

/// Collects semantics by instantiating a temporary modifier chain from a [`Modifier`].
pub fn collect_semantics_from_modifier(modifier: &Modifier) -> Option<SemanticsConfiguration> {
    let mut handle = ModifierChainHandle::new();
    handle.update(modifier);
    collect_semantics_from_chain(handle.chain())
}
