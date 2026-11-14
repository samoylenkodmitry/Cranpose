// The element a chain entry took last, kept as it is, held by the entry's
// node or written as words in the entry, and the view a comparison
// with a new element reads.

use std::{any::TypeId, cell::RefCell, rc::Rc};

use super::super::{
    AnyModifierElement, DynModifierElement, ElementMaker, ModifierNode, NodeCapabilities, NodeHeld,
    NodeMaker, PlainElement, PlainFns,
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
    pub(super) fn new(element: DynModifierElement, maker: &dyn NodeMaker) -> Self {
        match maker.held_by_node() {
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
    Plain {
        fns: &'static PlainFns,
        words: &'a [u32],
    },
}

impl<'a> HeldElement<'a> {
    pub(super) fn of_words(fns: &'static PlainFns, words: &'a [u32]) -> Self {
        Self(Held::Plain { fns, words })
    }
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
            Held::Plain { fns, .. } => fns.element_type(),
        }
    }

    /// The type of the node the element makes: its own type for a plain
    /// element, which makes none.
    pub fn node_type(&self) -> TypeId {
        match self.0 {
            Held::Element(element) => match element.maker() {
                ElementMaker::Node(maker) => maker.node_type(),
                ElementMaker::Plain(value) => value.fns().element_type(),
            },
            Held::Node { held, .. } => held.node_type(),
            Held::Plain { fns, .. } => fns.element_type(),
        }
    }

    /// The capabilities of the element's node.
    pub fn capabilities(&self) -> NodeCapabilities {
        match self.0 {
            Held::Element(element) => element.capabilities(),
            Held::Node { capabilities, .. } => capabilities,
            Held::Plain { fns, .. } => fns.capabilities(),
        }
    }

    /// Whether the element asks for an update of its node even when equal:
    /// one its node holds never does, nor does a plain one.
    pub fn requires_update(&self) -> bool {
        match self.0 {
            Held::Element(element) => element.requires_update(),
            Held::Node { .. } | Held::Plain { .. } => false,
        }
    }

    /// Whether the chain keeps the element's values, in its node or as
    /// words, in place of the element. Such an element asks for no update
    /// when equal, and an element equal to it is of its type, so that one
    /// asks for none either.
    pub fn is_kept_by_value(&self) -> bool {
        matches!(self.0, Held::Node { .. } | Held::Plain { .. })
    }

    /// Whether `element` is this element itself, not only an equal one.
    pub fn is(&self, element: &DynModifierElement) -> bool {
        match self.0 {
            Held::Element(held) => Rc::ptr_eq(held, element),
            Held::Node { .. } | Held::Plain { .. } => false,
        }
    }

    /// Whether `element` is of this element's type with equal data.
    pub fn equals(&self, element: &dyn AnyModifierElement) -> bool {
        match self.0 {
            Held::Element(held) => held.equals_element(element),
            Held::Node { node, .. } => element.equals_node(&*node.borrow()),
            Held::Plain { fns, words } => element.equals_plain(fns, words),
        }
    }

    /// The element's value, when the chain keeps it as words of an `E`.
    #[inline]
    pub fn plain<E: PlainElement>(&self) -> Option<E> {
        match self.0 {
            Held::Plain { fns, words } => fns.read(words),
            Held::Element(_) | Held::Node { .. } => None,
        }
    }

    /// The hash of the element, as the element itself hashes.
    pub fn hash_code(&self) -> u64 {
        match self.0 {
            Held::Element(element) => element.hash_code(),
            Held::Node { held, node, .. } => held.hash_code(&*node.borrow()),
            Held::Plain { fns, words } => fns.hash_code(words),
        }
    }

    /// The element, made again from its node when the node holds it.
    pub fn to_element(&self) -> DynModifierElement {
        match self.0 {
            Held::Element(element) => Rc::clone(element),
            Held::Node { held, node, .. } => held.element(&*node.borrow()),
            Held::Plain { fns, words } => fns.element(words),
        }
    }
}
