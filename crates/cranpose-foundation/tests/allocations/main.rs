use std::{
    alloc::System,
    hash::{Hash, Hasher},
};

use cranpose_foundation::{
    BasicModifierNodeContext, DelegatableNode, DynModifierElement, ModifierNode, ModifierNodeChain,
    ModifierNodeElement, NodeCapabilities, NodeState, modifier_element,
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

/// An element that draws, so each update of it invalidates drawing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DrawnElement(u32);

impl ModifierNodeElement for DrawnElement {
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

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}

/// A node's invalidations are handed over after every update: at most one
/// per kind, so they travel without a heap allocation.
fn invalidations_taken_after_every_update_allocate_nothing() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    chain.update_from_slice(&[modifier_element(DrawnElement(0))], &mut context);
    drop(context.take_invalidations());
    let updates: Vec<_> = (1..=UPDATES as u32)
        .map(|value| [modifier_element(DrawnElement(value))])
        .collect();

    let region = Region::new(GLOBAL);
    let mut handed_over = 0;
    for update in &updates {
        chain.update_from_slice(update, &mut context);
        handed_over += context.take_invalidations().len();
    }
    let change = region.change();

    assert!(handed_over >= UPDATES, "every update invalidates drawing");
    assert_eq!(change.allocations, 0, "{change:?}");
}

/// One test, so no other test's allocations land in these regions: the
/// counting allocator counts every thread.
#[test]
fn modifier_chain_updates_stay_within_their_allocation_budgets() {
    a_chain_whose_element_changes_content_in_place_updates_without_allocating();
    invalidations_taken_after_every_update_allocate_nothing();
}
