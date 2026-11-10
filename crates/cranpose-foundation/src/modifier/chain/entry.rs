// An entry of a modifier chain: a node with the element it last took, or a
// plain element's words.

use std::{cell::RefCell, rc::Rc};

use super::{
    DynModifierElement, EntryElement, HeldElement, ModifierNode, NodeCapabilities, NodeMaker,
    PLAIN_WORDS_MAX, PlainFns, PlainValue, detach_node_tree,
};

// One element of a chain as the chain keeps it. The element's hash, which
// matches entries when a chain rebuilds, is worked out then: an element
// updated in place on every frame is not hashed for it.
#[derive(Debug)]
pub(super) struct ModifierNodeEntry {
    pub(super) slot: EntrySlot,
    pub(super) capabilities: NodeCapabilities,
    // The union of the entry's capabilities and those of every entry after
    // it, while no node of the chain delegates.
    pub(super) aggregate: NodeCapabilities,
}

#[derive(Debug)]
pub(super) enum EntrySlot {
    // A node and the element it last took.
    Node {
        element: EntryElement,
        node: Rc<RefCell<dyn ModifierNode>>,
    },
    // A plain element's value: its first `fns.words()` words. They fit
    // beside the table in the room a node and its element take.
    Plain {
        fns: &'static PlainFns,
        words: [u32; PLAIN_WORDS_MAX],
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
        Self {
            slot: EntrySlot::Node {
                element: EntryElement::new(element, maker),
                node,
            },
            capabilities,
            aggregate: NodeCapabilities::empty(),
        }
    }

    pub(super) fn of_plain(value: &dyn PlainValue) -> Self {
        let fns = value.fns();
        let mut words = [0; PLAIN_WORDS_MAX];
        value.write_words(&mut words[..fns.words()]);
        Self {
            slot: EntrySlot::Plain { fns, words },
            capabilities: fns.capabilities(),
            aggregate: NodeCapabilities::empty(),
        }
    }

    pub(super) fn held(&self) -> HeldElement<'_> {
        match &self.slot {
            EntrySlot::Node { element, node } => element.view(node, self.capabilities),
            EntrySlot::Plain { fns, words } => HeldElement::of_words(fns, &words[..fns.words()]),
        }
    }

    pub(super) fn node(&self) -> Option<&Rc<RefCell<dyn ModifierNode>>> {
        match &self.slot {
            EntrySlot::Node { node, .. } => Some(node),
            EntrySlot::Plain { .. } => None,
        }
    }

    // The key of the entry's element: a plain element has none.
    pub(super) fn key(&self) -> Option<u64> {
        match &self.slot {
            EntrySlot::Node { element, .. } => element.key(),
            EntrySlot::Plain { .. } => None,
        }
    }

    pub(super) fn detach(&self) {
        if let Some(node) = self.node() {
            detach_node_tree(&mut *node.borrow_mut());
        }
    }
}
