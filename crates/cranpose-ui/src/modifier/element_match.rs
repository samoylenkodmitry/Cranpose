// How a modifier's elements compare with the elements held before them, in
// order: those of an earlier modifier.

use std::rc::Rc;

use cranpose_foundation::{DynModifierElement, NodeCapabilities};

use super::{InPlaceReach, InPlaceUpdate, Modifier};

// Which differences between two elements a comparison passes over.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ElementMatch {
    // None: two elements match when they are one element or compare equal,
    // and an element that asks for an update on every set matches only
    // itself.
    Equal,
    // Those layout does not read: two elements of one type that only draw
    // match whatever they draw.
    Structural,
}

// Whether `held` are `modifier`'s elements, one for one, as `rule` compares
// them.
pub(crate) fn elements_match<'a>(
    held: impl ExactSizeIterator<Item = &'a DynModifierElement>,
    modifier: &Modifier,
    rule: ElementMatch,
) -> bool {
    held.len() == modifier.element_count()
        && held
            .zip(modifier.iter_elements())
            .all(|(a, b)| element_matches(a, b, rule))
}

fn element_matches(a: &DynModifierElement, b: &DynModifierElement, rule: ElementMatch) -> bool {
    // One element in both, as a kept chain shares them.
    if Rc::ptr_eq(a, b) {
        return true;
    }
    match rule {
        ElementMatch::Equal => {
            !a.requires_update() && !b.requires_update() && a.equals_element(&**b)
        }
        ElementMatch::Structural => {
            (a.element_type() == b.element_type()
                && a.capabilities() == NodeCapabilities::DRAW
                && b.capabilities() == NodeCapabilities::DRAW)
                || a.equals_element(&**b)
        }
    }
}

// The update in place from `held` to `modifier`'s elements, when they line
// up one for one and are equal except elements that `InPlaceReach::of`
// accepts: how far it reaches, and the elements it updates. `None` when
// another element differs, or the chain is too long to name its elements;
// an element that asks for an update on every set counts as different.
pub(crate) fn in_place_update<'a>(
    held: impl ExactSizeIterator<Item = &'a DynModifierElement>,
    modifier: &Modifier,
) -> Option<InPlaceUpdate> {
    let count = held.len();
    if count != modifier.element_count() || count > u64::BITS as usize {
        return None;
    }
    let mut update = InPlaceUpdate {
        reach: InPlaceReach::Node,
        elements: 0,
    };
    for (index, (a, b)) in held.zip(modifier.iter_elements()).enumerate() {
        if a.requires_update() || b.requires_update() {
            return None;
        }
        let element_type = a.element_type();
        match InPlaceReach::of(element_type) {
            // A text updates in place whether it changed or not.
            Some(InPlaceReach::Node) if b.element_type() == element_type => {
                update.elements |= 1 << index;
            }
            Some(InPlaceReach::Resolved) if b.element_type() == element_type => {
                if !same_element(a, b) {
                    update.reach = InPlaceReach::Resolved;
                    update.elements |= 1 << index;
                }
            }
            _ => {
                if !same_element(a, b) {
                    return None;
                }
            }
        }
    }
    Some(update)
}

// Whether `a` and `b` are equal elements: one element, as a kept chain
// shares them, or two that compare equal.
fn same_element(a: &DynModifierElement, b: &DynModifierElement) -> bool {
    Rc::ptr_eq(a, b) || a.equals_element(&**b)
}
