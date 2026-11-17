// The elements a modifier holds, how two modifiers join, and the inspector
// metadata that travels with them.

use std::rc::Rc;

use cranpose_core::ProvidedValue;
use cranpose_foundation::DynModifierElement;
use smallvec::SmallVec;

use super::{InspectorMetadata, Modifier};

fn inspector_slice(inspector: &Option<Rc<Vec<InspectorMetadata>>>) -> &[InspectorMetadata] {
    inspector_slice_of(inspector.as_ref())
}

fn inspector_slice_of(inspector: Option<&Rc<Vec<InspectorMetadata>>>) -> &[InspectorMetadata] {
    inspector.map_or(&[], |list| list.as_slice())
}

/// The inspector metadata of two modifiers joined, first `first`'s, or
/// `None` when neither has any.
fn merged_inspector(
    first: &[InspectorMetadata],
    second: &[InspectorMetadata],
) -> Option<Rc<Vec<InspectorMetadata>>> {
    if first.is_empty() && second.is_empty() {
        return None;
    }
    Some(Rc::new(first.iter().chain(second).cloned().collect()))
}

// A modifier's elements, when it holds several: a chain of up to four
// lives inline in its one shared allocation.
pub(super) type ModifierElements = SmallVec<[DynModifierElement; 4]>;

// How many emptied lists of let-go modifiers a thread keeps for its next
// joins: a body that composes again builds its modifiers again and drops
// each once its node took it in, so a few cover the nesting of bodies.
const SPARE_LISTS: usize = 4;

// The elements of a modifier that holds several, or one with inspector
// metadata, in one shared allocation, which the chain's later links extend
// in place while it is the only owner.
#[derive(Clone, Default)]
pub(crate) struct ManyElements {
    elements: ModifierElements,
    // `None` when no element records inspector metadata, which is always
    // outside tests and modifier debugging: no allocation.
    inspector: Option<Rc<Vec<InspectorMetadata>>>,
    provides_composition_locals: bool,
}

impl ManyElements {
    // `first` followed by `more`. Elements move into the joined chain;
    // only those another modifier also holds are copied. A chain started
    // here takes a list another modifier let go when the thread has one.
    fn joined(first: ModifierKind, more: ModifierKind) -> Rc<Self> {
        let (mut chain, first) = match first {
            ModifierKind::Many(chain) => (chain, ModifierKind::Empty),
            first => (spare_list(), first),
        };
        match Rc::get_mut(&mut chain) {
            Some(owned) => {
                owned.push(first);
                owned.push(more);
            }
            None => {
                let mut copy = Self::clone(&chain);
                copy.push(first);
                copy.push(more);
                chain = Rc::new(copy);
            }
        }
        chain
    }

    // Takes in the elements of `more` after its own.
    fn push(&mut self, more: ModifierKind) {
        match more {
            ModifierKind::Empty => {}
            ModifierKind::One(element) => {
                self.provides_composition_locals |= element.provides_composition_locals();
                self.elements.push(element);
            }
            ModifierKind::Many(mut shared) => {
                self.provides_composition_locals |= shared.provides_composition_locals;
                if shared.inspector.is_some() {
                    self.inspector = merged_inspector(
                        inspector_slice(&self.inspector),
                        inspector_slice(&shared.inspector),
                    );
                }
                match Rc::get_mut(&mut shared) {
                    Some(owned) => self.elements.append(&mut owned.elements),
                    None => self.elements.extend(shared.elements.iter().cloned()),
                }
                release(shared);
            }
        }
    }
}

// A list for a new chain: one another modifier let go, or a new one.
fn spare_list() -> Rc<ManyElements> {
    crate::thread_state::with(|state| state.spare_modifier_lists.borrow_mut().pop())
        .unwrap_or_default()
}

// Lets `list` go: the last holder of a list that kept its elements inline
// empties it for the thread's next join, while the thread keeps fewer than
// `SPARE_LISTS`. Out of line, so a drop of a modifier stays small.
#[inline(never)]
fn release(mut list: Rc<ManyElements>) {
    let Some(owned) = Rc::get_mut(&mut list) else {
        return;
    };
    if owned.elements.spilled() {
        return;
    }
    owned.elements.clear();
    owned.inspector = None;
    owned.provides_composition_locals = false;
    // A thread that ends keeps none.
    let _ = crate::thread_state::try_with(|state| {
        let mut spare = state.spare_modifier_lists.borrow_mut();
        if spare.len() < SPARE_LISTS {
            spare.push(list);
        }
    });
}

// What a modifier holds.
#[derive(Clone)]
pub(super) enum ModifierKind {
    Empty,
    // A lone element without inspector metadata, as each link of a builder
    // chain starts out: a plain element takes no allocation.
    One(DynModifierElement),
    Many(Rc<ManyElements>),
}

