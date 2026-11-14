// An entry of a modifier chain: a node with the element it last took, or a
// plain element's words.

use std::{cell::RefCell, rc::Rc};

use super::{
    DynModifierElement, EntryElement, HeldElement, ModifierNode, NodeCapabilities, NodeMaker,
    PLAIN_WORDS_MAX, PlainFns, PlainLayoutFns, PlainValue, detach_node_tree,
};

// One element of a chain as the chain keeps it. The element's hash, which
// matches entries when a chain rebuilds, is worked out then: an element
// updated in place on every frame is not hashed for it.
//
// Each kind keeps `aggregate`, the union of the entry's capabilities and
// those of every entry after it, while no node of the chain delegates. A
// plain entry fits in the room of a node entry, beside the pattern a node's
// pointer never takes, so an entry of either kind takes 40 bytes.
#[derive(Debug)]
pub(super) enum ModifierNodeEntry {
    // A node and the element it last took.
    Node {
        element: EntryElement,
        node: Rc<RefCell<dyn ModifierNode>>,
        capabilities: NodeCapabilities,
        aggregate: NodeCapabilities,
    },
    // A plain element's value: its first `fns.words()` words. Its
    // capabilities are its type's.
    Plain {
        fns: &'static PlainFns,
        words: [u32; PLAIN_WORDS_MAX],
        aggregate: NodeCapabilities,
    },
}

impl ModifierNodeEntry {
    pub(super) fn of_node(
        element: DynModifierElement,
        maker: &dyn NodeMaker,
        capabilities: NodeCapabilities,
    ) -> Self {
        let node = maker.create_node();
        node.borrow().node_state().set_capabilities(capabilities);
        Self::Node {
            element: EntryElement::new(element, maker),
            node,
            capabilities,
            aggregate: NodeCapabilities::empty(),
        }
    }

    pub(super) fn of_plain(value: &dyn PlainValue) -> Self {
        let fns = value.fns();
        let mut words = [0; PLAIN_WORDS_MAX];
        value.write_words(&mut words[..fns.words()]);
        Self::Plain {
            fns,
            words,
            aggregate: NodeCapabilities::empty(),
        }
    }

    pub(super) fn capabilities(&self) -> NodeCapabilities {
        match self {
            Self::Node { capabilities, .. } => *capabilities,
            Self::Plain { fns, .. } => fns.capabilities(),
        }
    }

    pub(super) fn aggregate(&self) -> NodeCapabilities {
        match self {
            Self::Node { aggregate, .. } | Self::Plain { aggregate, .. } => *aggregate,
        }
    }

    pub(super) fn set_aggregate(&mut self, union: NodeCapabilities) {
        match self {
            Self::Node { aggregate, .. } | Self::Plain { aggregate, .. } => *aggregate = union,
        }
    }

    pub(super) fn held(&self) -> HeldElement<'_> {
        match self {
            Self::Node {
                element,
                node,
                capabilities,
                ..
            } => element.view(node, *capabilities),
            Self::Plain { fns, words, .. } => HeldElement::of_words(fns, &words[..fns.words()]),
        }
    }

    // The words of a plain entry with its type's table.
    pub(super) fn plain_words(&self) -> Option<(&'static PlainFns, &[u32])> {
        match self {
            Self::Plain { fns, words, .. } => Some((fns, &words[..fns.words()])),
            Self::Node { .. } => None,
        }
    }

    // How a plain entry's element measures, with every word of the entry:
    // the words past the element's own are zero.
    pub(super) fn plain_layout(
        &self,
    ) -> Option<(Option<&'static PlainLayoutFns>, &[u32; PLAIN_WORDS_MAX])> {
        match self {
            Self::Plain { fns, words, .. } => Some((fns.layout(), words)),
            Self::Node { .. } => None,
        }
    }

    pub(super) fn node(&self) -> Option<&Rc<RefCell<dyn ModifierNode>>> {
        match self {
            Self::Node { node, .. } => Some(node),
            Self::Plain { .. } => None,
        }
    }

    // The key of the entry's element: a plain element has none.
    pub(super) fn key(&self) -> Option<u64> {
        match self {
            Self::Node { element, .. } => element.key(),
            Self::Plain { .. } => None,
        }
    }

    pub(super) fn detach(&self) {
        if let Some(node) = self.node() {
            detach_node_tree(&mut *node.borrow_mut());
        }
    }
}
