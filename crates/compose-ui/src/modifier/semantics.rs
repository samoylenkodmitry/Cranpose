use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use compose_foundation::{
    DelegatableNode, ModifierNode, ModifierNodeChain, ModifierNodeElement, NodeCapabilities,
    NodeState, SemanticsConfiguration, SemanticsNode as SemanticsNodeTrait,
};

use super::{Modifier, ModifierChainHandle};

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
    pub fn new<F>(recorder: F) -> Self
    where
        F: Fn(&mut SemanticsConfiguration) + 'static,
    {
        Self {
            recorder: Rc::new(recorder),
        }
    }
}

impl fmt::Debug for SemanticsElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SemanticsElement")
    }
}

impl PartialEq for SemanticsElement {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.recorder, &other.recorder)
    }
}

impl Eq for SemanticsElement {}

impl Hash for SemanticsElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.recorder).hash(state);
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

    if merged {
        Some(config)
    } else {
        None
    }
}

/// Collects semantics by instantiating a temporary modifier chain from a [`Modifier`].
pub fn collect_semantics_from_modifier(modifier: &Modifier) -> Option<SemanticsConfiguration> {
    let mut handle = ModifierChainHandle::new();
    handle.update(modifier);
    collect_semantics_from_chain(handle.chain())
}