impl ModifierKind {
    /// Whether both are the same storage, so equal without comparing.
    pub(super) fn shares_storage(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::One(a), Self::One(b)) => a.is(b),
            (Self::Many(a), Self::Many(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

pub(crate) struct ModifierElementIterator<'a> {
    inner: std::slice::Iter<'a, DynModifierElement>,
}

impl<'a> Iterator for ModifierElementIterator<'a> {
    type Item = &'a DynModifierElement;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for ModifierElementIterator<'_> {}

pub(crate) struct ModifierInspectorIterator<'a> {
    inner: std::slice::Iter<'a, InspectorMetadata>,
}

impl<'a> Iterator for ModifierInspectorIterator<'a> {
    type Item = &'a InspectorMetadata;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for ModifierInspectorIterator<'_> {}

impl Modifier {
    /// This modifier followed by `next`.
    ///
    /// It takes `self`: a modifier built link by link owns its elements
    /// alone, so each link joins them in place instead of copying the chain.
    /// Clone a modifier first to keep using it; the join then copies it.
    pub fn then(mut self, mut next: Self) -> Self {
        let kind = match (self.take_kind(), next.take_kind()) {
            (ModifierKind::Empty, kind) | (kind, ModifierKind::Empty) => kind,
            (first, more) => ModifierKind::Many(ManyElements::joined(first, more)),
        };
        Self { kind }
    }

    // A modifier of `elements`, with the inspector metadata `inspector`.
    pub(super) fn of_elements(
        mut elements: ModifierElements,
        inspector: Option<Rc<Vec<InspectorMetadata>>>,
    ) -> Self {
        let kind = match (elements.len(), inspector) {
            (0, _) => ModifierKind::Empty,
            (1, None) => match elements.pop() {
                Some(element) => ModifierKind::One(element),
                None => ModifierKind::Empty,
            },
            (_, inspector) => ModifierKind::Many(Rc::new(ManyElements {
                provides_composition_locals: elements
                    .iter()
                    .any(|element| element.provides_composition_locals()),
                elements,
                inspector,
            })),
        };
        Self { kind }
    }

    /// How many elements the modifier holds.
    pub(crate) fn element_count(&self) -> usize {
        self.as_elements().len()
    }

    pub(crate) fn iter_elements(&self) -> ModifierElementIterator<'_> {
        ModifierElementIterator {
            inner: self.as_elements().iter(),
        }
    }

    pub(crate) fn provided_composition_locals(&self) -> Vec<ProvidedValue> {
        let provides = match &self.kind {
            ModifierKind::Empty => false,
            ModifierKind::One(element) => element.provides_composition_locals(),
            ModifierKind::Many(chain) => chain.provides_composition_locals,
        };
        if !provides {
            return Vec::new();
        }
        self.iter_elements()
            .flat_map(|element| element.provided_composition_locals())
            .collect()
    }

    // The modifier's inspector metadata, `None` when it carries none.
    pub(super) fn inspector(&self) -> Option<&Rc<Vec<InspectorMetadata>>> {
        match &self.kind {
            ModifierKind::Empty | ModifierKind::One(_) => None,
            ModifierKind::Many(chain) => chain.inspector.as_ref(),
        }
    }

    pub(crate) fn iter_inspector_metadata(&self) -> ModifierInspectorIterator<'_> {
        ModifierInspectorIterator {
            inner: inspector_slice_of(self.inspector()).iter(),
        }
    }

    #[cfg(test)]
    pub(crate) fn elements(&self) -> Vec<DynModifierElement> {
        self.as_elements().to_vec()
    }

    pub(crate) fn inspector_metadata(&self) -> Vec<InspectorMetadata> {
        inspector_slice_of(self.inspector()).to_vec()
    }

    /// A modifier of the one `element`.
    pub(crate) fn from_dyn_element(element: DynModifierElement) -> Self {
        Self {
            kind: ModifierKind::One(element),
        }
    }

    pub(crate) fn with_inspector_metadata(mut self, metadata: InspectorMetadata) -> Self {
        if metadata.is_empty() {
            return self;
        }
        let mut chain = match self.take_kind() {
            ModifierKind::Empty => return self,
            ModifierKind::One(element) => {
                let mut chain = ManyElements::default();
                chain.push(ModifierKind::One(element));
                chain
            }
            ModifierKind::Many(chain) => Rc::unwrap_or_clone(chain),
        };
        let mut list = chain.inspector.as_deref().cloned().unwrap_or_default();
        list.push(metadata);
        chain.inspector = Some(Rc::new(list));
        Self {
            kind: ModifierKind::Many(Rc::new(chain)),
        }
    }

    // What the modifier holds, leaving it empty.
    fn take_kind(&mut self) -> ModifierKind {
        std::mem::replace(&mut self.kind, ModifierKind::Empty)
    }

    pub(super) fn as_elements(&self) -> &[DynModifierElement] {
        match &self.kind {
            ModifierKind::Empty => &[],
            ModifierKind::One(element) => std::slice::from_ref(element),
            ModifierKind::Many(chain) => &chain.elements,
        }
    }
}

impl Drop for Modifier {
    fn drop(&mut self) {
        if let ModifierKind::Many(list) = self.take_kind() {
            release(list);
        }
    }
}
