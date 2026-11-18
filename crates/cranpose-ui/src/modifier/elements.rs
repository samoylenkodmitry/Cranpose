// The elements a modifier holds, how two modifiers join, and the inspector
// metadata that travels with them.

use std::rc::Rc;

use cranpose_core::ProvidedValue;
use cranpose_foundation::DynModifierElement;
use smallvec::SmallVec;

use super::{InspectorMetadata, Modifier, spare_lists};

fn inspector_slice(inspector: &Option<Rc<Vec<InspectorMetadata>>>) -> &[InspectorMetadata] {
    inspector_slice_of(inspector.as_ref())
}

fn inspector_slice_of(inspector: Option<&Rc<Vec<InspectorMetadata>>>) -> &[InspectorMetadata] {
    inspector.map_or(&[], |list| list.as_slice())
}

/// The inspector metadata of two modifiers joined, first `first`'s, or
/// `None` when neither has any.
// Cold: only a build that records inspector metadata reaches it.
#[cold]
#[inline(never)]
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

// The elements of a modifier that holds several, or one with inspector
// metadata, in one shared allocation, which the chain's later links extend
// in place while it is the only owner.
#[derive(Default)]
pub(super) struct ManyElements {
    elements: ModifierElements,
    // `None` when no element records inspector metadata, which is always
    // outside tests and modifier debugging: no allocation.
    inspector: Option<Rc<Vec<InspectorMetadata>>>,
    provides_composition_locals: bool,
}

impl ManyElements {
    // `first` followed by `more`. Elements move into the joined chain;
    // only those another modifier also holds are copied.
    fn joined(first: ModifierKind, more: ModifierKind) -> SharedElements {
        // A new chain starts empty in its block, so its elements land there
        // and the list never moves.
        let (mut chain, mut first) = match first {
            ModifierKind::Many(chain) => (chain, ModifierKind::Empty),
            first => (SharedElements::spare(), first),
        };
        loop {
            if let Some(owned) = chain.get_mut() {
                owned.push(first);
                owned.push(more);
                return chain;
            }
            // A list another modifier holds too: its elements go as copies
            // into a block of their own.
            first = ModifierKind::Many(std::mem::replace(&mut chain, SharedElements::spare()));
        }
    }

    // Empties the list for the next use of its block. Its elements drop
    // here, where the block's drop would have dropped them.
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    // Takes in the elements of `more` after its own. Always inline, with the
    // list case out of line: a join pushes two lone elements mostly, and the
    // call of each cost more than the push (39 of 50 instructions a push on
    // the feed when it stayed out of line).
    #[inline(always)]
    fn push(&mut self, more: ModifierKind) {
        match more {
            ModifierKind::Empty => {}
            ModifierKind::One(element) => {
                self.provides_composition_locals |= element.provides_composition_locals();
                self.elements.push(element);
            }
            ModifierKind::Many(shared) => self.append(&shared),
        }
    }

    // Takes in the elements of the list `shared` after its own, as copies:
    // a plain element's copy is its value, and one way to take them in keeps
    // the code of the join small.
    #[inline(never)]
    fn append(&mut self, shared: &Self) {
        self.provides_composition_locals |= shared.provides_composition_locals;
        if shared.inspector.is_some() {
            self.inspector = merged_inspector(
                inspector_slice(&self.inspector),
                inspector_slice(&shared.inspector),
            );
        }
        for element in &shared.elements {
            self.elements.push(element.clone());
        }
    }
}

// What a modifier holds.
#[derive(Clone)]
pub(super) enum ModifierKind {
    Empty,
    // A lone element without inspector metadata, as each link of a builder
    // chain starts out: a plain element takes no allocation.
    One(DynModifierElement),
    Many(SharedElements),
}

impl ModifierKind {
    /// Whether both are the same storage, so equal without comparing.
    pub(super) fn shares_storage(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::One(a), Self::One(b)) => a.is(b),
            (Self::Many(a), Self::Many(b)) => Rc::ptr_eq(&a.0, &b.0),
            _ => false,
        }
    }
}

// The block of a list that modifiers share. When its last holder lets it
// go, the list empties and the block goes to the thread's spare blocks, for
// the next list the thread makes.
#[derive(Clone)]
pub(super) struct SharedElements(Rc<ManyElements>);

impl SharedElements {
    fn new(list: ManyElements) -> Self {
        Self(Rc::new(list))
    }

    // An empty block from the thread's spares, or a new one.
    fn spare() -> Self {
        Self(spare_lists::take().unwrap_or_default())
    }

    // The list, while this is its only holder.
    fn get_mut(&mut self) -> Option<&mut ManyElements> {
        Rc::get_mut(&mut self.0)
    }
}

impl std::ops::Deref for SharedElements {
    type Target = ManyElements;

    fn deref(&self) -> &ManyElements {
        &self.0
    }
}

impl Drop for SharedElements {
    fn drop(&mut self) {
        // The test stays inline: a holder that lets go of a block others
        // keep makes no call.
        if Rc::strong_count(&self.0) == 1 {
            spare_lists::give_back(&mut self.0);
        }
    }
}

// A modifier's elements and its inspector metadata, in order.
pub(crate) type ModifierElementIterator<'a> = std::slice::Iter<'a, DynModifierElement>;
pub(crate) type ModifierInspectorIterator<'a> = std::slice::Iter<'a, InspectorMetadata>;

impl Modifier {
    /// This modifier followed by `next`.
    ///
    /// It takes `self`: a modifier built link by link owns its elements
    /// alone, so each link joins them in place instead of copying the chain.
    /// Clone a modifier first to keep using it; the join then copies it.
    // One copy: every builder call of an app's bodies joins through it.
    #[inline(never)]
    pub fn then(self, next: Self) -> Self {
        let kind = match (self.kind, next.kind) {
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
            (_, inspector) => ModifierKind::Many(SharedElements::new(ManyElements {
                provides_composition_locals: elements
                    .iter()
                    .any(DynModifierElement::provides_composition_locals),
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
        self.as_elements().iter()
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
        inspector_slice_of(self.inspector()).iter()
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

    pub(crate) fn with_inspector_metadata(self, metadata: InspectorMetadata) -> Self {
        if metadata.is_empty() {
            return self;
        }
        let mut chain = match self.kind {
            ModifierKind::Empty => return self,
            ModifierKind::One(element) => {
                let mut chain = ManyElements::default();
                chain.push(ModifierKind::One(element));
                chain
            }
            ModifierKind::Many(shared) => {
                let mut chain = ManyElements::default();
                chain.append(&shared);
                chain
            }
        };
        let mut list = chain.inspector.as_deref().cloned().unwrap_or_default();
        list.push(metadata);
        chain.inspector = Some(Rc::new(list));
        Self {
            kind: ModifierKind::Many(SharedElements::new(chain)),
        }
    }

    pub(super) fn as_elements(&self) -> &[DynModifierElement] {
        match &self.kind {
            ModifierKind::Empty => &[],
            ModifierKind::One(element) => std::slice::from_ref(element),
            ModifierKind::Many(chain) => &chain.elements,
        }
    }
}
