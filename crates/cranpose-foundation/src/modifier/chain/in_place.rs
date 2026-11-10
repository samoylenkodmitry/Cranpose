use std::rc::Rc;

use super::{
    DynModifierElement, ElementMaker, EntryElement, EntrySlot, ModifierNodeContext,
    ModifierNodeEntry, request_update_auto_invalidations,
};

/// What updating an entry in place did to the chain's links.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum InPlace {
    /// The links and capability aggregates stand as they were.
    LinksHold,
    /// The entry's capabilities changed, or its updated node delegates and
    /// may have changed its delegates: the chain links again.
    Relink,
}

// Updates `entry` for `element` where it is, when the element is of the
// entry's type and key and the entry can take it: an element of a node's
// type and key updates the node whatever its content, as Compose does, and
// a plain element writes its words over the entry's in `payload`. `None`
// leaves the entry as it was.
pub(super) fn update_entry_in_place(
    entry: &mut ModifierNodeEntry,
    payload: &mut [u32],
    element: &DynModifierElement,
    context: &mut dyn ModifierNodeContext,
) -> Option<InPlace> {
    match (&mut entry.slot, element.maker()) {
        (EntrySlot::Plain { fns, at }, ElementMaker::Plain(value)) => {
            if fns.element_type() != value.fns().element_type() {
                return None;
            }
            let words = &mut payload[*at as usize..][..fns.words()];
            if !value.equals_words(words) {
                value.write_words(words);
                request_update_auto_invalidations(&**element, context, entry.capabilities);
            }
            Some(InPlace::LinksHold)
        }
        (
            EntrySlot::Node {
                element: held,
                node,
            },
            ElementMaker::Node(maker),
        ) => {
            let view = held.view(node, entry.capabilities);
            if view.element_type() != element.element_type()
                || view.node_type() != maker.node_type()
                || held.key() != element.key()
                || !maker.can_update_node(&*node.borrow())
            {
                return None;
            }
            let same_element = view.equals(&**element);
            let capabilities = element.capabilities();
            let mut outcome = if capabilities == entry.capabilities {
                InPlace::LinksHold
            } else {
                InPlace::Relink
            };
            if !same_element || element.requires_update() {
                maker.update_node(&mut *node.borrow_mut());
                *held = EntryElement::new(Rc::clone(element), maker);
                request_update_auto_invalidations(&**element, context, capabilities);
                node.borrow()
                    .for_each_delegate(&mut |_| outcome = InPlace::Relink);
            }
            entry.capabilities = capabilities;
            node.borrow().node_state().set_capabilities(capabilities);
            Some(outcome)
        }
        _ => None,
    }
}
