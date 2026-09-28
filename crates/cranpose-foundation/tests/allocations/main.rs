use std::{
    alloc::System,
    hash::{Hash, Hasher},
};

use cranpose_foundation::{
    BasicModifierNodeContext, DelegatableNode, DynModifierElement, ModifierNode, ModifierNodeChain,
    ModifierNodeElement, NodeState, modifier_element,
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const UPDATES: usize = 1_000;

#[derive(Debug)]
struct ValueNode {
    value: u32,
    state: NodeState,
}

impl DelegatableNode for ValueNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for ValueNode {}

/// An element like a Text's: one node whose content changes with the element.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ValueElement(u32);

impl Hash for ValueElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl ModifierNodeElement for ValueElement {
    type Node = ValueNode;

    fn create(&self) -> ValueNode {
        ValueNode {
            value: self.0,
            state: NodeState::new(),
        }
    }

    fn update(&self, node: &mut ValueNode) {
        node.value = self.0;
    }
}

/// A padding-like element whose content never changes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FixedElement;

#[derive(Debug)]
struct FixedNode {
    state: NodeState,
}

impl DelegatableNode for FixedNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for FixedNode {}

impl ModifierNodeElement for FixedElement {
    type Node = FixedNode;

    fn create(&self) -> FixedNode {
        FixedNode {
            state: NodeState::new(),
        }
    }

    fn update(&self, _node: &mut FixedNode) {}
}

fn elements(value: u32) -> Vec<DynModifierElement> {
    vec![
        modifier_element(FixedElement),
        modifier_element(ValueElement(value)),
    ]
}

/// A reused list row recomposing with a new text: every element keeps its
/// type and place, one changes its content. The chain updates that node where
/// it is, as Compose does, without building anything.
#[test]
fn a_chain_whose_element_changes_content_in_place_updates_without_allocating() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    chain.update_from_slice(&elements(0), &mut context);
    let updates: Vec<_> = (1..=UPDATES as u32).map(elements).collect();
    context.clear_invalidations();
    chain.update_from_slice(&updates[0], &mut context);
    context.clear_invalidations();

    let region = Region::new(GLOBAL);
    for update in &updates[1..] {
        chain.update_from_slice(update, &mut context);
        context.clear_invalidations();
    }
    let change = region.change();

    assert_eq!(
        chain.node::<ValueNode>(1).map(|node| node.value),
        Some(UPDATES as u32)
    );
    assert_eq!(change.allocations, 0, "{change:?}");
}
