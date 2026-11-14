// How a modifier's elements compare with the elements held before them, in
// order: those of an earlier modifier, or those a node's chain holds.

use cranpose_foundation::{DynModifierElement, HeldElement, NodeCapabilities};

use super::{InPlaceReach, InPlaceUpdate, Modifier, ModifierKind};

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
    held: impl ExactSizeIterator<Item = HeldElement<'a>>,
    modifier: &Modifier,
    rule: ElementMatch,
) -> bool {
    held.len() == modifier.element_count()
        && held
            .zip(modifier.iter_elements())
            .all(|(a, b)| element_matches(a, b, rule))
}

// Whether `held` are `modifier`'s elements, one for one, as `rule` compares
// them, and one element more after them.
pub(crate) fn elements_match_but_last<'a>(
    held: impl ExactSizeIterator<Item = HeldElement<'a>>,
    modifier: &Modifier,
    rule: ElementMatch,
) -> bool {
    held.len() == modifier.element_count() + 1
        && held
            .zip(modifier.iter_elements())
            .all(|(a, b)| element_matches(a, b, rule))
}

// Whether `a` and `b` hold equal elements as `rule` compares them: those of
// one shared list without comparing them.
pub(crate) fn modifiers_match(a: &Modifier, b: &Modifier, rule: ElementMatch) -> bool {
    match (&a.kind, &b.kind) {
        (ModifierKind::Single { elements: a, .. }, ModifierKind::Single { elements: b, .. })
            if a.shares_storage(b) =>
        {
            true
        }
        _ => elements_match(a.as_elements().iter().map(HeldElement::from), b, rule),
    }
}

fn element_matches(a: HeldElement<'_>, b: &DynModifierElement, rule: ElementMatch) -> bool {
    // One element in both, as a kept chain shares them.
    if a.is(b) {
        return true;
    }
    match rule {
        ElementMatch::Equal => !a.requires_update() && !b.requires_update() && a.equals(&**b),
        ElementMatch::Structural => {
            (a.element_type() == b.element_type()
                && a.capabilities() == NodeCapabilities::DRAW
                && b.capabilities() == NodeCapabilities::DRAW)
                || a.equals(&**b)
        }
    }
}

// How a modifier's elements compare with the elements a chain holds, from
// one walk of both: what a set of the modifier needs to know.
pub(crate) struct SetMatch {
    // Whether the chain holds the modifier's elements as `ElementMatch::Equal`
    // compares them.
    pub(crate) equal: bool,
    // Whether it holds them as `ElementMatch::Structural` compares them.
    pub(crate) structural: bool,
    // The update in place from the chain's elements to the modifier's, when
    // they line up one for one and are equal except elements that
    // `InPlaceReach::of` accepts: how far it reaches, and the elements it
    // updates. `None` when another element differs, or the chain is too
    // long to name its elements; an element that asks for an update on
    // every set counts as different.
    pub(crate) in_place: Option<InPlaceUpdate>,
}

// Compares `held` with `modifier`'s elements, each pair once.
pub(crate) fn match_for_set<'a>(
    held: impl ExactSizeIterator<Item = HeldElement<'a>>,
    modifier: &Modifier,
) -> SetMatch {
    let count = held.len();
    let mut matched = SetMatch {
        equal: count == modifier.element_count(),
        structural: count == modifier.element_count(),
        in_place: (count == modifier.element_count() && count <= u64::BITS as usize).then_some(
            InPlaceUpdate {
                reach: InPlaceReach::Node,
                elements: 0,
            },
        ),
    };
    if !matched.structural {
        return matched;
    }
    for (index, (a, b)) in held.zip(modifier.iter_elements()).enumerate() {
        // Most pairs are equal values the chain keeps: one compare tells,
        // and an equal element asks for no update either.
        if a.is_kept_by_value() && a.equals(&**b) {
            continue;
        }
        // One element in both, as a kept chain shares them.
        let shared = a.is(b);
        let asks_update = a.requires_update() || b.requires_update();
        // Most pairs are equal: they leave everything as it is. An update
        // in place skips them, as it would find them equal again.
        if shared || (!asks_update && !a.is_kept_by_value() && a.equals(&**b)) {
            if asks_update {
                matched.in_place = None;
            }
            continue;
        }
        matched.differ_at(index, (a, b), asks_update);
        if !matched.structural && matched.in_place.is_none() {
            break;
        }
    }
    matched
}

impl SetMatch {
    // Takes in the pair `a` and `b` at `index`, which differ or ask for an
    // update on every set, as `asks_update` tells.
    fn differ_at(
        &mut self,
        index: usize,
        (a, b): (HeldElement<'_>, &DynModifierElement),
        asks_update: bool,
    ) {
        self.equal = false;
        let same = asks_update && a.equals(&**b);
        let element_type = a.element_type();
        let same_type = b.element_type() == element_type;
        self.structural &= same
            || (same_type
                && a.capabilities() == NodeCapabilities::DRAW
                && b.capabilities() == NodeCapabilities::DRAW);
        if let Some(update) = &mut self.in_place {
            match InPlaceReach::of(element_type).filter(|_| same_type && !asks_update) {
                Some(InPlaceReach::Node) => update.elements |= 1 << index,
                Some(InPlaceReach::Measure) => {
                    // A parent data change of another element reaches further.
                    if update.reach == InPlaceReach::Node {
                        update.reach = InPlaceReach::Measure;
                    }
                    update.elements |= 1 << index;
                }
                Some(InPlaceReach::Resolved) => {
                    update.reach = InPlaceReach::Resolved;
                    update.elements |= 1 << index;
                }
                None => self.in_place = None,
            }
        }
    }
}
