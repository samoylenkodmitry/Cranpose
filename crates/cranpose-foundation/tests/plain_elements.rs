// Plain elements live in a chain as words, among elements that make nodes.
// A chain reconciled with other elements gives back each plain value at its
// new place, keeps the nodes it can and asks for a redraw of a changed value.

use std::{hash::Hash, rc::Rc};

use cranpose_foundation::{
    BasicModifierNodeContext, DelegatableNode, DynModifierElement, InvalidationKind, ModifierNode,
    ModifierNodeChain, ModifierNodeElement, NodeCapabilities, NodeState, PlainElement,
    modifier_element, plain_element,
};

#[derive(Debug, Clone, Copy, PartialEq, Hash)]
struct Tint(u32);

impl PlainElement for Tint {
    const WORDS: usize = 1;
    const CAPABILITIES: NodeCapabilities = NodeCapabilities::DRAW;

    fn to_words(&self, words: &mut [u32]) {
        words[0] = self.0;
    }

    fn from_words(words: &[u32]) -> Self {
        Self(words[0])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Hash)]
struct Span(u32, u32);

impl PlainElement for Span {
    const WORDS: usize = 2;
    const CAPABILITIES: NodeCapabilities = NodeCapabilities::DRAW;

    fn to_words(&self, words: &mut [u32]) {
        words.copy_from_slice(&[self.0, self.1]);
    }

    fn from_words(words: &[u32]) -> Self {
        Self(words[0], words[1])
    }
}

#[derive(Debug)]
struct Marker {
    state: NodeState,
    tag: u32,
}

impl DelegatableNode for Marker {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for Marker {}

#[derive(Debug, Clone, Copy, PartialEq, Hash)]
struct Mark(u32);

impl ModifierNodeElement for Mark {
    type Node = Marker;

    fn create(&self) -> Marker {
        Marker {
            state: NodeState::new(),
            tag: self.0,
        }
    }

    fn update(&self, node: &mut Marker) {
        node.tag = self.0;
    }
}

// What the chain holds at each place: a tint, a span or a mark's tag.
fn read(chain: &ModifierNodeChain) -> Vec<String> {
    (0..chain.len())
        .map(|index| {
            if let Some(Tint(value)) = chain.plain::<Tint>(index) {
                format!("tint {value}")
            } else if let Some(Span(from, to)) = chain.plain::<Span>(index) {
                format!("span {from}-{to}")
            } else if let Some(marker) = chain.node::<Marker>(index) {
                format!("mark {}", marker.tag)
            } else {
                "unknown".to_owned()
            }
        })
        .collect()
}

fn tint(value: u32) -> DynModifierElement {
    plain_element(Tint(value))
}

fn span(from: u32, to: u32) -> DynModifierElement {
    plain_element(Span(from, to))
}

fn mark(tag: u32) -> DynModifierElement {
    modifier_element(Mark(tag))
}

#[test]
fn plain_values_follow_their_elements_through_every_reconcile() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let steps: [(&[DynModifierElement], &[&str]); 7] = [
        (
            &[tint(1), mark(7), span(2, 3), tint(4)],
            &["tint 1", "mark 7", "span 2-3", "tint 4"],
        ),
        // Values change where they are.
        (
            &[tint(9), mark(7), span(5, 6), tint(4)],
            &["tint 9", "mark 7", "span 5-6", "tint 4"],
        ),
        // The order changes.
        (
            &[span(5, 6), tint(4), mark(7), tint(9)],
            &["span 5-6", "tint 4", "mark 7", "tint 9"],
        ),
        // One leaves from the middle.
        (
            &[span(5, 6), mark(7), tint(9)],
            &["span 5-6", "mark 7", "tint 9"],
        ),
        // Two come in, one of them first.
        (
            &[tint(8), span(5, 6), mark(7), span(1, 1), tint(9)],
            &["tint 8", "span 5-6", "mark 7", "span 1-1", "tint 9"],
        ),
        // The tail leaves.
        (&[tint(8), span(5, 6)], &["tint 8", "span 5-6"]),
        (&[], &[]),
    ];
    for (elements, expected) in steps {
        chain.update_from_slice(elements, &mut context);
        assert_eq!(read(&chain), expected);
        let held: Vec<bool> = chain
            .held_elements()
            .zip(elements)
            .map(|(held, element)| held.equals(&**element))
            .collect();
        assert_eq!(held, vec![true; elements.len()], "{expected:?}");
    }
}

#[test]
fn a_plain_element_makes_no_node_and_keeps_the_nodes_beside_it() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    chain.update_from_slice(&[tint(1), mark(7)], &mut context);
    assert!(
        chain.get_node_rc(0).is_none(),
        "a plain element has no node"
    );
    let node = Rc::clone(chain.get_node_rc(1).expect("the mark's node"));

    chain.update_from_slice(&[mark(7), tint(2)], &mut context);

    let kept = chain.get_node_rc(0).expect("the mark's node");
    assert!(Rc::ptr_eq(&node, kept), "the mark keeps its node");
    assert_eq!(chain.capabilities(), NodeCapabilities::DRAW);
}

#[test]
fn a_changed_plain_value_asks_for_a_redraw_and_an_equal_one_for_nothing() {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    chain.update_from_slice(&[tint(1), span(2, 3)], &mut context);
    context.clear_invalidations();

    chain.update_from_slice(&[tint(1), span(2, 3)], &mut context);
    assert!(context.invalidations().is_empty(), "equal values");

    chain.update_from_slice(&[tint(1), span(2, 4)], &mut context);
    let kinds: Vec<InvalidationKind> = context
        .invalidations()
        .iter()
        .map(|invalidation| invalidation.kind())
        .collect();
    assert_eq!(kinds, [InvalidationKind::Draw]);
}
