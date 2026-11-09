use std::rc::Rc;

use super::{
    DynModifierElement, EntryElement, ModifierNodeContext, ModifierNodeEntry,
    request_update_auto_invalidations,
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

/// Updates `entry` for `element` where it is, when the element is of the
/// entry's type and key and its node can take it: an element of a node's
/// type and key updates the node whatever its content, as Compose does.
/// `None` leaves the entry as it was.
pub(super) fn update_entry_in_place(
    entry: &mut ModifierNodeEntry,
    element: &DynModifierElement,
    context: &mut dyn ModifierNodeContext,
) -> Option<InPlace> {
    let held = entry.held();
    if held.element_type() != element.element_type()
        || held.node_type() != element.node_type()
        || entry.element.key() != element.key()
        || !element.can_update_node(&*entry.node.borrow())
    {
        return None;
    }
    let same_element = held.equals(element.as_ref());
    let capabilities = element.capabilities();
    let mut outcome = if capabilities == entry.capabilities {
        InPlace::LinksHold
    } else {
        InPlace::Relink
    };
    if !same_element || element.requires_update() {
        element.update_node(&mut *entry.node.borrow_mut());
        entry.element = EntryElement::new(Rc::clone(element));
        request_update_auto_invalidations(element.as_ref(), context, capabilities);
        entry
            .node
            .borrow()
            .for_each_delegate(&mut |_| outcome = InPlace::Relink);
    }
    entry.capabilities = capabilities;
    entry
        .node
        .borrow()
        .node_state()
        .set_capabilities(capabilities);
    Some(outcome)
}
