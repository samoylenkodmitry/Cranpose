// The element a chain entry took last, kept as it is or held by the entry's
// node, and the view a comparison with a new element reads.

use std::{any::TypeId, cell::RefCell, rc::Rc};

use super::super::{
    AnyModifierElement, DynModifierElement, ModifierNode, NodeCapabilities, NodeHeld,
};

#[derive(Debug)]
pub(super) enum EntryElement {
    Kept(DynModifierElement),
    // The node keeps every value of the element: see
    // `ModifierNodeElement::FROM_NODE`.
    InNode(NodeHeld),
}

impl EntryElement {
    // How an entry keeps `element`: by its node when the node holds every
    // value of it.
    pub(super) fn new(element: DynModifierElement) -> Self {
        match element.held_by_node() {
            Some(held) if element.key().is_none() => Self::InNode(held),
            _ => Self::Kept(element),
        }
    }

    // The key of the element: an element its node holds has none.
    pub(super) fn key(&self) -> Option<u64> {
        match self {
            Self::Kept(element) => element.key(),
            Self::InNode(_) => None,
        }
    }

    pub(super) fn view<'a>(
        &'a self,
        node: &'a RefCell<dyn ModifierNode>,
        capabilities: NodeCapabilities,
    ) -> HeldElement<'a> {
        HeldElement(match self {
            Self::Kept(element) => Held::Element(element),
            Self::InNode(held) => Held::Node {
                held: *held,
                node,
                capabilities,
            },
        })
    }
}

/// An element a chain holds, as a comparison with a new element reads it:
/// the element itself, or the node that keeps every value of it.
#[derive(Clone, Copy)]
pub struct HeldElement<'a>(Held<'a>);

#[derive(Clone, Copy)]
enum Held<'a> {
    Element(&'a DynModifierElement),
    Node {
        held: NodeHeld,
        node: &'a RefCell<dyn ModifierNode>,
        capabilities: NodeCapabilities,
    },
}

impl<'a> From<&'a DynModifierElement> for HeldElement<'a> {
    fn from(element: &'a DynModifierElement) -> Self {
        Self(Held::Element(element))
    }
}

impl HeldElement<'_> {
    /// The type of the element.
    pub fn element_type(&self) -> TypeId {
        match self.0 {
            Held::Element(element) => element.element_type(),
            Held::Node { held, .. } => held.element_type(),
        }
    }

    /// The type of the node the element makes.
    pub fn node_type(&self) -> TypeId {
        match self.0 {
            Held::Element(element) => element.node_type(),
            Held::Node { held, .. } => held.node_type(),
        }
    }

    /// The capabilities of the element's node.
    pub fn capabilities(&self) -> NodeCapabilities {
        match self.0 {
            Held::Element(element) => element.capabilities(),
            Held::Node { capabilities, .. } => capabilities,
        }
    }

    /// Whether the element asks for an update of its node even when equal:
    /// one its node holds never does.
    pub fn requires_update(&self) -> bool {
        match self.0 {
            Held::Element(element) => element.requires_update(),
            Held::Node { .. } => false,
        }
    }

    /// Whether `element` is this element itself, not only an equal one.
    pub fn is(&self, element: &DynModifierElement) -> bool {
        match self.0 {
            Held::Element(held) => Rc::ptr_eq(held, element),
            Held::Node { .. } => false,
        }
    }

    /// Whether `element` is of this element's type with equal data.
    pub fn equals(&self, element: &dyn AnyModifierElement) -> bool {
        match self.0 {
            Held::Element(held) => held.equals_element(element),
            Held::Node { node, .. } => element.equals_node(&*node.borrow()),
        }
    }

    /// The hash of the element, as the element itself hashes.
    pub fn hash_code(&self) -> u64 {
        match self.0 {
            Held::Element(element) => element.hash_code(),
            Held::Node { held, node, .. } => held.hash_code(&*node.borrow()),
        }
    }

    /// The element, made again from its node when the node holds it.
    pub fn to_element(&self) -> DynModifierElement {
        match self.0 {
            Held::Element(element) => Rc::clone(element),
            Held::Node { held, node, .. } => held.element(&*node.borrow()),
        }
    }
}
